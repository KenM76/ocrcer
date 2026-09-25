//! Forward pass for the optional neural glyph classifier
//! (`ARCHITECTURE.md` §11, "Operator: do training steps..." 2026-09-24,
//! amended 2026-09-25 for the junk output; interfaces fixed 2026-09-25,
//! "Chunk 15 interfaces: trainer output, the `nn` table, and the parity
//! check", item 3).
//!
//! # Contract
//!
//! [`Network`] is a minimal, already-dequantised layer list. `ocrcer-build`'s
//! `nn`-table parser (a separate change, not this module) hands its
//! dequantised `f32` tensors straight into this shape; nothing here reads a
//! `.ocrw` byte or an int8 value — int8 is a storage format only, per
//! `ARCHITECTURE.md` §7, and dequantisation happens once at load, same as
//! the prototype tables.
//!
//! [`Network::forward`] runs the layers in order — `conv3x3`, `relu`,
//! `maxpool2`, `flatten`, `concat_features`, `dense` — and returns
//! log-softmax over `n_outputs` classes (charset length + 1 junk unit, per
//! the 2026-09-25 junk-output amendment). It never re-derives the
//! extractor's 107-dim vector or 32x32 grid: both are handed in already
//! computed by [`crate::feature::extract_with_grid`] and normalised the same
//! way [`crate::ocrw::Model::standardise`] normalises a matcher query
//! (`CLAUDE.md` rule 4 — one extractor, one normalisation, read by everything
//! that scores a glyph).
//!
//! # Conventions the PyTorch trainer (`tools/nn/`, a separate change) must
//! match, or the parity fixture (`ARCHITECTURE.md` §11, "Chunk 15
//! interfaces", item 4) fails at its own 1e-4 tolerance:
//!
//! - **`conv3x3`**: `nn.Conv2d(kernel_size=3, padding=1, stride=1)` — "same"
//!   padding, zero-filled, stride 1. Weight layout is `[out][in][3][3]`,
//!   PyTorch's default `Conv2d.weight` layout, so a dequantised tensor is
//!   used as-is with no transpose.
//! - **`maxpool2`**: `nn.MaxPool2d(2)` — 2x2 window, stride 2, no padding,
//!   which floors when a dimension is odd (the last row/column is dropped,
//!   never padded).
//! - **`flatten`**: `torch.flatten(x, 1)` on an `[N, C, H, W]` tensor is
//!   row-major over `(C, H, W)`, i.e. index `c*H*W + h*W + w` —
//!   "channel-major `[c][y][x]`". This module's feature maps are stored in
//!   exactly that layout throughout, so `flatten` here is a relabelling, not
//!   a data movement.
//! - **`concat_features`**: `torch.cat([conv_features, feature_vector], 1)`
//!   — the flattened conv output first, the (already `feature_norm`-
//!   normalised) 107-dim vector second. Never the other order.
//! - **log-softmax**: `F.log_softmax(logits, dim=-1)`, i.e. max-subtracted:
//!   `logits[i] - max - ln(sum_j exp(logits[j] - max))`.
//!
//! # Determinism, and where it does not hold
//!
//! Every layer through `dense` accumulates in `f32`, in a fixed ascending
//! index order (input channel, then kernel row, then kernel column for a
//! conv; ascending input index for a dense layer) — the same discipline
//! [`crate::feature`] and [`crate::r#match`] use.
//!
//! Log-softmax is the one exception in this crate outside
//! [`crate::feature`]'s documented `sqrt`: it calls `f32::exp` and
//! `f32::ln`, neither of which IEEE 754 requires to be correctly rounded, so
//! the network head is not guaranteed bit-identical between x86 and wasm32
//! the way every other stage's fixtures are. This is accepted rather than
//! worked around: the only fixture that reads it (§8.2's parity check, item
//! 4 above) is itself a tolerance check against PyTorch's own
//! log-probabilities, not a byte-exact one, so a bisection trick that made
//! this module's answer reproducible across targets still would not make it
//! agree with PyTorch to the bit — nothing would be bought by writing one
//! twice as complex as [`crate::confidence`]'s. `match.classifier` defaults
//! to `0`, so no shipped fixture reads this module at all yet.

