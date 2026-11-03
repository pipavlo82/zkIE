//! S7 core-chain real-data proofs (bound/bytes entry), two-statement shape.
//!
//! The single-statement chain cannot certify its internal routing INDEX
//! intermediates: IdxBroadcast requires the base to be verifier-replayable
//! or certified public u32 data, and replacing that with random-point claims
//! would drop the pointwise u32 / b*base+j overflow contract. Instead the
//! chain is split at the routing arrays, which are PUBLIC data in the S7
//! statement:
//!
//!   A: scores -> TopKSelect -> Scatter -> PadIdx
//!      public outputs: thr, gate, d1, d2, token_id_pad, expert_of_pad
//!   B: token_id_pad/expert_of_pad (public index inputs) -> IdxBroadcast
//!      -> Lookup(real post-attn activation table) -> BroadcastScale(mask)
//!      -> GatherMatMul(real expert gate weights) -> Scale(shift 16)
//!      public outputs: g
//!
//! IdxBroadcast in B uses the existing verifier replay path, which checks
//! the public index values are < 2^32 and uses checked u32 arithmetic for
//! b*base+j. No reduction is added and no constraint is bypassed.
//!
//! Real inputs: scripts/ds_layer_core_inputs.py exports the layer-L post-attn
//! activation and the Q16 router scores with the model's fixed-point recipe.
//!
//! BOUNDARY (NOT gist S7 acceptance): this is a two-statement segmented
//! real-data experiment. The verifier of B ingests the full routing arrays as
//! public data, which the gist forbids for the final S7 construction, and the
//! A/B link is a caller-trusted external statement, not one proof. This
//! harness exists to exercise the full 64-expert weight (no slice identity
//! assumption: the real L*_experts_gate_i32.bin is used directly) at 90-bit
//! security and to keep the O5 cold/warm memory regression reproducible; it
//! does not certify the router-gate consistency chain or Gather into the
//! router output.

use std::sync::Arc;
use std::time::Instant;

use zkie_core::common::field::{Goldilocks, PrimeCharacteristicRing};
use zkie_core::common::fixed_point::{from_i32, to_i64};
use zkie_core::common::weights_io::WeightMmap;
use zkie_core::pcs::whir::Whir;
use zkie_ops::compose::{
    commit_model_weights, evaluate_ops, prove_committed_shard_dag_bound,
    verify_committed_shard_dag_bound_bytes, Op, PublicStatement, Store, TensorKey,
    TrustedExecution, TrustedModelManifest,
};

const H: usize = 2048;
const N: usize = 64;
const K: usize = 6;
const MOE_PAD: usize = 2048;

fn load_i32(path: &str) -> Vec<Goldilocks> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    bytes
        .chunks_exact(4)
        .map(|c| from_i32(i32::from_le_bytes([c[0], c[1], c[2], c[3]])))
        .collect()
}

fn peak_rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("VmHWM:") {
            return v.trim().trim_end_matches(" kB").parse().unwrap_or(0);
        }
    }
    0
}

fn to_g(v: &[u32]) -> Vec<Goldilocks> {
    v.iter().map(|&x| Goldilocks::from_u64(x as u64)).collect()
}

