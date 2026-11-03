# O5 continuation record (W_t grouping + streaming Gather)

State: HEAD `a2e9dc5` on `fix/soundness-repair` (test machine
`/data/zkie-soundness-fix`, remote origin trustless-ai/zkIE). S7 is NOT
accepted. No security/transcript change may be introduced by O5.

## What is already landed (O5 steps 1 + measurements)

- `commit_model_weights`: each weight is materialized once; its original
  length is captured in the group tuple (no second `store.materialize` just
  for `tensor_lengths`).
- `prove_committed_shard_dag_bound` weight-opening loop: for batches whose
  claims all target distinct tables the padded buffer is MOVED (no clone);
  duplicate-table claims still clone and print
  `O5-metrics batch=.. claims=.. unique=.. cloned_bytes=.. tables=..`.
- Phase instrumentation: `O5-phases mat_ms=.. restore_ms=.. open_ms=..
  rss_now_mat=.. rss_now_restore=.. rss_now_clone=.. rss_now_open=.. hwm=..`
  (VmRSS sampled at four points; VmHWM only as the cumulative peak).
- Measured (layer-10 sliced run, 90-bit, warm page cache; prover-data cache
  in /dev/shm tmpfs): mat 45 ms; restore 420-430 ms (+~1.05 GB VmRSS);
  clone +2.10 GB (equals cloned_bytes 2,147,483,648); open 17.2 s and drops
  current RSS to 2.8-3.1 GB while VmHWM reaches 10.0-10.3 GB. Cold page
  cache (sync + drop_caches) is within noise. The clone's 2.10 GB is live
  pre-open and cannot be excluded from the peak via VmHWM alone; the open's
  transient peak dominates HWM.

## Remaining A: combined equality W_t grouping (batch_open)

Files: `crates/zkie-core/src/pcs/batch_open.rs`,
`crates/zkie-core/src/common/sumcheck.rs`,
`crates/zkie-ops/src/compose.rs` (bound loop only as a caller).

Current prover `prove_batch_open_committed_with_data` (~line 292):
- builds one eq-weight vector per CLAIM (len 2^arity each), then calls
  `prove_sum_of_products(&weights, &tensors, claimed_sum, &mut t)` where
  `tensors` has one entry per claim (the same table may be cloned N times);
- `claimed_sum = Σ γ^i claimed_i`; proof `BatchOpeningProof { sumcheck,
  open, evals }`.

Target (no new primitive):
- Group claims by `table_idxs[i]`. For each unique table t:
  `W_t = Σ_{i: table_i=t} γ^i · eq(r_i, X)` (one buffer of len 2^arity built
  by adding scaled eq vectors), and use ONE `tensors[t]` slice.
- Run `prove_sum_of_products(&[W_t], &[table_t], claimed_sum)` over the
  unique tables; `claimed_sum` is unchanged. The gamma/point absorption
  order must stay exactly as now; only the set of sumcheck polynomials
  changes (n -> #unique tables), so the transcript shape changes for proofs
  but the authentication relation is identical (a random γ-combination of
  the same claims, verified against the same committed tables).
- Verifier `verify_batch_open_committed` (~line 338): derive the same
  grouping from the public `table_idxs`, rebuild `W_t`, and check the
  sumcheck final value against the unique-table final evals; the WHIR
  multi-open is over the unique tables (num_tables unchanged, table_idxs
  must be adjusted to the unique table indices in the same order for the
  opening). Keep the FS absorption order unchanged.
- Tests: existing batch-opening tests + malicious cases must stay green;
  add a same-table multi-claim test asserting the grouping equals the
  per-claim opening relation and that a tampered claim/table index rejects.
- Expected effect: duplicate-table clones and per-claim eq vectors vanish
  from the bound GatherMatMul weight batch (2.10 GB here; 16 GB for the full
  64-expert weight).

## Remaining B: streaming GatherMatMul weight evaluation

Doc: `docs/streaming-weight-gather.md` (linear-combination binding, not a
new reduction). Locations: `crates/zkie-ops/src/compose.rs` forward
GatherMatMul arm (`forward_ops`, ~6676/6918) and reverse
`prove_reverse_gather_matmul` (~3318-3325, currently `store.materialize(w)`),
plus its verifier and the op-layer weight binding.

Target: compute `w_v[pos][d] = Σ_j W_all[expert_of[pos]][d][j]·eq(j,v)` by
streaming one `H x MOE_PAD` real-expert block at a time from a shared
mmap/owned handle, without materializing the whole weight for both
forward and reverse. Emit the weight claim as `(counts_e, table_eval[e])`
and bind it by opening `W_all[e]` at the n points `(e, ch_d, v)` (batched)
with the linear combination check. Preserve the eq-selector sumcheck, the
transcript order and all authentication; no new primitive.

## Measurement protocol (mandatory)

- Current RSS (VmRSS) at every phase; VmHWM labeled as cumulative only.
- Warm and cold page cache runs; state explicitly that /dev/shm (tmpfs) is
  not dropped. A cold run command:
  `sync; echo 3 > /proc/sys/vm/drop_caches` then the exact example command.
- Record unique tensors / claims / cloned bytes / RSS / page faults
  (`/proc/self/status` minflt, majflt) / read bytes (`/proc/self/io`) /
  prove time, at 90-bit security, same real inputs and authentication scope.
- File-cache hits are not algorithmic speedup.

## Test gate before the S7 full 64-expert regression

- `cargo test -p zkie-ops --lib` = 153/0 (remote test machine).
- Real segmented run (A/B, bytes entry) green: composition honest verify,
  invalid slice identity/slice mismatch and individually-valid A1+B2
  mismatch rejected, all eight real-data negatives rejected.
- New W_t and streaming tests green; malicious proof bytes still rejected.
- Then re-run the S7 full 64-expert weight case (previous OOM: anon-rss
  502,710,376 kB) with the new memory profile and report faithfully.

## Resume commands (test machine)

```
cd /data/zkie-soundness-fix
git log --oneline -1                    # expect a2e9dc5 + this doc commit
cargo test -p zkie-ops --lib
DS_WEIGHTS=/data/zkIE/models/deepseek-v2-lite/weights \
ZKIE_CACHE=/dev/shm/zkie-core-chain-cache \
cargo run --release -p zkie-models-deepseek-v2-lite --example prove_core_chain
```

If local cargo stalls on the shared registry lock, build/verify on the test
machine (no lock contention) rather than polling the local one.

## Acceptance reminders

- S7 is not accepted; the B segment stops at the expert gate projection
  (no router-gate consistency chain / Gather) and its verifier reads the
  full routing table. The slice-weight identity vs the full 64-expert model
  is a trusted-offline assumption, not verified by the composition verifier.
- Do not claim O5 completion until A+B are implemented, tested and compared
  under the protocol above.