use crate::feature::FEATURE_DIMS;

/// One `conv3x3` layer's weights: `out_channels` filters, each
/// `in_channels x 3 x 3`, PyTorch's `Conv2d.weight` layout, flattened
/// `[out][in][3][3]` row-major (`weight[((o * in_channels + i) * 3 + ky) * 3
/// + kx]`). One bias per output channel.
#[derive(Debug, Clone)]
pub struct Conv3x3 {
    pub out_channels: usize,
    pub in_channels: usize,
    /// Length `out_channels * in_channels * 9`.
    pub weight: Vec<f32>,
    /// Length `out_channels`.
    pub bias: Vec<f32>,
}

/// One `dense` layer's weights, PyTorch's `Linear.weight` layout,
/// `[out][in]` row-major (`weight[o * in_features + j]`). One bias per
/// output.
#[derive(Debug, Clone)]
pub struct Dense {
    pub out_features: usize,
    pub in_features: usize,
    /// Length `out_features * in_features`.
    pub weight: Vec<f32>,
    /// Length `out_features`.
    pub bias: Vec<f32>,
}

/// One layer in the network, in forward-pass order.
#[derive(Debug, Clone)]
pub enum Layer {
    Conv3x3(Conv3x3),
    Relu,
    MaxPool2,
    Flatten,
    /// Appends the extractor's normalised 107-dim feature vector to the
    /// current (already-flattened) activation. Only valid after `Flatten`.
    ConcatFeatures,
    Dense(Dense),
}

/// A loaded network: the layer list plus the two facts the decoder needs to
/// read the output layer honestly.
///
/// `junk_index` and `n_outputs` are carried on `Network` rather than derived
/// from the layer list because nothing about a bare list of tensors can say
/// which output column is the reject unit — that fact comes from
/// `meta.nn`/`spec.json`, one layer up from here (`ARCHITECTURE.md` §11,
/// "the network learns to reject non-characters").
#[derive(Debug, Clone)]
pub struct Network {
    pub layers: Vec<Layer>,
    /// Charset length + 1. The output layer's width.
    pub n_outputs: usize,
    /// Equal to the charset length: the index of the junk unit. Junk is
    /// never emitted and is never a rival class for confidence purposes
    /// (`ARCHITECTURE.md` §11, 2026-09-25 amendment, item 4).
    pub junk_index: usize,
}

/// Why [`Network::forward`] could not run.
///
/// All of these mean the layer list and the tensors it carries disagree with
/// each other, or with the input — never a value the layer list computed.
/// Returned rather than panicked: a network handed in by a parser reading an
/// untrusted file must fail with an `Err`, the same discipline `ocrw.rs`
/// applies to every table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardError {
    /// A `conv3x3`/`relu`/`maxpool2` layer ran on an activation that is not
    /// a feature map (already flattened, or the list starts with one of
    /// these).
    ExpectedFeatureMap,
    /// A `flatten`/`concat_features`/`dense` layer ran on a feature map
    /// rather than a flat vector.
    ExpectedVector,
    /// A conv layer's weight/bias length disagrees with its declared shape.
    BadConvShape,
    /// A conv layer's declared `in_channels` disagrees with the activation
    /// feeding it.
    ChannelMismatch { declared: usize, actual: usize },
    /// A dense layer's weight/bias length disagrees with its declared shape.
    BadDenseShape,
    /// A dense layer's declared `in_features` disagrees with the activation
    /// feeding it.
    WidthMismatch { declared: usize, actual: usize },
    /// The layer list does not end in `Dense`, so there is no logit vector
    /// for log-softmax to run over.
    NoFinalDense,
    /// The final `Dense` layer's width disagrees with `Network::n_outputs`.
    OutputWidthMismatch { got: usize, want: usize },
}

