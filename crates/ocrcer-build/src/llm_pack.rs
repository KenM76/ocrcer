//! Converts a Hugging Face Qwen model directory (`config.json`,
//! `tokenizer.json`, `model.safetensors`, `LICENSE`) into a single `.ocrl`
//! file (`ARCHITECTURE.md` section 11, 2026-09-24).
//!
//! Reads only what `ocrcer-llm`'s runtime needs, no new dependency: the
//! safetensors header and `tokenizer.json` both parse with
//! `ocrcer_core::json::Json`. The GPT-2 byte<->unicode table
//! (`gpt2_bytes::ByteMap`) is used here, once, to turn `tokenizer.json`'s
//! string-keyed vocabulary and merges into id-keyed tables — the one place
//! in this whole pipeline that ever needs it (`ocrcer-llm`'s runtime never
//! does, see `ocrcer_llm::tokenizer`'s module doc).

use crate::gpt2_bytes::ByteMap;
use crate::ocrl::{json_string, Table};
use crate::safetensors::SafeTensors;
use ocrcer_core::json::Json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quant {
    F32,
    Q8,
}

impl Quant {
    pub fn parse(s: &str) -> Result<Quant, String> {
        match s {
            "f32" => Ok(Quant::F32),
            "q8" => Ok(Quant::Q8),
            other => Err(format!("unknown quant {other:?} (expected f32 or q8)")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Quant::F32 => "f32",
            Quant::Q8 => "q8",
        }
    }
}

struct DecoderConfig {
    n_layers: u32,
    hidden_size: u32,
    n_heads: u32,
    n_kv_heads: u32,
    head_dim: u32,
    intermediate_size: u32,
    vocab_size: u32,
    rope_theta: f64,
    rms_eps: f64,
    qk_norm: bool,
    qkv_bias: bool,
    tie_word_embeddings: bool,
}

pub fn convert(hf_dir: &Path, out: &Path, quant: Quant, model_id: &str, revision: &str) -> Result<(), String> {
    let config = parse_json_file(&hf_dir.join("config.json"))?;
    let tokenizer_json = parse_json_file(&hf_dir.join("tokenizer.json"))?;
    let license = fs::read_to_string(hf_dir.join("LICENSE")).unwrap_or_default();
    let st_bytes = fs::read(hf_dir.join("model.safetensors")).map_err(|e| format!("model.safetensors: {e}"))?;
    let st = SafeTensors::parse(&st_bytes)?;

    let cfg = read_config(&config)?;
    if !cfg.tie_word_embeddings {
        return Err("untied word embeddings are not supported (both Qwen3-0.6B and Qwen2.5-0.5B-Instruct tie them)".into());
    }

    let byte_map = ByteMap::new();
    let tok = extract_tokenizer(&tokenizer_json, &byte_map)?;

    let mut tables = Vec::new();
    tables.push(Table::u32s("tok.byte_id", vec![256], &tok.byte_id));
    tables.push(Table::u32s("tok.merge_left", vec![tok.merge_left.len() as u32], &tok.merge_left));
    tables.push(Table::u32s("tok.merge_right", vec![tok.merge_right.len() as u32], &tok.merge_right));
    tables.push(Table::u32s("tok.merge_result", vec![tok.merge_result.len() as u32], &tok.merge_result));

    add_weight(&mut tables, &st, "model.embed_tokens.weight", quant)?;
    add_weight(&mut tables, &st, "model.norm.weight", Quant::F32)?;
    for l in 0..cfg.n_layers {
        add_weight(&mut tables, &st, &format!("model.layers.{l}.input_layernorm.weight"), Quant::F32)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.q_proj.weight"), quant)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.k_proj.weight"), quant)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.v_proj.weight"), quant)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.o_proj.weight"), quant)?;
        if cfg.qkv_bias {
            add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.q_proj.bias"), Quant::F32)?;
            add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.k_proj.bias"), Quant::F32)?;
            add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.v_proj.bias"), Quant::F32)?;
        }
        if cfg.qk_norm {
            add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.q_norm.weight"), Quant::F32)?;
            add_weight(&mut tables, &st, &format!("model.layers.{l}.self_attn.k_norm.weight"), Quant::F32)?;
        }
        add_weight(&mut tables, &st, &format!("model.layers.{l}.post_attention_layernorm.weight"), Quant::F32)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.mlp.gate_proj.weight"), quant)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.mlp.up_proj.weight"), quant)?;
        add_weight(&mut tables, &st, &format!("model.layers.{l}.mlp.down_proj.weight"), quant)?;
    }

    let meta = build_meta(model_id, revision, &license, quant, &cfg, &tok.specials);
    ocrl_write(out, &meta, &tables)
}