fn main() {
    let dir = std::env::var("DS_WEIGHTS")
        .unwrap_or_else(|_| "/data/zkIE/models/deepseek-v2-lite/weights".to_string());
    let layer: usize = std::env::var("DS_LAYER").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let cache = std::env::var("ZKIE_CACHE")
        .unwrap_or_else(|_| "/dev/shm/zkie-core-chain-cache".to_string());
    let m = 1usize;
    let s = K * m;
    let s_pad = s.next_power_of_two();
    let scores_v = load_i32(&format!("{dir}/L{layer}_core_scores_i32.bin"));
    let act_v = load_i32(&format!("{dir}/L{layer}_core_act_i32.bin"));
    assert_eq!(scores_v.len(), m * N, "real router scores length");
    assert_eq!(act_v.len(), m * H, "real activation length");
    let max_abs = scores_v.iter().map(|v| { let x = to_i64(*v); if x < 0 { -x } else { x } }).max().unwrap();
    let max_v = scores_v.iter().map(|v| to_i64(*v)).max().unwrap();
    println!("layer={layer} seq=1 k={K} real scores: max|.|={max_abs} max={max_v}");

    // ---------------- Phase A: selection + routing + padding ----------------
    let mut sa = Store::new();
    let scores_a = sa.push(scores_v.clone());
    let sel = sa.push(vec![]);
    let thr = sa.push(vec![]);
    let gate = sa.push(vec![]);
    let d1 = sa.push(vec![]);
    let d2 = sa.push(vec![]);
    let token_id = sa.push_idx(vec![]);
    let expert_of = sa.push_idx(vec![]);
    let tok_pad = sa.push_idx(vec![]);
    let exp_pad = sa.push_idx(vec![]);
    let ops_a = vec![
        Op::TopKSelect { x: scores_a, sel, thr, gate, d1, d2, m, n: N, k: K },
        Op::Scatter { sel, token_id, expert_of, m, n: N },
        Op::PadIdx { base: token_id, out: tok_pad, len: s_pad, base_len: s },
        Op::PadIdx { base: expert_of, out: exp_pad, len: s_pad, base_len: s },
    ];
    let t0 = Instant::now();
    evaluate_ops(&mut sa, &ops_a);
    println!("A forward {:?}", t0.elapsed());
    let thr_v = sa.get(thr).to_vec();
    let gate_v = sa.get(gate).to_vec();
    let d1_v = sa.get(d1).to_vec();
    let d2_v = sa.get(d2).to_vec();
    let tok_v = sa.idx[tok_pad].clone();
    let exp_v = sa.idx[exp_pad].clone();
    println!("sel/route: thr={:?} experts={:?} token_pad={:?} expert_pad={:?}",
        thr_v.iter().map(|v| to_i64(*v)).collect::<Vec<_>>(), exp_v,
        tok_v, exp_v);
    let tk_key = TensorKey::Index(tok_pad).encoded().unwrap();
    let ek_key = TensorKey::Index(exp_pad).encoded().unwrap();
    let execution_a = TrustedExecution {
        inputs: [(scores_a, scores_v.clone())].into(),
        outputs: [
            (thr, thr_v.clone()), (gate, gate_v.clone()), (d1, d1_v.clone()),
            (d2, d2_v.clone()), (tk_key, to_g(&tok_v)), (ek_key, to_g(&exp_v)),
        ].into(),
    };
    let cache_a = format!("{cache}-a");
    let t1 = Instant::now();
    let cached_a = commit_model_weights(&sa, &ops_a, &cache_a);
    let model_a = TrustedModelManifest::from_offline_commitment(&cached_a, &sa.owned_slices(), &ops_a)
        .with_index_shapes(&[(token_id, s), (expert_of, s)].into());
    let whir = Whir::new_testing(5);
    let proof_a = prove_committed_shard_dag_bound(&mut sa, &ops_a, 1, &whir, &cached_a, 0, 0,
        &PublicStatement::Generic, &model_a, &execution_a)
        .expect("phase A real routing proof");
    let bytes_a = bincode::serialize(&proof_a).expect("serialize A");
    println!("A commit {:?} prove {:?} bytes {}", t1.elapsed(), t1.elapsed(), bytes_a.len());
    assert!(verify_committed_shard_dag_bound_bytes(&bytes_a, &ops_a, 1, &whir,
        &PublicStatement::Generic, &model_a, &execution_a), "phase A must verify");
    // Real-data router range: the TopKSelect proof carries XRange/ThrRange.
    let mut bad_scores = scores_v.clone();
    bad_scores[0] = bad_scores[0] + Goldilocks::from_u64(1u64 << 22);
    let mut sb = Store::new();
    let sc = sb.push(bad_scores.clone());
    let sel_b = sb.push(vec![]); let thr_b = sb.push(vec![]); let gate_b = sb.push(vec![]);
    let d1_b = sb.push(vec![]); let d2_b = sb.push(vec![]);
    let ops_r = vec![Op::TopKSelect { x: sc, sel: sel_b, thr: thr_b, gate: gate_b, d1: d1_b, d2: d2_b, m, n: N, k: K }];
    evaluate_ops(&mut sb, &ops_r);
    let cached_r = commit_model_weights(&sb, &ops_r, &format!("{cache}-range"));
    let model_r = TrustedModelManifest::from_offline_commitment(&cached_r, &sb.owned_slices(), &ops_r);
    // Correct public outputs for the tampered forward, so the only reason the
    // prover can refuse is the out-of-contract router score (range aux).
    let mut exec_r = TrustedExecution {
        inputs: [(sc, bad_scores)].into(),
        outputs: [
            (thr_b, sb.get(thr_b).to_vec()), (gate_b, sb.get(gate_b).to_vec()),
            (d1_b, sb.get(d1_b).to_vec()), (d2_b, sb.get(d2_b).to_vec()),
        ].into(),
    };
    let _ = &exec_r;
    assert!(prove_committed_shard_dag_bound(&mut sb, &ops_r, 1, &whir, &cached_r, 0, 0,
        &PublicStatement::Generic, &model_r, &exec_r).is_none(),
        "out-of-range router score must fail closed at proof level");
    println!("A negative rejected: router score above 2^22 (proof-level range)");

    // ---------------- Phase B: replay routing + expert FFN gate ----------------
    let mut sb = Store::new();
    let tok_b = sb.push_idx(tok_v.clone());
    let exp_b = sb.push_idx(exp_v.clone());
    let mask_v: Vec<Goldilocks> = (0..s_pad)
        .map(|i| if i < s { Goldilocks::ONE } else { Goldilocks::ZERO })
        .collect();
    let mask = sb.push(mask_v.clone());
    let act = sb.push(act_v.clone());
    let idx_seg = sb.push_idx(vec![]);
    let seg = sb.push(vec![]);
    let seg_z = sb.push(vec![]);
    let g_raw = sb.push(vec![]);
    let g = sb.push(vec![]);
    let wmap = Arc::new(WeightMmap::open(&format!("{dir}/L{layer}_experts_gate_i32.bin"))
        .unwrap_or_else(|e| panic!("open expert gate weights: {e}")));
    let w = sb.push_mmap(wmap, 0, N * H * MOE_PAD);
    let ops_b = vec![
        Op::IdxBroadcast { base: tok_b, out: idx_seg, b: H },
        Op::Lookup { idx: idx_seg, out: seg, table: act },
        Op::BroadcastScale { x: seg, scale: mask, out: seg_z, h: H },
        Op::GatherMatMul { a: seg_z, expert_of: exp_b, w, c: g_raw, s: s_pad, k: H, n: MOE_PAD, n_experts: N },
        Op::Scale { x: g_raw, out: g, factor: 1, shift: 16 },
    ];
    let t2 = Instant::now();
    evaluate_ops(&mut sb, &ops_b);
    let g_v = sb.get(g).to_vec();
    println!("B forward {:?} g_len={}", t2.elapsed(), g_v.len());
    let kb_tok = TensorKey::Index(tok_b).encoded().unwrap();
    let kb_exp = TensorKey::Index(exp_b).encoded().unwrap();
    let execution_b = TrustedExecution {
        inputs: [
            (kb_tok, to_g(&tok_v)), (kb_exp, to_g(&exp_v)), (mask, mask_v.clone()),
        ].into(),
        outputs: [(g, g_v.clone())].into(),
    };
    let cache_b = format!("{cache}-b");
    let t3 = Instant::now();
    let cached_b = commit_model_weights(&sb, &ops_b, &cache_b);
    let model_b = TrustedModelManifest::from_offline_commitment(&cached_b, &sb.owned_slices(), &ops_b)
        .with_index_shapes(&[(tok_b, s_pad), (exp_b, s_pad), (idx_seg, s_pad * H)].into());
    let proof_b = prove_committed_shard_dag_bound(&mut sb, &ops_b, 1, &whir, &cached_b, 0, 0,
        &PublicStatement::Generic, &model_b, &execution_b)
        .expect("phase B real FFN proof");
    let bytes_b = bincode::serialize(&proof_b).expect("serialize B");
    let t4 = Instant::now();
    assert!(verify_committed_shard_dag_bound_bytes(&bytes_b, &ops_b, 1, &whir,
        &PublicStatement::Generic, &model_b, &execution_b), "phase B must verify");
    println!("B commit+prove {:?} verify {:?} bytes {} peak_rss={}kB",
        t3.elapsed(), t4.elapsed(), bytes_b.len(), peak_rss_kb());

    // ---------------- real-data malicious regressions (bytes entry) ----------------
    let mut bad_a = execution_a.clone();
    bad_a.outputs.get_mut(&gate).unwrap()[0] = bad_a.outputs[&gate][0] + Goldilocks::ONE;
    assert!(!verify_committed_shard_dag_bound_bytes(&bytes_a, &ops_a, 1, &whir,
        &PublicStatement::Generic, &model_a, &bad_a), "A tampered gate must be rejected");
    println!("negative rejected: A tampered gate");
    let mut bad_a2 = execution_a.clone();
    bad_a2.inputs.get_mut(&scores_a).unwrap()[0] = bad_a2.inputs[&scores_a][0] + Goldilocks::ONE;
    assert!(!verify_committed_shard_dag_bound_bytes(&bytes_a, &ops_a, 1, &whir,
        &PublicStatement::Generic, &model_a, &bad_a2), "A tampered score input must be rejected");
    println!("negative rejected: A tampered router score input");
    let mut bad_b_inputs = execution_b.clone();
    let mut bad_exp = to_g(&exp_v);
    bad_exp[0] = Goldilocks::from_u64(exp_v[1] as u64);
    bad_b_inputs.inputs.insert(kb_exp, bad_exp);
    assert!(!verify_committed_shard_dag_bound_bytes(&bytes_b, &ops_b, 1, &whir,
        &PublicStatement::Generic, &model_b, &bad_b_inputs),
        "B tampered (duplicate) public routing array must be rejected");
    println!("negative rejected: B tampered public routing array");
    let mut bad_b_out = execution_b.clone();
    bad_b_out.outputs.get_mut(&g).unwrap()[0] = bad_b_out.outputs[&g][0] + Goldilocks::ONE;
    assert!(!verify_committed_shard_dag_bound_bytes(&bytes_b, &ops_b, 1, &whir,
        &PublicStatement::Generic, &model_b, &bad_b_out), "B tampered g must be rejected");
    println!("negative rejected: B tampered g output");
    // Wrong expert weight root: substitute an unrelated committed root.
    let mut small = Store::new();
    let xa = small.push(vec![Goldilocks::ONE; 32]);
    let xw = small.push((0..32).map(|i| Goldilocks::from_u64(i as u64 + 1)).collect());
    let xc = small.push(vec![]);
    let xops = vec![Op::MatMul { a: xa, b: xw, c: xc, m: 4, k: 8, n: 4 }];
    let xcached = commit_model_weights(&small, &xops, &format!("{cache}-small"));
    let mut bad_cached = cached_b.clone();
    bad_cached[0].commitment = xcached[0].commitment.clone();
    let bad_model = TrustedModelManifest::from_offline_commitment(&bad_cached, &sb.owned_slices(), &ops_b)
        .with_index_shapes(&[(tok_b, s_pad), (exp_b, s_pad), (idx_seg, s_pad * H)].into());
    assert!(!verify_committed_shard_dag_bound_bytes(&bytes_b, &ops_b, 1, &whir,
        &PublicStatement::Generic, &bad_model, &execution_b), "wrong expert root must be rejected");
    println!("negative rejected: wrong expert weight root");
    // Wrong activation table root (cheap committed table).
    let mut act2 = act_v.clone();
    act2[0] = act2[0] + Goldilocks::ONE;
    let mut s2 = Store::new();
    let tok2 = s2.push_idx(tok_v.clone());
    let exp2 = s2.push_idx(exp_v.clone());
    let mask2 = s2.push(mask_v.clone());
    let act2_t = s2.push(act2);
    let idx2 = s2.push_idx(vec![]); let seg2 = s2.push(vec![]); let segz2 = s2.push(vec![]);
    let graw2 = s2.push(vec![]); let g2 = s2.push(vec![]);
    let wmap2 = Arc::new(WeightMmap::open(&format!("{dir}/L{layer}_experts_gate_i32.bin")).unwrap());
    let w2 = s2.push_mmap(wmap2, 0, N * H * MOE_PAD);
    let ops2 = vec![
        Op::IdxBroadcast { base: tok2, out: idx2, b: H },
        Op::Lookup { idx: idx2, out: seg2, table: act2_t },
        Op::BroadcastScale { x: seg2, scale: mask2, out: segz2, h: H },
        Op::GatherMatMul { a: segz2, expert_of: exp2, w: w2, c: graw2, s: s_pad, k: H, n: MOE_PAD, n_experts: N },
        Op::Scale { x: graw2, out: g2, factor: 1, shift: 16 },
    ];
    // Fill the store shapes (graw2/g2/segz2 lengths) exactly as the honest B
    // path does before committing/inspecting the manifest; without this the
    // Scale input shape is missing and manifest construction panics instead of
    // reaching the verifier. The tampered activation table root remains the
    // only difference from cached_b, so the verifier must reject.
    evaluate_ops(&mut s2, &ops2);
    let cached2 = commit_model_weights(&s2, &ops2, &format!("{cache}-table"));
    let model2 = TrustedModelManifest::from_offline_commitment(&cached2, &s2.owned_slices(), &ops2)
        .with_index_shapes(&[(tok2, s_pad), (exp2, s_pad), (idx2, s_pad * H)].into());
    assert!(!verify_committed_shard_dag_bound_bytes(&bytes_b, &ops_b, 1, &whir,
        &PublicStatement::Generic, &model2, &execution_b), "wrong activation table root must be rejected");
    println!("negative rejected: wrong activation table root");
    println!("CORE-CHAIN-OK layer={layer} A_bytes={} B_bytes={}", bytes_a.len(), bytes_b.len());
}