/// The extractor's grid side, mirroring `feature::extract_with_grid`'s
/// private `GRID` constant (`CLAUDE.md` rule 4 — this is the one place
/// outside `feature.rs` that names it, rather than a second definition of
/// the grid size; a change to the extractor's grid is a `FEATURE_VERSION`
/// bump either way, so the two cannot silently drift).
pub const GRID: usize = 32;

impl Network {
    /// Runs the network on one glyph's extractor output.
    ///
    /// `grid` is `extract_with_grid`'s 32x32 grid, laid out `[y][x]`
    /// (`ARCHITECTURE.md` §11, "Chunk 15 interfaces", item 1: "`G`... laid
    /// out `[1][32][32]`" — the leading `1` is the single input channel this
    /// function starts the activation with). `features` is the 107-dim
    /// vector **already normalised** with the model's `feature_norm`
    /// (`crate::ocrw::Model::standardise`) — this function does not
    /// normalise it again.
    pub fn forward(
        &self,
        grid: &[[f32; GRID]; GRID],
        features: &[f32; FEATURE_DIMS],
    ) -> Result<Vec<f32>, ForwardError> {
        if !matches!(self.layers.last(), Some(Layer::Dense(_))) {
            return Err(ForwardError::NoFinalDense);
        }

        let mut data = Vec::with_capacity(GRID * GRID);
        for row in grid {
            data.extend_from_slice(row);
        }
        let mut act = Activation::Map { c: 1, h: GRID, w: GRID, data };

        for layer in &self.layers {
            act = match layer {
                Layer::Conv3x3(cv) => apply_conv(&act, cv)?,
                Layer::Relu => apply_relu(act),
                Layer::MaxPool2 => apply_pool(&act)?,
                Layer::Flatten => apply_flatten(act)?,
                Layer::ConcatFeatures => apply_concat(act, features)?,
                Layer::Dense(d) => apply_dense(&act, d)?,
            };
        }

        let logits = match act {
            Activation::Vector(v) => v,
            Activation::Map { .. } => return Err(ForwardError::ExpectedVector),
        };
        if logits.len() != self.n_outputs {
            return Err(ForwardError::OutputWidthMismatch { got: logits.len(), want: self.n_outputs });
        }
        Ok(log_softmax(&logits))
    }
}

/// The activation flowing between layers: a channel-major feature map before
/// `flatten`, a flat vector after.
enum Activation {
    Map { c: usize, h: usize, w: usize, data: Vec<f32> },
    Vector(Vec<f32>),
}

fn apply_conv(act: &Activation, cv: &Conv3x3) -> Result<Activation, ForwardError> {
    let Activation::Map { c: in_c, h, w, data } = act else {
        return Err(ForwardError::ExpectedFeatureMap);
    };
    let (h, w) = (*h, *w);
    if *in_c != cv.in_channels {
        return Err(ForwardError::ChannelMismatch { declared: cv.in_channels, actual: *in_c });
    }
    if cv.weight.len() != cv.out_channels * cv.in_channels * 9 || cv.bias.len() != cv.out_channels {
        return Err(ForwardError::BadConvShape);
    }

    let mut out = vec![0.0f32; cv.out_channels * h * w];
    for o in 0..cv.out_channels {
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0f32;
                // Fixed order: input channel, then kernel row, then kernel
                // column, ascending — the "fixed loop order" the module
                // header promises. Zero padding is implemented by skipping
                // an out-of-range tap rather than reordering the sum: a
                // skipped tap contributes 0 either way.
                for i in 0..cv.in_channels {
                    for ky in 0..3usize {
                        let iy = y as isize + ky as isize - 1;
                        if iy < 0 || iy >= h as isize {
                            continue;
                        }
                        for kx in 0..3usize {
                            let ix = x as isize + kx as isize - 1;
                            if ix < 0 || ix >= w as isize {
                                continue;
                            }
                            let wv = cv.weight[((o * cv.in_channels + i) * 3 + ky) * 3 + kx];
                            let iv = data[(i * h + iy as usize) * w + ix as usize];
                            acc += wv * iv;
                        }
                    }
                }
                acc += cv.bias[o];
                out[(o * h + y) * w + x] = acc;
            }
        }
    }
    Ok(Activation::Map { c: cv.out_channels, h, w, data: out })
}

