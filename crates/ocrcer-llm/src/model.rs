//! The Qwen decoder forward pass, one function for every stage, called once
//! per token whether that token is part of the prompt or a generated
//! continuation — a "decode step" is a prefill of length one against a
//! nonempty cache, not a second implementation.
//!
//! Qwen3's per-head Q/K RMSNorm and Qwen2.5's Q/K/V projection bias are both
//! read from `Config` (`qk_norm`, `qkv_bias`); this module branches on the
//! flags, never on which model is loaded.

use crate::config::Config;
use crate::container::{Container, Error};
use crate::tensor::Matrix;

struct Layer {
    input_norm: Vec<f32>,
    q_proj: Matrix,
    q_bias: Option<Vec<f32>>,
    k_proj: Matrix,
    k_bias: Option<Vec<f32>>,
    v_proj: Matrix,
    v_bias: Option<Vec<f32>>,
    o_proj: Matrix,
    q_norm: Option<Vec<f32>>,
    k_norm: Option<Vec<f32>>,
    post_attn_norm: Vec<f32>,
    gate_proj: Matrix,
    up_proj: Matrix,
    down_proj: Matrix,
}

pub struct Model {
    pub config: Config,
    embed_tokens: Matrix,
    layers: Vec<Layer>,
    final_norm: Vec<f32>,
    /// Row-parallel matmul thread count (`parallel` feature only; ignored
    /// otherwise). `matmul_threaded` assigns whole, disjoint output rows to
    /// threads with each row's own accumulation in fixed ascending order, so
    /// this can never change a single bit of the result — only wall time.
    #[cfg_attr(not(feature = "parallel"), allow(dead_code))]
    threads: usize,
}

/// Per-layer, per-position key/value vectors (`n_kv_heads * head_dim` long
/// each), append-only. Kept as one `Vec` per cached position rather than one
/// flat buffer per layer so that `truncate` — reusing a prefix's cache
/// across several candidate continuations — is a plain `Vec::truncate`, not
/// an offset computation that has to be re-derived correctly at every call
/// site.
#[derive(Clone)]
pub struct KvCache {
    keys: Vec<Vec<Vec<f32>>>,
    values: Vec<Vec<Vec<f32>>>,
}

impl KvCache {
    pub fn new(n_layers: usize) -> KvCache {
        KvCache { keys: vec![Vec::new(); n_layers], values: vec![Vec::new(); n_layers] }
    }

    pub fn len(&self) -> usize {
        self.keys.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn truncate(&mut self, len: usize) {
        for k in &mut self.keys {
            k.truncate(len);
        }
        for v in &mut self.values {
            v.truncate(len);
        }
    }
}

fn rmsnorm(x: &[f32], weight: &[f32], eps: f32) -> Vec<f32> {
    let ss: f32 = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let inv = 1.0 / (ss + eps).sqrt();
    x.iter().zip(weight).map(|(v, w)| v * inv * w).collect()
}

fn silu(x: f32) -> f32 {
    x / (1.0 + (-x).exp())
}

/// Rotates `vec` (one attention head's `head_dim`-long slice) in place by
/// the Llama/Qwen "rotate half" convention: pairs `(i, i + head_dim/2)`
/// rotated by `pos * theta^(-2i/head_dim)`.
fn apply_rope(vec: &mut [f32], pos: usize, theta: f64) {
    let d = vec.len();
    let half = d / 2;
    for i in 0..half {
        let freq = theta.powf(-2.0 * (i as f64) / (d as f64));
        let angle = pos as f64 * freq;
        let (sin, cos) = angle.sin_cos();
        let (sin, cos) = (sin as f32, cos as f32);
        let x1 = vec[i];
        let x2 = vec[i + half];
        vec[i] = x1 * cos - x2 * sin;
        vec[i + half] = x2 * cos + x1 * sin;
    }
}

fn layer_name(l: usize, suffix: &str) -> String {
    format!("model.layers.{l}.{suffix}")
}

fn require<'a, 'c>(c: &'c Container<'a>, name: &str) -> Result<&'c crate::container::RawTable<'a>, Error> {
    c.table(name).ok_or_else(|| Error::BadTable { name: name.into(), why: "missing table" })
}

fn load_matrix(c: &Container, name: &str) -> Result<Matrix, Error> {
    Matrix::from_table(require(c, name)?)
}

fn load_vec(c: &Container, name: &str) -> Result<Vec<f32>, Error> {
    require(c, name)?.f32s()
}

impl Model {
    pub fn from_container(c: &Container, config: Config) -> Result<Model, Error> {
        let embed_tokens = load_matrix(c, "model.embed_tokens.weight")?;
        let final_norm = load_vec(c, "model.norm.weight")?;

        let mut layers = Vec::with_capacity(config.n_layers);
        for l in 0..config.n_layers {
            let q_proj = load_matrix(c, &layer_name(l, "self_attn.q_proj.weight"))?;
            let k_proj = load_matrix(c, &layer_name(l, "self_attn.k_proj.weight"))?;
            let v_proj = load_matrix(c, &layer_name(l, "self_attn.v_proj.weight"))?;
            let o_proj = load_matrix(c, &layer_name(l, "self_attn.o_proj.weight"))?;

            let (q_bias, k_bias, v_bias) = if config.qkv_bias {
                (
                    Some(load_vec(c, &layer_name(l, "self_attn.q_proj.bias"))?),
                    Some(load_vec(c, &layer_name(l, "self_attn.k_proj.bias"))?),
                    Some(load_vec(c, &layer_name(l, "self_attn.v_proj.bias"))?),
                )
            } else {
                (None, None, None)
            };

            let (q_norm, k_norm) = if config.qk_norm {
                (
                    Some(load_vec(c, &layer_name(l, "self_attn.q_norm.weight"))?),
                    Some(load_vec(c, &layer_name(l, "self_attn.k_norm.weight"))?),
                )
            } else {
                (None, None)
            };

            layers.push(Layer {
                input_norm: load_vec(c, &layer_name(l, "input_layernorm.weight"))?,
                q_proj,
                q_bias,
                k_proj,
                k_bias,
                v_proj,
                v_bias,
                o_proj,
                q_norm,
                k_norm,
                post_attn_norm: load_vec(c, &layer_name(l, "post_attention_layernorm.weight"))?,
                gate_proj: load_matrix(c, &layer_name(l, "mlp.gate_proj.weight"))?,
                up_proj: load_matrix(c, &layer_name(l, "mlp.up_proj.weight"))?,
                down_proj: load_matrix(c, &layer_name(l, "mlp.down_proj.weight"))?,
            });
        }

        Ok(Model { config, embed_tokens, layers, final_norm, threads: 1 })
    }

