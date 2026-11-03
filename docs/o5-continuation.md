# O5 continuation record (W_t grouping + streaming Gather)

State: `3e49e59` + this O5 step on `fix/soundness-repair` (test machine
`/data/zkie-soundness-fix`, remote origin trustless-ai/zkIE). S7 is NOT
accepted. No security/transcript change is introduced by O5.

## Landed in this step (A + B)

### A. Combined equality W_t grouping (batch opening)

- `claim_groups(table_idxs)` (crates/zkie-core/src/pcs/batch_open.rs): unique
  tables in order of first appearance plus each claim's group position, shared
  by prover and verifier.
- Prover `prove_batch_open_committed_with_data`: one combined equality weight
  per unique table,
  `W_t(X) = sum_{i: table_i = t} gamma^i * eq(r_i, X)`,
  built by adding scaled eq vectors; `prove_sum_of_products_owned` runs over
  one polynomial pair per unique table. The global gamma powers, `claimed_sum`,
  and the Fiat-Shamir absorption order are unchanged (gamma powers stay global
  over the original claim order; only the sumcheck polynomial count changes).
- Verifier `verify_batch_open_committed`: derives the identical grouping from
  the public `table_idxs` and checks
  `b_eval == sum_g W_g(r) * evals[unique[g]]`, i.e. the same randomized
  gamma-combination of the same claims against the same committed tables.
  The WHIR multi-open still opens every batch table at the fresh point, so the
  unique-table indices are the batch indices. Added fail-closed checks for
  per-claim point arity and out-of-range table indices (previously a panic).
- New `prove_batch_open_committed_unique_owned` takes exactly one owned tensor
  per unique table (first-appearance order) and MOVES it; the legacy
  per-claim entry points keep their signatures and dedupe internally.
- `prove_sum_of_products_owned` (sumcheck.rs): the folding pass takes the
  polynomial buffers by value, so a multi-GB table is not cloned inside the
  sumcheck.
- `mle::eq_evals`: O(2^d) doubling construction instead of O(d*2^d); the
  values are bit-identical to the direct product (dedicated test), so no
  proof/transcript change.
- Bound weight-opening loop (compose.rs): groups claims by table, moves each
  unique padded buffer, drops unreferenced tables before opening, and prints
  `O5-metrics batch=.. claims=.. unique=.. moved_bytes=.. cloned_bytes=0
  tables=..`.
- Tests: `same_table_multi_claim_grouping` (grouped and per-claim provers
  agree on sumcheck + evals; tampered claimed value, tampered table index,
  out-of-range index, tampered b_eval rejected); all existing batch-opening
  tests stay green.

### B. Streaming GatherMatMul

- `WeightBlocks::{Slice, Mmap}` (batched_matmul.rs) + `Store::weight_blocks`:
  a shared owned/mmap handle that yields one `K x N` expert block at a time.
  Owned data is borrowed; mmap data converts one block per expert and frees it
  immediately, so the whole expert-major W_all is never materialized for
  evaluation.
- `stream_weights`: for each expert block compute the row folds
  `row_dots[e][d] = sum_j W_all[e][d][j] * eq(j, v)` once, then
  `w_v[pos][d] = row_dots[expert_of[pos]][d]`; the per-expert terminal
  `table_eval[e] = MLE(W_all[e], (ch_d, v))` is recovered from the stored row
  folds after the sumcheck (`table_evals`), with no second weight pass.
- `gather_matmul_forward_streaming` (forward), `prove_with_w_v` (the existing
  eq-selector sumcheck), `prove_gather_matmul_streaming` (self-contained path).
- Forward, plain prove and the bound reverse reduction no longer materialize
  W_all. The bound weight claim is still `(counts_e, table_eval[e])`, bound by
  opening `W_all[e]` at the n points `(e, ch_d, v)` batched with the
  linear-combination check. No new reduction; transcript and authentication
  semantics are unchanged; out-of-range expert ids fail closed.
- Tests: `streaming_from_mmap_matches_slice` (mmap vs owned forward, w_v and
  per-expert terminals agree with the materialized reference; the streamed
  proof verifies; a tampered terminal is rejected) and
  `s7_gather_matmul_mmap_weight_bound_roundtrip` (bound path with an
  mmap-backed weight; honest verify + bytes verify; tampered weight terminal
  rejected).

## Test gate (test machine)

- `cargo test -p zkie-ops --lib`: 154 passed, 0 failed (153 baseline + the new
  mmap bound regression).
- `cargo test -p zkie-core --lib`: 69 passed, 0 failed, 1 ignored (includes the
  new `eq_evals_matches_direct_product` and `same_table_multi_claim_grouping`).