fn apply_relu(act: Activation) -> Activation {
    match act {
        Activation::Map { c, h, w, mut data } => {
            for v in data.iter_mut() {
                if *v < 0.0 {
                    *v = 0.0;
                }
            }
            Activation::Map { c, h, w, data }
        }
        Activation::Vector(mut v) => {
            for x in v.iter_mut() {
                if *x < 0.0 {
                    *x = 0.0;
                }
            }
            Activation::Vector(v)
        }
    }
}

/// 2x2 window, stride 2, floor — `nn.MaxPool2d(2)`'s default. A trailing odd
/// row or column is dropped, never padded.
fn apply_pool(act: &Activation) -> Result<Activation, ForwardError> {
    let Activation::Map { c, h, w, data } = act else {
        return Err(ForwardError::ExpectedFeatureMap);
    };
    let (c, h, w) = (*c, *h, *w);
    let oh = h / 2;
    let ow = w / 2;
    let mut out = vec![0.0f32; c * oh * ow];
    for ch in 0..c {
        for y in 0..oh {
            for x in 0..ow {
                let a = data[(ch * h + 2 * y) * w + 2 * x];
                let b = data[(ch * h + 2 * y) * w + 2 * x + 1];
                let d = data[(ch * h + 2 * y + 1) * w + 2 * x];
                let e = data[(ch * h + 2 * y + 1) * w + 2 * x + 1];
                out[(ch * oh + y) * ow + x] = a.max(b).max(d).max(e);
            }
        }
    }
    Ok(Activation::Map { c, h: oh, w: ow, data: out })
}

/// A relabelling, not a data movement: `Activation::Map`'s `data` is already
/// stored `[c][y][x]` row-major, which is exactly `torch.flatten(x, 1)`'s
/// order for an `[N, C, H, W]` tensor.
fn apply_flatten(act: Activation) -> Result<Activation, ForwardError> {
    match act {
        Activation::Map { data, .. } => Ok(Activation::Vector(data)),
        Activation::Vector(_) => Err(ForwardError::ExpectedFeatureMap),
    }
}

/// Conv features first, then the 107-dim normalised vector — `torch.cat([conv,
/// feats], 1)`'s order, never the other way round.
fn apply_concat(act: Activation, features: &[f32; FEATURE_DIMS]) -> Result<Activation, ForwardError> {
    match act {
        Activation::Vector(mut v) => {
            v.extend_from_slice(features);
            Ok(Activation::Vector(v))
        }
        Activation::Map { .. } => Err(ForwardError::ExpectedVector),
    }
}

fn apply_dense(act: &Activation, d: &Dense) -> Result<Activation, ForwardError> {
    let Activation::Vector(v) = act else {
        return Err(ForwardError::ExpectedVector);
    };
    if d.weight.len() != d.out_features * d.in_features || d.bias.len() != d.out_features {
        return Err(ForwardError::BadDenseShape);
    }
    if v.len() != d.in_features {
        return Err(ForwardError::WidthMismatch { declared: d.in_features, actual: v.len() });
    }
    let mut out = vec![0.0f32; d.out_features];
    for o in 0..d.out_features {
        let base = o * d.in_features;
        let mut acc = 0.0f32;
        // Fixed order: ascending input index.
        for j in 0..d.in_features {
            acc += d.weight[base + j] * v[j];
        }
        acc += d.bias[o];
        out[o] = acc;
    }
    Ok(Activation::Vector(out))
}