    /// Sets the row-parallel matmul thread count (`parallel` feature only;
    /// a no-op build-time choice otherwise). `n` is clamped to at least 1.
    pub fn with_threads(mut self, n: usize) -> Model {
        self.threads = n.max(1);
        self
    }

    fn linear(&self, m: &Matrix, x: &[f32], y: &mut [f32]) {
        #[cfg(feature = "parallel")]
        {
            if self.threads > 1 {
                crate::tensor::matmul_threaded(m, x, y, self.threads);
                return;
            }
        }
        m.matmul_into(x, y);
    }

    fn lm_head(&self, hidden: &[f32]) -> Vec<f32> {
        let mut y = vec![0.0f32; self.embed_tokens.out_dim()];
        self.linear(&self.embed_tokens, hidden, &mut y);
        y
    }

    fn forward_token(&self, token_id: u32, pos: usize, cache: &mut KvCache) -> Vec<f32> {
        let cfg = &self.config;
        let mut hidden = self.embed_tokens.row(token_id as usize);

        for (l, layer) in self.layers.iter().enumerate() {
            let normed = rmsnorm(&hidden, &layer.input_norm, cfg.rms_eps);

            let mut q = vec![0.0f32; cfg.q_dim()];
            self.linear(&layer.q_proj, &normed, &mut q);
            let mut k = vec![0.0f32; cfg.kv_dim()];
            self.linear(&layer.k_proj, &normed, &mut k);
            let mut v = vec![0.0f32; cfg.kv_dim()];
            self.linear(&layer.v_proj, &normed, &mut v);
            if let Some(b) = &layer.q_bias {
                for (x, bb) in q.iter_mut().zip(b) {
                    *x += bb;
                }
            }
            if let Some(b) = &layer.k_bias {
                for (x, bb) in k.iter_mut().zip(b) {
                    *x += bb;
                }
            }
            if let Some(b) = &layer.v_bias {
                for (x, bb) in v.iter_mut().zip(b) {
                    *x += bb;
                }
            }

            for h in 0..cfg.n_heads {
                let slice = &mut q[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                if let Some(w) = &layer.q_norm {
                    let normed_head = rmsnorm(slice, w, cfg.rms_eps);
                    slice.copy_from_slice(&normed_head);
                }
                apply_rope(slice, pos, cfg.rope_theta);
            }
            for h in 0..cfg.n_kv_heads {
                let slice = &mut k[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                if let Some(w) = &layer.k_norm {
                    let normed_head = rmsnorm(slice, w, cfg.rms_eps);
                    slice.copy_from_slice(&normed_head);
                }
                apply_rope(slice, pos, cfg.rope_theta);
            }

            cache.keys[l].push(k);
            cache.values[l].push(v);

            let repeat = cfg.n_heads / cfg.n_kv_heads;
            let scale = 1.0 / (cfg.head_dim as f32).sqrt();
            let mut attn_out = vec![0.0f32; cfg.q_dim()];
            for h in 0..cfg.n_heads {
                let kv_head = h / repeat;
                let q_head = &q[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                let n_ctx = cache.keys[l].len();
                let mut scores = Vec::with_capacity(n_ctx);
                for j in 0..n_ctx {
                    let k_vec = &cache.keys[l][j][kv_head * cfg.head_dim..(kv_head + 1) * cfg.head_dim];
                    let dot: f32 = q_head.iter().zip(k_vec).map(|(a, b)| a * b).sum();
                    scores.push(dot * scale);
                }
                let max = scores.iter().fold(f32::MIN, |m, &s| m.max(s));
                let mut weights: Vec<f32> = scores.iter().map(|&s| (s - max).exp()).collect();
                let sum: f32 = weights.iter().sum();
                for w in &mut weights {
                    *w /= sum;
                }
                let out = &mut attn_out[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                for j in 0..n_ctx {
                    let v_vec = &cache.values[l][j][kv_head * cfg.head_dim..(kv_head + 1) * cfg.head_dim];
                    for (o, vv) in out.iter_mut().zip(v_vec) {
                        *o += weights[j] * vv;
                    }
                }
            }

            let mut attn_proj = vec![0.0f32; cfg.hidden_size];
            self.linear(&layer.o_proj, &attn_out, &mut attn_proj);
            for (h, a) in hidden.iter_mut().zip(&attn_proj) {
                *h += a;
            }

            let normed2 = rmsnorm(&hidden, &layer.post_attn_norm, cfg.rms_eps);
            let mut gate = vec![0.0f32; cfg.intermediate_size];
            self.linear(&layer.gate_proj, &normed2, &mut gate);
            let mut up = vec![0.0f32; cfg.intermediate_size];
            self.linear(&layer.up_proj, &normed2, &mut up);
            let swiglu: Vec<f32> = gate.iter().zip(&up).map(|(&g, &u)| silu(g) * u).collect();
            let mut down = vec![0.0f32; cfg.hidden_size];
            self.linear(&layer.down_proj, &swiglu, &mut down);
            for (h, d) in hidden.iter_mut().zip(&down) {
                *h += d;
            }
        }

        rmsnorm(&hidden, &self.final_norm, cfg.rms_eps)
    }

    /// Logits at every position of `tokens` (position `i` predicts token
    /// `i + 1`), computed with a fresh cache.
    pub fn logits_all(&self, tokens: &[u32]) -> Vec<Vec<f32>> {
        let mut cache = KvCache::new(self.layers.len());
        tokens
            .iter()
            .enumerate()
            .map(|(pos, &t)| {
                let hidden = self.forward_token(t, pos, &mut cache);
                self.lm_head(&hidden)
            })
            .collect()
    }

    /// Logits after the last token of `tokens`.
    pub fn logits(&self, tokens: &[u32]) -> Vec<f32> {
        self.logits_all(tokens).pop().unwrap_or_default()
    }

    /// Processes `prefix` once, returning its KV cache and the logits that
    /// predict the token after it. Reuse this across every candidate scored
    /// against the same prefix instead of calling `score`, which reprocesses
    /// the prefix on every call.
    pub fn prefill(&self, prefix: &[u32]) -> (KvCache, Vec<f32>) {
        assert!(!prefix.is_empty(), "prefill() needs a non-empty prefix to produce next-token logits");
        let mut cache = KvCache::new(self.layers.len());
        let mut logits = Vec::new();
        for (pos, &t) in prefix.iter().enumerate() {
            let hidden = self.forward_token(t, pos, &mut cache);
            logits = self.lm_head(&hidden);
        }
        (cache, logits)
    }

    /// `sum_i log p(candidate[i] | prefix, candidate[..i])`, continuing from
    /// a prefix already processed by `prefill`. Clones the cache so the same
    /// prefill can be reused for further candidates.
    pub fn score_continue(&self, cache: &KvCache, next_logits: &[f32], prefix_len: usize, candidate: &[u32]) -> f64 {
        let mut cache = cache.clone();
        let mut next_logits = next_logits.to_vec();
        let mut total = 0.0f64;
        for (i, &t) in candidate.iter().enumerate() {
            total += log_softmax_at(&next_logits, t as usize);
            let hidden = self.forward_token(t, prefix_len + i, &mut cache);
            next_logits = self.lm_head(&hidden);
        }
        total
    }

    /// `sum_i log p(candidate[i] | prefix, candidate[..i])`. `prefix` must be
    /// non-empty: this format defines no BOS convention to score a candidate
    /// with no context at all. Scoring several candidates against the same
    /// prefix should use `prefill` + `score_continue` instead, so the prefix
    /// is not reprocessed per candidate.
    pub fn score(&self, prefix: &[u32], candidate: &[u32]) -> f64 {
        debug_assert!(!prefix.is_empty(), "score() needs a non-empty prefix; see prefill()/score_continue()");
        let (cache, logits) = self.prefill(prefix);
        self.score_continue(&cache, &logits, prefix.len(), candidate)
    }
}

fn log_softmax_at(logits: &[f32], idx: usize) -> f64 {
    let max = logits.iter().fold(f32::MIN, |m, &v| m.max(v));
    let sum: f64 = logits.iter().map(|&v| f64::from(v - max).exp()).sum();
    f64::from(logits[idx] - max) - sum.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_config() -> Config {
        Config {
            n_layers: 1,
            hidden_size: 8,
            n_heads: 2,
            n_kv_heads: 1,
            head_dim: 4,
            intermediate_size: 6,
            vocab_size: 5,
            rope_theta: 10000.0,
            rms_eps: 1e-6,
            qk_norm: false,
            qkv_bias: false,
            tie_word_embeddings: true,
        }
    }

    fn filled_matrix(out: usize, inp: usize, seed: u64) -> Matrix {
        let mut state = seed.wrapping_add(1);
        let mut next = || {
            // xorshift64, deterministic and dependency-free.
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state % 2000) as f32 - 1000.0) / 1000.0
        };
        Matrix::F32 { out, inp, data: (0..out * inp).map(|_| next()).collect() }
    }

    fn tiny_model() -> Model {
        let cfg = tiny_config();
        let layer = Layer {
            input_norm: vec![1.0; cfg.hidden_size],
            q_proj: filled_matrix(cfg.q_dim(), cfg.hidden_size, 1),
            q_bias: None,
            k_proj: filled_matrix(cfg.kv_dim(), cfg.hidden_size, 2),
            k_bias: None,
            v_proj: filled_matrix(cfg.kv_dim(), cfg.hidden_size, 3),
            v_bias: None,
            o_proj: filled_matrix(cfg.hidden_size, cfg.q_dim(), 4),
            q_norm: None,
            k_norm: None,
            post_attn_norm: vec![1.0; cfg.hidden_size],
            gate_proj: filled_matrix(cfg.intermediate_size, cfg.hidden_size, 5),
            up_proj: filled_matrix(cfg.intermediate_size, cfg.hidden_size, 6),
            down_proj: filled_matrix(cfg.hidden_size, cfg.intermediate_size, 7),
        };
        Model {
            embed_tokens: filled_matrix(cfg.vocab_size, cfg.hidden_size, 8),
            layers: vec![layer],
            final_norm: vec![1.0; cfg.hidden_size],
            config: cfg,
            threads: 1,
        }
    }

    #[test]
    fn a_tiny_random_config_forward_pass_has_the_right_shape() {
        let m = tiny_model();
        let logits = m.logits(&[0, 1, 2]);
        assert_eq!(logits.len(), m.config.vocab_size);
        assert!(logits.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn scoring_is_deterministic_across_repeated_calls() {
        let m = tiny_model();
        let a = m.score(&[0, 1], &[2, 3]);
        let b = m.score(&[0, 1], &[2, 3]);
        assert_eq!(a, b);
    }

    #[test]
    fn prefill_reuse_matches_scoring_from_scratch() {
        let m = tiny_model();
        let direct = m.score(&[0, 1], &[2, 3]);
        let (cache, logits) = m.prefill(&[0, 1]);
        let reused = m.score_continue(&cache, &logits, 2, &[2, 3]);
        assert_eq!(direct, reused);
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn threaded_matmul_is_byte_identical_to_single_threaded() {
        let single = tiny_model().with_threads(1);
        let threaded = tiny_model().with_threads(8);
        let a = single.score(&[0, 1], &[2, 3, 4]);
        let b = threaded.score(&[0, 1], &[2, 3, 4]);
        assert_eq!(a.to_bits(), b.to_bits());
    }
}