fn ocrl_write(out: &Path, meta: &str, tables: &[Table]) -> Result<(), String> {
    crate::ocrl::write(out, 1, 1, meta, tables).map_err(|e| format!("writing {}: {e}", out.display()))
}

fn parse_json_file(path: &Path) -> Result<Json, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn req_u32(j: &Json, key: &str) -> Result<u32, String> {
    j.get(key).and_then(Json::as_u32).ok_or_else(|| format!("config.json: missing or non-integer {key}"))
}

fn read_config(config: &Json) -> Result<DecoderConfig, String> {
    let model_type = config.get("model_type").and_then(Json::as_str).ok_or("config.json: missing model_type")?;
    let (qk_norm, qkv_bias) = match model_type {
        "qwen3" => (true, false),
        "qwen2" => (false, true),
        other => return Err(format!("config.json: unsupported model_type {other:?} (only qwen2 and qwen3 are implemented)")),
    };
    let hidden_size = req_u32(config, "hidden_size")?;
    let n_heads = req_u32(config, "num_attention_heads")?;
    let head_dim = config.get("head_dim").and_then(Json::as_u32).unwrap_or(hidden_size / n_heads);
    Ok(DecoderConfig {
        n_layers: req_u32(config, "num_hidden_layers")?,
        hidden_size,
        n_heads,
        n_kv_heads: req_u32(config, "num_key_value_heads")?,
        head_dim,
        intermediate_size: req_u32(config, "intermediate_size")?,
        vocab_size: req_u32(config, "vocab_size")?,
        rope_theta: config.get("rope_theta").and_then(Json::as_f64).ok_or("config.json: missing rope_theta")?,
        rms_eps: config.get("rms_norm_eps").and_then(Json::as_f64).ok_or("config.json: missing rms_norm_eps")?,
        qk_norm,
        qkv_bias,
        tie_word_embeddings: config.get("tie_word_embeddings").and_then(Json::as_bool).unwrap_or(false),
    })
}

struct TokenizerTables {
    byte_id: Vec<u32>,
    merge_left: Vec<u32>,
    merge_right: Vec<u32>,
    merge_result: Vec<u32>,
    specials: Vec<(String, u32)>,
}

fn extract_tokenizer(tok: &Json, byte_map: &ByteMap) -> Result<TokenizerTables, String> {
    let model = tok.get("model").ok_or("tokenizer.json: missing model")?;
    let vocab_fields = model.get("vocab").and_then(Json::as_object).ok_or("tokenizer.json: missing model.vocab")?;

    let mut id_of: HashMap<&str, u32> = HashMap::with_capacity(vocab_fields.len());
    for (k, v) in vocab_fields {
        let id = v.as_u32().ok_or_else(|| format!("tokenizer.json: vocab id for {k:?} is not an integer"))?;
        id_of.insert(k.as_str(), id);
    }

    let mut byte_id = vec![u32::MAX; 256];
    for (k, &id) in &id_of {
        if let Some(bytes) = byte_map.decode(k) {
            if bytes.len() == 1 {
                byte_id[bytes[0] as usize] = id;
            }
        }
    }
    for (b, id) in byte_id.iter().enumerate() {
        if *id == u32::MAX {
            return Err(format!("tokenizer.json: vocabulary has no single-byte entry for byte {b}"));
        }
    }

    let merges_json = model.get("merges").and_then(Json::as_array).ok_or("tokenizer.json: missing model.merges")?;
    let mut merge_left = Vec::with_capacity(merges_json.len());
    let mut merge_right = Vec::with_capacity(merges_json.len());
    let mut merge_result = Vec::with_capacity(merges_json.len());
    for m in merges_json {
        let (l_str, r_str) = merge_pair(m)?;
        let l_id = *id_of.get(l_str.as_str()).ok_or_else(|| format!("tokenizer.json: merge left {l_str:?} not in vocab"))?;
        let r_id = *id_of.get(r_str.as_str()).ok_or_else(|| format!("tokenizer.json: merge right {r_str:?} not in vocab"))?;
        let combined = format!("{l_str}{r_str}");
        let result_id =
            *id_of.get(combined.as_str()).ok_or_else(|| format!("tokenizer.json: merge result {combined:?} not in vocab"))?;
        merge_left.push(l_id);
        merge_right.push(r_id);
        merge_result.push(result_id);
    }

    let mut specials = Vec::new();
    if let Some(added) = tok.get("added_tokens").and_then(Json::as_array) {
        for a in added {
            let content = a.get("content").and_then(Json::as_str).ok_or("tokenizer.json: added_tokens entry missing content")?;
            let id = a.get("id").and_then(Json::as_u32).ok_or("tokenizer.json: added_tokens entry missing id")?;
            specials.push((content.to_string(), id));
        }
    }

    Ok(TokenizerTables { byte_id, merge_left, merge_right, merge_result, specials })
}

