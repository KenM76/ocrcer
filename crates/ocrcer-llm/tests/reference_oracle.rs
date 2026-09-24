//! Verification against the transformers/torch reference oracle
//! (`tools/qwen_reference.py`) and Q8-vs-F32 self-consistency.
//!
//! Every test here needs real weights, which never live in the repo
//! (`ARCHITECTURE.md` section 11), so all of them are `#[ignore]`d and gated
//! by an environment variable naming a `.ocrl` file the operator packed with
//! `ocrcer-build llm-pack`. Run explicitly with weights present:
//!
//! ```text
//! OCRCER_LLM_QWEN3_F32_OCRL=D:/Dev/ExcludedPrivate/ocrcer/llm/qwen3-0.6b.f32.ocrl \
//! OCRCER_LLM_QWEN3_Q8_OCRL=D:/Dev/ExcludedPrivate/ocrcer/llm/qwen3-0.6b.q8.ocrl \
//! OCRCER_LLM_QWEN25_F32_OCRL=D:/Dev/ExcludedPrivate/ocrcer/llm/qwen2.5-0.5b.f32.ocrl \
//! OCRCER_LLM_QWEN25_Q8_OCRL=D:/Dev/ExcludedPrivate/ocrcer/llm/qwen2.5-0.5b.q8.ocrl \
//! cargo test -p ocrcer-llm --release -- --ignored --nocapture
//! ```
//!
//! Plain `cargo test` (no env vars) never touches these — they are absent
//! from the default run, as required.

use std::fs;
use std::path::PathBuf;

use ocrcer_llm::json::Json;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/llm")
}

fn ocrl_from_env(var: &str) -> Option<(ocrcer_llm::Model, ocrcer_llm::Tokenizer)> {
    let path = std::env::var(var).ok()?;
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{var}={path}: {e}"));
    Some(ocrcer_llm::load(&bytes).unwrap_or_else(|e| panic!("{var}={path}: {e:?}")))
}

fn read_logits_bin(path: &PathBuf, vocab_size: usize) -> Vec<f32> {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(bytes.len(), vocab_size * 4, "{}: wrong byte length for vocab_size {vocab_size}", path.display());
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn top1(v: &[f32]) -> usize {
    v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i).unwrap()
}

fn topk(v: &[f32], k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[b].total_cmp(&v[a]));
    idx.truncate(k);
    idx
}

/// KL(p || q) in nats, both distributions over the same support, built from
/// raw logits via a numerically stable softmax.
fn mean_kl(logits_p: &[f32], logits_q: &[f32]) -> f64 {
    let softmax = |v: &[f32]| -> Vec<f64> {
        let max = v.iter().fold(f32::MIN, |m, &x| m.max(x));
        let exps: Vec<f64> = v.iter().map(|&x| f64::from(x - max).exp()).collect();
        let sum: f64 = exps.iter().sum();
        exps.into_iter().map(|e| e / sum).collect()
    };
    let p = softmax(logits_p);
    let q = softmax(logits_q);
    p.iter().zip(&q).filter(|(&pi, _)| pi > 0.0).map(|(&pi, &qi)| pi * (pi / qi.max(1e-30)).ln()).sum()
}