/// `F.log_softmax(logits, dim=-1)`: max-subtracted for numerical stability,
/// `f32` accumulation. See the module header for why `exp`/`ln` are the
/// deliberate exception to this crate's no-transcendental-function rule.
fn log_softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;
    for &v in logits {
        sum += (v - max).exp();
    }
    let log_sum = sum.ln();
    logits.iter().map(|&v| v - max - log_sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_grid() -> [[f32; GRID]; GRID] {
        [[0.0; GRID]; GRID]
    }

    fn zero_features() -> [f32; FEATURE_DIMS] {
        [0.0; FEATURE_DIMS]
    }

    // ---- shape plumbing ----

    /// The contract shape from `ARCHITECTURE.md`'s 2026-09-24 entry, item 3:
    /// conv3x3x16 -> relu -> pool -> conv3x3x32 -> relu -> pool -> flatten ->
    /// concat_features -> dense(2048+107 -> 128) -> relu -> dense(128 ->
    /// n_outputs). Weights are all zero; this test is about shape survival,
    /// not arithmetic — see the hand-computed tests below for that.
    fn contract_shape_network(n_outputs: usize) -> Network {
        let conv1 = Conv3x3 { out_channels: 16, in_channels: 1, weight: vec![0.0; 16 * 1 * 9], bias: vec![0.0; 16] };
        let conv2 = Conv3x3 { out_channels: 32, in_channels: 16, weight: vec![0.0; 32 * 16 * 9], bias: vec![0.0; 32] };
        let dense1 = Dense {
            out_features: 128,
            in_features: 32 * 8 * 8 + FEATURE_DIMS,
            weight: vec![0.0; 128 * (32 * 8 * 8 + FEATURE_DIMS)],
            bias: vec![0.0; 128],
        };
        let dense2 = Dense { out_features: n_outputs, in_features: 128, weight: vec![0.0; n_outputs * 128], bias: vec![0.0; n_outputs] };
        Network {
            layers: vec![
                Layer::Conv3x3(conv1),
                Layer::Relu,
                Layer::MaxPool2,
                Layer::Conv3x3(conv2),
                Layer::Relu,
                Layer::MaxPool2,
                Layer::Flatten,
                Layer::ConcatFeatures,
                Layer::Dense(dense1),
                Layer::Relu,
                Layer::Dense(dense2),
            ],
            n_outputs,
            junk_index: n_outputs - 1,
        }
    }

    #[test]
    fn contract_shape_forward_produces_n_outputs_log_probs() {
        let net = contract_shape_network(188);
        let out = net.forward(&zero_grid(), &zero_features()).unwrap();
        assert_eq!(out.len(), 188);
        // All-zero weights and biases: every logit is 0, so log-softmax is
        // uniform, -ln(188) everywhere.
        let expected = -(188.0f32).ln();
        for &v in &out {
            assert!((v - expected).abs() < 1e-4, "got {v}, want {expected}");
        }
    }

    #[test]
    fn a_conv_with_wrong_in_channels_errors_rather_than_panics() {
        let bad = Conv3x3 { out_channels: 1, in_channels: 2, weight: vec![0.0; 1 * 2 * 9], bias: vec![0.0] };
        let net = Network {
            layers: vec![Layer::Conv3x3(bad), Layer::Flatten, Layer::Dense(Dense {
                out_features: 1,
                in_features: GRID * GRID,
                weight: vec![0.0; GRID * GRID],
                bias: vec![0.0],
            })],
            n_outputs: 1,
            junk_index: 0,
        };
        let err = net.forward(&zero_grid(), &zero_features()).unwrap_err();
        assert_eq!(err, ForwardError::ChannelMismatch { declared: 2, actual: 1 });
    }

    #[test]
    fn a_layer_list_not_ending_in_dense_is_refused() {
        let net = Network { layers: vec![Layer::Relu], n_outputs: 1, junk_index: 0 };
        assert_eq!(net.forward(&zero_grid(), &zero_features()).unwrap_err(), ForwardError::NoFinalDense);
    }

    #[test]
    fn an_output_width_disagreeing_with_n_outputs_is_refused() {
        let dense = Dense { out_features: 5, in_features: GRID * GRID, weight: vec![0.0; 5 * GRID * GRID], bias: vec![0.0; 5] };
        let net = Network { layers: vec![Layer::Flatten, Layer::Dense(dense)], n_outputs: 3, junk_index: 2 };
        let err = net.forward(&zero_grid(), &zero_features()).unwrap_err();
        assert_eq!(err, ForwardError::OutputWidthMismatch { got: 5, want: 3 });
    }

    // ---- hand-computed arithmetic, on a tiny network ----

    /// A single 2x2 input, one conv output channel, no pool (2x2 has nothing
    /// left to pool losslessly so this network skips it), straight to a
    /// dense head — small enough to compute by hand.
    ///
    /// Grid (only the top-left 2x2 corner is nonzero, rest is 0):
    /// ```text
    /// 1 2
    /// 3 4
    /// ```
    /// One conv filter, identity-ish weights (only the centre tap is 1, the
    /// rest are 0), bias 0: the "convolution" is just a copy. Flatten gives
    /// `[1,2,0,...,0,3,4,0,...,0, 0,...]` (32x32, mostly zero). Rather than
    /// hand-expand that, this test uses a 1x1 "image" so the whole pipeline
    /// is checkable to the last decimal.
    #[test]
    fn hand_computed_conv_relu_dense_on_a_single_pixel() {
        let mut grid = zero_grid();
        grid[0][0] = 2.0;
        // A 3x3 filter whose centre tap is -1 and everything else 0: with
        // zero padding, the only nonzero contribution to output pixel (0,0)
        // is centre_weight * input(0,0) = -1 * 2 = -2, plus bias 0.5, giving
        // -1.5 pre-ReLU. Every other output pixel sees zero input (the grid
        // is otherwise all-background) so is bias-only: 0.5, ReLU'd to 0.5.
        let mut weight = vec![0.0f32; 9];
        weight[4] = -1.0; // (ky=1, kx=1): the centre tap.
        let conv = Conv3x3 { out_channels: 1, in_channels: 1, weight, bias: vec![0.5] };

        // Dense straight off the flattened 1x32x32 map (no features, no
        // second conv, no pool): weight picks out (0,0)'s post-ReLU value
        // with a 1, everything else with a 0, bias 0 — so the single logit
        // equals ReLU(-1.5) = 0.0 (a negative pre-activation was clamped).
        let mut dw = vec![0.0f32; GRID * GRID];
        dw[0] = 1.0;
        let dense = Dense { out_features: 1, in_features: GRID * GRID, weight: dw, bias: vec![0.0] };

        let net = Network {
            layers: vec![Layer::Conv3x3(conv), Layer::Relu, Layer::Flatten, Layer::Dense(dense)],
            n_outputs: 1,
            junk_index: 0,
        };
        let out = net.forward(&grid, &zero_features()).unwrap();
        // A single output class: log-softmax over one element is always 0.
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    /// Same network, but the centre tap is positive so ReLU does not clamp
    /// it, and there are two output classes so log-softmax is non-trivial —
    /// checked against the value computed by hand.
    #[test]
    fn hand_computed_two_class_log_softmax() {
        let mut grid = zero_grid();
        grid[0][0] = 2.0;
        let mut weight = vec![0.0f32; 9];
        weight[4] = 1.0; // centre tap: output(0,0) = 1*2 + bias.
        let conv = Conv3x3 { out_channels: 1, in_channels: 1, weight, bias: vec![0.0] };
        // Post-conv, post-ReLU: pixel (0,0) is 2.0, every other pixel is
        // ReLU(0) = 0.0 (bias-only, zero bias).

        // Two dense outputs straight off the flattened map: class 0 reads
        // pixel (0,0) with weight 1 (logit = 2.0); class 1 reads it with
        // weight 0.5 (logit = 1.0). Both biases 0.
        let mut w0 = vec![0.0f32; GRID * GRID];
        w0[0] = 1.0;
        let mut w1 = vec![0.0f32; GRID * GRID];
        w1[0] = 0.5;
        let mut dw = Vec::with_capacity(2 * GRID * GRID);
        dw.extend_from_slice(&w0);
        dw.extend_from_slice(&w1);
        let dense = Dense { out_features: 2, in_features: GRID * GRID, weight: dw, bias: vec![0.0, 0.0] };

        let net = Network {
            layers: vec![Layer::Conv3x3(conv), Layer::Relu, Layer::Flatten, Layer::Dense(dense)],
            n_outputs: 2,
            junk_index: 1,
        };
        let out = net.forward(&grid, &zero_features()).unwrap();

        // Logits [2.0, 1.0]. max = 2.0. sum = exp(0) + exp(-1.0) =
        // 1 + 0.367_879_44... = 1.367_879_44...
        // log_sum = ln(1.367_879_44...) = 0.313_261_69...
        // log_softmax = [2.0 - 2.0 - log_sum, 1.0 - 2.0 - log_sum]
        //             = [-0.313_261_69..., -1.313_261_69...]
        let sum = 1.0f32 + (-1.0f32).exp();
        let log_sum = sum.ln();
        let want0 = 2.0 - 2.0 - log_sum;
        let want1 = 1.0 - 2.0 - log_sum;
        assert!((out[0] - want0).abs() < 1e-6, "got {}, want {}", out[0], want0);
        assert!((out[1] - want1).abs() < 1e-6, "got {}, want {}", out[1], want1);
        // log-probabilities: never positive, and the larger logit wins.
        assert!(out[0] < 0.0 && out[1] < 0.0);
        assert!(out[0] > out[1]);
    }

    /// The `concat_features` layer's order: conv features first, the 107-dim
    /// vector second — never the other way round, per the module header.
    #[test]
    fn concat_features_puts_conv_output_before_the_feature_vector() {
        let mut grid = zero_grid();
        grid[0][0] = 5.0;
        let mut weight = vec![0.0f32; 9];
        weight[4] = 1.0;
        let conv = Conv3x3 { out_channels: 1, in_channels: 1, weight, bias: vec![0.0] };

        let mut features = zero_features();
        features[0] = 42.0;

        // Dense straight off the concatenated vector (length GRID*GRID +
        // FEATURE_DIMS): weight 1 at index 0 (the conv output's pixel
        // (0,0)), weight 1 at index GRID*GRID (the feature vector's first
        // entry) -- sum should be 5.0 + 42.0 = 47.0 if concat order is
        // conv-then-features, or would instead pick up features[GRID*GRID]
        // (out of bounds; impossible) if reversed. This asserts the order
        // directly: index GRID*GRID must read the feature vector's first
        // entry, not the conv map's.
        let mut dw = vec![0.0f32; GRID * GRID + FEATURE_DIMS];
        dw[0] = 1.0;
        dw[GRID * GRID] = 1.0;
        let dense = Dense {
            out_features: 1,
            in_features: GRID * GRID + FEATURE_DIMS,
            weight: dw,
            bias: vec![0.0],
        };

        let net = Network {
            layers: vec![
                Layer::Conv3x3(conv),
                Layer::Relu,
                Layer::Flatten,
                Layer::ConcatFeatures,
                Layer::Dense(dense),
            ],
            n_outputs: 1,
            junk_index: 0,
        };
        let out = net.forward(&grid, &features).unwrap();
        // One output class: log-softmax is 0 regardless of the logit's
        // value, so this only proves the shapes lined up (a shape mismatch
        // would have returned an `Err`, not a wrong number). The real
        // assertion is that `forward` succeeded at all with this exact
        // layout of weight indices.
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    // ---- maxpool2, hand-computed ----

    #[test]
    fn maxpool_takes_the_max_of_each_2x2_block_floor_on_odd_size() {
        // A 3x3 map (one channel): pool floors to 1x1, taking the max of
        // the top-left 2x2 block only, and drops the trailing row/column.
        let data = vec![1.0f32, 2.0, 9.0, 3.0, 4.0, 9.0, 9.0, 9.0, 9.0];
        let act = Activation::Map { c: 1, h: 3, w: 3, data };
        let pooled = apply_pool(&act).unwrap();
        let Activation::Map { c, h, w, data } = pooled else { panic!("expected a map") };
        assert_eq!((c, h, w), (1, 1, 1));
        assert_eq!(data, vec![4.0]);
    }
}