- Real segmented example (`prove_core_chain`, 8-expert real slice): composition
  honest verify, invalid slice identity / slice mismatch rejected,
  individually-valid A1+B2 mismatch rejected, all eight real-data negatives
  rejected, SEGMENT-OK.

## Measurements (real layer-10 segmented example, 8-expert slice, 90-bit)

Command:
`DS_WEIGHTS=/data/zkIE/models/deepseek-v2-lite/weights ZKIE_CACHE=/dev/shm/zkie-core-chain-cache target/release/examples/prove_core_chain`

Warm page cache (measured): wall 11.147 s; maxrss 2,417,552 kB (rusage);
minflt 71,570; majflt 0; `ru_inblock` 0; sampled `/proc/self/io` read_bytes 0
(file-cache hits are not an algorithmic speedup). First weight batch:
`claims=8 unique=1 moved_bytes=268,435,456 cloned_bytes=0`; mat 45.7 ms,
restore 426.4 ms, open 3,177.4 ms; VmRSS mat 555,584 kB, restore 1,616,468 kB,
open 422,340 kB; VmHWM 2,133,352 kB (cumulative). Second (B2) open 3,119.9 ms,
VmHWM 2,417,552 kB cumulative. B commit+prove 5.30 s, verify 29.7 ms, B proof
461,940 B. Composition and all eight negatives rejected; SEGMENT-OK.

Cold page cache (`sync; echo 3 > /proc/sys/vm/drop_caches`; /dev/shm is tmpfs
and is NOT dropped): wall 12.193 s; maxrss 2,449,648 kB; minflt 69,028;
majflt 6; `ru_inblock` 2,106,888 (512-byte blocks); sampled read_bytes
1,078,726,656 (1.08 GB, essentially the full 64-expert weight file read by the
example's `load_expert_slice`). B commit+prove 5.247 s, verify 30.0 ms; all
negative classes rejected; SEGMENT-OK.

Before this step (same example, documented baseline): per-claim clone
+2.10 GB, open 17.2 s, VmHWM 10.0-10.3 GB, B commit+prove 19.9 s. After:
cloned_bytes=0, 256 MiB moved once, open 3.12-3.18 s, VmHWM 2.42 GB, B
commit+prove 5.25-5.30 s. Same 90-bit security, same real inputs, same
authentication scope.

## S7 full 64-expert weight regression (previously OOM-killed)

Harness: the temporary `/tmp/prove_core_chain.rs` full-64 variant (a single
2^28-element real expert-gate weight, mmap-backed, 64 claims on one table),
built as `prove_core_chain_full64` and NOT committed; same real layer-10 data,
90-bit, warm page cache, cache in /dev/shm.

Result: completes (no OOM).
- `O5-metrics batch=0 claims=64 unique=1 moved_bytes=2,147,483,648
  cloned_bytes=0 tables=1`
- `O5-phases mat_ms=686.8 restore_ms=3279.9 open_ms=127367.4
  rss_now_mat=3,177,672 rss_now_restore=11,808,392 rss_now_clone=11,808,392
  rss_now_open=1,192,540 hwm=15,998,796` (kB; VmHWM is cumulative)
- B commit+prove 142.71 s, verify 154.7 ms, proof 513,849 B, peak RSS
  15,998,796 kB (15.26 GiB); wall 153.0 s; minflt 271,684; majflt 0; sampled
  read_bytes 0 (warm).
- Harness negatives rejected: A gate/score tamper, B public routing array,
  B g output, wrong expert root, wrong activation table root;
  `CORE-CHAIN-OK`.

Previous state: OOM-killed at anon-rss 502,710,376 kB (dmesg) on the 495 GB
machine. The staged RSS shows the peak window is the W_t construction after
the prover-data restore: restore VmRSS 11.26 GiB, then one 2 GiB table buffer +
one 2 GiB W_t + one 2 GiB transient eq buffer; by the opening sample current
VmRSS is 1.14 GiB and VmHWM stays at 15.26 GiB. The opening (127 s) dominates
the time, not memory.

## Remaining / acceptance reminders

- S7 is still NOT accepted: the segmented B verifier ingests the full routing
  table as public data, and the slice-weight identity vs the full 64-expert
  model is a trusted-offline assumption. O5 changes none of that. The full
  64-expert case is now a working memory regression, not S7 acceptance.
- Next: S7 router-gate consistency chain + Gather; then O2/O3/O4/O6/O7; S8/S9
  postponed.

## Resume commands (test machine)

```
cd /data/zkie-soundness-fix
cargo test -p zkie-ops --lib
DS_WEIGHTS=/data/zkIE/models/deepseek-v2-lite/weights \
ZKIE_CACHE=/dev/shm/zkie-core-chain-cache \
cargo run --release -p zkie-models-deepseek-v2-lite --example prove_core_chain
```
