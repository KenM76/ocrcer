//! The Qwen decoder configuration, read out of `.ocrl`'s `meta.config`.
//!
//! Two families share this struct, distinguished by two flags rather than by
//! two code paths (`ARCHITECTURE.md` section 11, 2026-09-24): Qwen3 sets
//! `qk_norm = true, qkv_bias = false`; Qwen2.5 sets the reverse. Both are
//! architecture facts, not choices the model file gets to override
//! independently of which weights it actually carries — the converter derives
//! them from `model_type` and writes them, and the runtime just reads what it
//! is told.

use crate::container::{Container, Error};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub n_layers: usize,
    pub hidden_size: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub intermediate_size: usize,
    pub vocab_size: usize,
    pub rope_theta: f64,
    pub rms_eps: f32,
    /// Qwen3: RMSNorm applied to Q and K per head before RoPE.
    pub qk_norm: bool,
    /// Qwen2.5: bias added after the Q/K/V projections.
    pub qkv_bias: bool,
    /// `lm_head` shares `embed_tokens`'s weight; both models set this.
    pub tie_word_embeddings: bool,
}

impl Config {
    pub fn from_container(c: &Container) -> Result<Config, Error> {
        let cfg = c
            .meta
            .get("config")
            .ok_or_else(|| Error::BadTable { name: "meta".into(), why: "missing config object" })?;
        let get_u32 = |key: &'static str| -> Result<usize, Error> {
            cfg.get(key)
                .and_then(|v| v.as_u32())
                .map(|v| v as usize)
                .ok_or(Error::BadTable { name: "meta.config".into(), why: key_missing(key) })
        };
        let get_f64 = |key: &'static str| -> Result<f64, Error> {
            cfg.get(key).and_then(|v| v.as_f64()).ok_or(Error::BadTable { name: "meta.config".into(), why: key_missing(key) })
        };
        let get_bool = |key: &'static str| -> Result<bool, Error> {
            cfg.get(key).and_then(|v| v.as_bool()).ok_or(Error::BadTable { name: "meta.config".into(), why: key_missing(key) })
        };
        Ok(Config {
            n_layers: get_u32("n_layers")?,
            hidden_size: get_u32("hidden_size")?,
            n_heads: get_u32("n_heads")?,
            n_kv_heads: get_u32("n_kv_heads")?,
            head_dim: get_u32("head_dim")?,
            intermediate_size: get_u32("intermediate_size")?,
            vocab_size: get_u32("vocab_size")?,
            rope_theta: get_f64("rope_theta")?,
            rms_eps: get_f64("rms_norm_eps")? as f32,
            qk_norm: get_bool("qk_norm")?,
            qkv_bias: get_bool("qkv_bias")?,
            tie_word_embeddings: get_bool("tie_word_embeddings")?,
        })
    }

    pub fn q_dim(&self) -> usize {
        self.n_heads * self.head_dim
    }

    pub fn kv_dim(&self) -> usize {
        self.n_kv_heads * self.head_dim
    }
}

fn key_missing(key: &'static str) -> &'static str {
    match key {
        "n_layers" => "missing config.n_layers",
        "hidden_size" => "missing config.hidden_size",
        "n_heads" => "missing config.n_heads",
        "n_kv_heads" => "missing config.n_kv_heads",
        "head_dim" => "missing config.head_dim",
        "intermediate_size" => "missing config.intermediate_size",
        "vocab_size" => "missing config.vocab_size",
        "rope_theta" => "missing config.rope_theta",
        "rms_norm_eps" => "missing config.rms_norm_eps",
        "qk_norm" => "missing config.qk_norm",
        "qkv_bias" => "missing config.qkv_bias",
        "tie_word_embeddings" => "missing config.tie_word_embeddings",
        _ => "missing config field",
    }
}
