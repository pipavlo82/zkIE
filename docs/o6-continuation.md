# O6 continuation record — split witness/proof work; reuse Projection matmul

State: `d8ec50c` + this O6 change on `fix/soundness-repair` (test machine
`/data/zkie-soundness-fix`). O6 is NOT complete pending TL review.

## Inventory: where the gist's pattern actually lives

- The "run a plain proof first, then re-prove the fractional sections with
  commitment-derived challenges" pattern is in the **retired committed path**:
  `prove_committed_shard_dag_inner` calls
  `prove_shard_dag_inner(..., FracMode::Committed)`, whose permutation/gather
  fractional proofs run on scratch transcripts and are re-proven later on the
  committed transcripts (comments in `prove_shard_precomputed`). The committed
  verifier permanently rejects (`verify_committed_shard_dag` is retired), so no
  accepted proof exercises that path. It was not modified; a separate cleanup
  would be a dedicated task.
- The **active bound path** already has the intended split: forward/aux witness
  construction and commitments happen first, then the transcript/challenges are
  derived, then the reverse DAG proof is produced. It does not build a plain
  proof and discard it.
- The remaining O6 redundancy on the active path is the Projection proof:
  the forward computes `h = x @ w` to produce `out`/`rem` and then drops it,
  and `prove_reverse_projection` recomputed the same product; the weight was
  also materialized twice.

## Implementation (active bound path)

- `Store` has a bounded per-store cache of the forward Projection product
  `h = x @ w`, keyed by the Projection output tensor id, with a byte budget.
  The bound prover enables it (default 256 MB; `ZKIE_O6_CACHE_MB=0` disables it
  for baseline measurement or memory pressure). The forward moves `h` into the
  cache without copying when the budget allows; otherwise it is dropped and
  the proof recomputes it (controlled fallback).
- `prove_reverse_projection` reuses the cached `h` when present (one cache
  hit), otherwise recomputes it once; the weight is materialized exactly once
  and shared with the matmul proof.
- Per-store metrics count forward matmul calls, reverse-proof recomputes and
  cache hits, plus cached tensors/bytes. These are measurement counters only;
  no proof value, transcript message or authentication relation changes.
- No all-model activation residency is introduced: only Projection `h`
  buffers are cached, bounded by the byte budget.

## Measurements

`crates/zkie-ops/examples/o6_projection.rs`, one real Projection
`m=256, k=1024, n=256, shift=8` with bounded fixed-point inputs (release):

| mode | fwd mm | proof mm | hits | cached | bytes | wall | proof bytes / hash |
|---|---|---|---|---|---|---|---|
| `ZKIE_O6_CACHE_MB=0` | 1 | 1 | 0 | 0 | 0 | 147.9-157.0 ms | 275,815 / `9ce6b98fa9199322` |
| default 256 MB | 1 | 0 | 1 | 1 | 524,288 | 153.8-155.0 ms | 275,815 / `9ce6b98fa9199322` |

The repeated matmul product is eliminated (two computations -> one). The wall
time is dominated by the Projection aux/WHIR proof at these sizes and the
difference is within run-to-run host noise; the metric that matters is the
single computation and the identical proof bytes. Cached `h` adds 512 KiB,
within the budget; VmHWM stayed ~49 MB in both runs.

## Regression

- New `o6_projection_h_cache_reuses_forward_product`: the bound prover computes
  the forward product once (`fwd=1`), the proof reuses it (`recompute=0`,
  `hit=1`), the cache holds one tensor, and the proof verifies.
- `zkie-ops --lib`: 159/0 (158 + the new O6 test); all existing negative tests
  remain green (wrong boundary/claims, tampered witnesses, retired committed
  verifier rejection).
- Real segmented A/B/C: proof bytes identical to the pre-O6 dumps
  (241,874 / 462,836 / 355,183 B); verify 15.5 ms (C) / 29.4 ms (B). The real
  chain has no Projection ops, so O6 does not alter it.
- Real full64: `CORE-CHAIN-OK`; 64 claims, one `W_t` opening; B 510,969 B;
  verify 154.8 ms.
- No transcript/authentication/security change; proof bytes unchanged; 90-bit.

## Remaining

O6 awaits TL review. O7 is not started; S7 is not accepted; S8/S9 remain
postponed. The retired committed path's scratch/re-prove structure is
documented above and intentionally untouched.