/// A `tokenizer.json` merge entry is either `"left right"` (older format,
/// space-joined) or `["left", "right"]` (current `tokenizers` format).
fn merge_pair(m: &Json) -> Result<(String, String), String> {
    match m {
        Json::Str(s) => {
            let mut parts = s.splitn(2, ' ');
            let l = parts.next().ok_or_else(|| format!("tokenizer.json: malformed merge {s:?}"))?;
            let r = parts.next().ok_or_else(|| format!("tokenizer.json: malformed merge {s:?}"))?;
            Ok((l.to_string(), r.to_string()))
        }
        Json::Arr(items) => {
            if items.len() != 2 {
                return Err("tokenizer.json: a merge array must have exactly two entries".into());
            }
            let l = items[0].as_str().ok_or("tokenizer.json: merge entry is not a string")?;
            let r = items[1].as_str().ok_or("tokenizer.json: merge entry is not a string")?;
            Ok((l.to_string(), r.to_string()))
        }
        _ => Err("tokenizer.json: merge entry is neither a string nor a two-element array".into()),
    }
}

fn add_weight(tables: &mut Vec<Table>, st: &SafeTensors, name: &str, quant: Quant) -> Result<(), String> {
    let (shape, values) = st.read_f32(name)?;
    let dims: Vec<u32> = shape.iter().map(|&d| d as u32).collect();
    let table = if quant == Quant::Q8 && shape.len() == 2 && shape[1] % 32 == 0 {
        Table::q8(name, dims, &values)
    } else {
        Table::f32s(name, dims, &values)
    };
    tables.push(table);
    Ok(())
}

fn build_meta(model_id: &str, revision: &str, license: &str, quant: Quant, cfg: &DecoderConfig, specials: &[(String, u32)]) -> String {
    let mut specials_json = String::from("[");
    for (i, (text, id)) in specials.iter().enumerate() {
        if i > 0 {
            specials_json.push(',');
        }
        specials_json.push_str(&format!("{{\"id\":{id},\"text\":{}}}", json_string(text)));
    }
    specials_json.push(']');

    format!(
        "{{\"model_id\":{},\"upstream_revision\":{},\"quant\":{},\"license\":{},\
         \"config\":{{\"n_layers\":{},\"hidden_size\":{},\"n_heads\":{},\"n_kv_heads\":{},\
         \"head_dim\":{},\"intermediate_size\":{},\"vocab_size\":{},\"rope_theta\":{},\
         \"rms_norm_eps\":{},\"qk_norm\":{},\"qkv_bias\":{},\"tie_word_embeddings\":{}}},\
         \"special_tokens\":{}}}",
        json_string(model_id),
        json_string(revision),
        json_string(quant.as_str()),
        json_string(license),
        cfg.n_layers,
        cfg.hidden_size,
        cfg.n_heads,
        cfg.n_kv_heads,
        cfg.head_dim,
        cfg.intermediate_size,
        cfg.vocab_size,
        cfg.rope_theta,
        cfg.rms_eps,
        cfg.qk_norm,
        cfg.qkv_bias,
        cfg.tie_word_embeddings,
        specials_json,
    )
}