fn check_tokenizer_fixture(tok: &ocrcer_llm::Tokenizer, model_prefix: &str) {
    let text = fs::read_to_string(fixtures_dir().join(format!("{model_prefix}_tokens.json"))).unwrap();
    let json = Json::parse(&text).unwrap();
    let obj = json.as_object().expect("tokens fixture is a JSON object");
    let mut mismatches = Vec::new();
    for (s, ids_json) in obj {
        let expect: Vec<u32> = ids_json.as_array().unwrap().iter().map(|v| v.as_u32().unwrap()).collect();
        let got = tok.encode(s);
        if got != expect {
            mismatches.push(format!("{s:?}: expected {expect:?}, got {got:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{} of {} strings mismatched:\n{}", mismatches.len(), obj.len(), mismatches.join("\n"));
}

fn check_f32_logits_fixture(model: &ocrcer_llm::Model, model_prefix: &str) {
    let manifest_text = fs::read_to_string(fixtures_dir().join(format!("{model_prefix}_logits.json"))).unwrap();
    let manifest = Json::parse(&manifest_text).unwrap();
    let vocab_size = manifest.get("vocab_size").unwrap().as_u32().unwrap() as usize;
    assert_eq!(vocab_size, model.config.vocab_size, "fixture vocab_size disagrees with the packed model's config");

    let mut max_abs_diff = 0.0f32;
    let mut top5_agreements = 0;
    let mut n = 0;
    for entry in manifest.get("prompts").unwrap().as_array().unwrap() {
        let tokens: Vec<u32> = entry.get("tokens").unwrap().as_array().unwrap().iter().map(|v| v.as_u32().unwrap()).collect();
        let expect_top5: Vec<usize> =
            entry.get("top5_ids").unwrap().as_array().unwrap().iter().map(|v| v.as_u32().unwrap() as usize).collect();
        let bin_name = entry.get("logits_file").unwrap().as_str().unwrap();
        let expect = read_logits_bin(&fixtures_dir().join(bin_name), vocab_size);

        let got = model.logits(&tokens);
        assert_eq!(got.len(), vocab_size);

        for (&g, &e) in got.iter().zip(&expect) {
            max_abs_diff = max_abs_diff.max((g - e).abs());
        }
        let got_top5 = topk(&got, 5);
        if got_top5[..1] == expect_top5[..1] {
            top5_agreements += 1;
        }
        n += 1;
        eprintln!(
            "{model_prefix} {:?}: top1 rust={} hf={}, max|diff| so far={:.6}",
            entry.get("text").unwrap().as_str().unwrap(),
            got_top5[0],
            expect_top5[0],
            max_abs_diff
        );
    }
    assert_eq!(top5_agreements, n, "top-1 token disagreed with the reference on at least one prompt");
    // f32 vs f32, same math, different implementations (torch's fused BLAS
    // kernels vs this crate's plain summation) across a 24-28 layer network:
    // some drift from summation order is expected, not a bug. Measured on
    // both shipped models (2026-09-24, docs/measurements/2026-09-24_llm_engine.txt):
    // qwen2.5-0.5b max|diff| 0.000110, qwen3-0.6b max|diff| 0.000079 — this
    // bound carries roughly 90x headroom over both readings, not a promise
    // about a future model's summation drift.
    assert!(max_abs_diff < 0.01, "{model_prefix}: max|diff| {max_abs_diff} exceeds the 0.01-logit bound");
}

fn check_q8_agrees_with_f32(f32_model: &ocrcer_llm::Model, q8_model: &ocrcer_llm::Model, tok: &ocrcer_llm::Tokenizer, model_prefix: &str) {
    let manifest_text = fs::read_to_string(fixtures_dir().join(format!("{model_prefix}_logits.json"))).unwrap();
    let manifest = Json::parse(&manifest_text).unwrap();
    let mut top1_matches = 0;
    let mut n = 0;
    let mut kl_sum = 0.0f64;
    for entry in manifest.get("prompts").unwrap().as_array().unwrap() {
        let tokens: Vec<u32> = entry.get("tokens").unwrap().as_array().unwrap().iter().map(|v| v.as_u32().unwrap()).collect();
        let a = f32_model.logits(&tokens);
        let b = q8_model.logits(&tokens);
        if top1(&a) == top1(&b) {
            top1_matches += 1;
        }
        kl_sum += mean_kl(&a, &b);
        n += 1;
    }

    // 200 tokens of running text, every position compared (not just the
    // last), per the chunk 16a verification requirement.
    let text = "The quarterly report shows revenue of $4,502,118 against a budget of $4,100,000, \
        a variance the finance team attributes to strong demand in the industrial segment. Part \
        number 4402-A-REV2 passed inspection at a tolerance of plus 0.010 minus 0.005 millimetres, \
        and the M8x1.25 fastener torque spec was verified against the drawing. Net income for the \
        quarter was $612,340, up 12.5 percent year over year. The board approved a dividend of \
        $0.42 per share, payable to shareholders of record as of the close of business on the last \
        trading day of the month. Meanwhile, the engineering team completed a design review of the \
        bracket assembly, confirming that the R0.5 fillet radius meets the fatigue requirement under \
        cyclic loading. Total headcount grew by fourteen employees during the period, and the \
        company opened a new distribution centre to reduce shipping times to the western region.";
    let tokens = tok.encode(text);
    assert!(tokens.len() >= 150, "sample text tokenized shorter than expected: {} tokens", tokens.len());
    let a_all = f32_model.logits_all(&tokens);
    let b_all = q8_model.logits_all(&tokens);
    let mut text_top1_matches = 0;
    let mut text_kl_sum = 0.0f64;
    for (a, b) in a_all.iter().zip(&b_all) {
        if top1(a) == top1(b) {
            text_top1_matches += 1;
        }
        text_kl_sum += mean_kl(a, b);
    }

    eprintln!(
        "{model_prefix} q8-vs-f32: 5 prompts top1 {top1_matches}/{n}, mean KL {:.6} nats; {} text tokens top1 {text_top1_matches}/{}, mean KL {:.6} nats",
        kl_sum / n as f64,
        tokens.len(),
        tokens.len(),
        text_kl_sum / tokens.len() as f64
    );
    // Measured 2026-09-24 (docs/measurements/2026-09-24_llm_engine.txt):
    // qwen2.5-0.5b 5/5 prompts, 213/219 (97.3%) text tokens; qwen3-0.6b 5/5
    // prompts, 212/219 (96.8%) text tokens. 90% carries headroom over both
    // readings without being a promise about a future model's block
    // quantization error.
    assert!(top1_matches as f64 / n as f64 >= 0.8, "{model_prefix}: q8 top-1 agreement on the 5 prompts fell below 80%");
    assert!(
        text_top1_matches as f64 / tokens.len() as f64 >= 0.9,
        "{model_prefix}: q8 top-1 agreement on the 200-token text fell below 90%"
    );
}

#[test]
#[ignore = "needs a .ocrl packed from real weights; see module docs"]
fn qwen3_tokenizer_matches_reference() {
    let (_, tok) = ocrl_from_env("OCRCER_LLM_QWEN3_F32_OCRL").expect("set OCRCER_LLM_QWEN3_F32_OCRL");
    check_tokenizer_fixture(&tok, "qwen3_0.6b");
}

#[test]
#[ignore = "needs a .ocrl packed from real weights; see module docs"]
fn qwen25_tokenizer_matches_reference() {
    let (_, tok) = ocrl_from_env("OCRCER_LLM_QWEN25_F32_OCRL").expect("set OCRCER_LLM_QWEN25_F32_OCRL");
    check_tokenizer_fixture(&tok, "qwen2.5_0.5b");
}

#[test]
#[ignore = "needs a .ocrl packed from real weights; see module docs"]
fn qwen3_f32_logits_match_reference() {
    let (model, _) = ocrl_from_env("OCRCER_LLM_QWEN3_F32_OCRL").expect("set OCRCER_LLM_QWEN3_F32_OCRL");
    check_f32_logits_fixture(&model, "qwen3_0.6b");
}

#[test]
#[ignore = "needs a .ocrl packed from real weights; see module docs"]
fn qwen25_f32_logits_match_reference() {
    let (model, _) = ocrl_from_env("OCRCER_LLM_QWEN25_F32_OCRL").expect("set OCRCER_LLM_QWEN25_F32_OCRL");
    check_f32_logits_fixture(&model, "qwen2.5_0.5b");
}

#[test]
#[ignore = "needs .ocrl files packed from real weights; see module docs"]
fn qwen3_q8_agrees_with_f32() {
    let (f32_model, tok) = ocrl_from_env("OCRCER_LLM_QWEN3_F32_OCRL").expect("set OCRCER_LLM_QWEN3_F32_OCRL");
    let (q8_model, _) = ocrl_from_env("OCRCER_LLM_QWEN3_Q8_OCRL").expect("set OCRCER_LLM_QWEN3_Q8_OCRL");
    check_q8_agrees_with_f32(&f32_model, &q8_model, &tok, "qwen3_0.6b");
}

#[test]
#[ignore = "needs .ocrl files packed from real weights; see module docs"]
fn qwen25_q8_agrees_with_f32() {
    let (f32_model, tok) = ocrl_from_env("OCRCER_LLM_QWEN25_F32_OCRL").expect("set OCRCER_LLM_QWEN25_F32_OCRL");
    let (q8_model, _) = ocrl_from_env("OCRCER_LLM_QWEN25_Q8_OCRL").expect("set OCRCER_LLM_QWEN25_Q8_OCRL");
    check_q8_agrees_with_f32(&f32_model, &q8_model, &tok, "qwen2.5_0.5b");
}
