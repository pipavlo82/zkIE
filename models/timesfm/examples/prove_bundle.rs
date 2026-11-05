//! B2: prove the full TimesFM graph under a Generic statement and write the
//! public verification bundle. TimesFM still contains the out-of-scope
//! fail-closed LayerNormCentered op, so this example reports the unsupported
//! boundary instead of producing an unverified bundle.

use std::path::Path;
use std::time::Instant;

use zkie_core::pcs::whir::Whir;
use zkie_models_timesfm::build_timesfm;
use zkie_ops::bundle::{
    index_shapes, public_execution, verify_bundle_dir, write_bundle, PublicBundle,
};
use zkie_ops::compose::{
    commit_model_weights, evaluate_ops, prove_committed_shard_dag_bound, PublicStatement,
    Store, TrustedModelManifest,
};

fn main() {
    let dir = "models/timesfm/weights";
    let bundle_dir = std::env::var("ZKIE_BUNDLE_DIR")
        .unwrap_or_else(|_| "/tmp/zkie-timesfm-bundle".to_string());
    let cache_dir = std::env::var("ZKIE_CACHE_DIR")
        .unwrap_or_else(|_| "/tmp/zkie-b2-cache-timesfm".to_string());
    let mut store = Store::new();
    let mut ops = Vec::new();
    let _out = build_timesfm(&mut store, &mut ops, dir, 16);
    evaluate_ops(&mut store, &ops);
    let cached = commit_model_weights(&store, &ops, &cache_dir);
    let model = TrustedModelManifest::from_offline_commitment(
        &cached, &store.owned_slices(), &ops).with_index_shapes(&index_shapes(&store));
    let execution = match public_execution(&store, &ops, &cached) {
        Some(e) => e,
        None => {
            eprintln!("TimesFM still hits the fail-closed LayerNormCentered gap; no bundle written");
            std::process::exit(2);
        }
    };
    let stmt = PublicStatement::Generic;
    let whir = Whir::new_testing(5);
    let t0 = Instant::now();
    let proof = prove_committed_shard_dag_bound(
        &mut store, &ops, 1, &whir, &cached, 0, 0, &stmt, &model, &execution)
        .expect("TimesFM bound proof");
    let elapsed = t0.elapsed();
    let bytes = bincode::serialize(&proof).expect("serialize proof");
    let bundle = PublicBundle::new(ops.clone(), 1, stmt, model, execution);
    write_bundle(Path::new(&bundle_dir), &bundle, &bytes).expect("write bundle");
    let ok = verify_bundle_dir(Path::new(&bundle_dir));
    println!("timesfm bundle={bundle_dir} prove={elapsed:?} proof_bytes={} independent_verify={ok}",
        bytes.len());
}
