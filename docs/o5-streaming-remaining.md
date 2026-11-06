# O5 remaining: per-batch weight streaming, fold budgets, W_t prefix reuse

Scope: prover-side memory/time work for the committed weight-opening stage.
No transcript, commitment, or verifier-acceptance rule changes.

## Per-batch mmap streaming

Before, each weight proof materialized every parent weight tensor into an
owned `Arc<Vec<Goldilocks>>` and held it for the whole opening stage; each
`2^arity` chunk was then copied out of that parent, and every split sub-claim
recomputed its MLE value from the chunk.

Now the stage walks the committed weight batches in order:

- for every table of the batch, read only the `chunk_index`-th `2^arity`
  block through `Store::weight_blocks` (owned slice or mmap range);
- rebuild the batch PCS prover data from those blocks;
- move one buffer per unique claimed table into the batch opening and drop the
  rest before the opening starts.

No parent tensor is owned across batches. A tail chunk keeps the previous
zero-padding semantics: it copies the available prefix and leaves the rest
zero, matching the padded offline commitment. All tables of a committed batch
are read because the WHIR prover data must be rebuilt over the full batch;
only the claimed unique tables are kept for the opening. The mmap path fills
the chunk buffer in place (`read_goldilocks_into`), so it does not allocate an
intermediate converted copy.

## Explicit fold budget

`ZKIE_FOLD_BUDGET_MB` (default 32768) bounds the projected folding buffers of
one opening. The projection charges 64 bytes per table row: the streamed
base-field block plus EF `W_t` and value buffer, with equality-scratch
headroom. Weight batches and pooled aux groups are opened sequentially
(`parallel_batches=1`, `parallel_groups=1`), so the peak is the per-opening
budget, not `N x groups`. An over-budget opening fails closed (no proof is
produced).

Metrics per weight batch: claims, unique tables, copied source bytes, allocated
buffer bytes, moved bytes, fold projection, batch tables, budget, and parallel
count. Phase timings and RSS are printed separately, together with
`/proc/self/io` and minor/major fault deltas.

## W_t prefix reuse

Claims of one unique table often share a coordinate prefix. The 64-expert
GatherMatMul weight claims have the same inner `(n, k)` point for every expert;
only the trailing expert bits differ. For such a group:

- the MLE claim value folds the shared prefix once and evaluates each claim on
  the small residual table;
- `W_t` is built as `eq(shared_prefix) (x) sum_i gamma^i eq(suffix_i)`, one
  `O(2^d)` pass instead of one full equality vector per claim.

Both transforms use the same exact field operations and produce the same EF
values; the verifier's formula and transcript are unchanged. Groups with no
shared prefix keep the previous per-claim path.

## Evidence

Cold-page-cache S7 full-64-expert segmented example (real layer-10 weights,
`DS_LAYER=10`): 48 committed weight batches in total (16 for the gate weight,
16 up + 16 down for the FFN). Every batch carries 64 global claims and one
unique chunk table.

| run | batches | claims/batch | unique/batch | mat+eval | restore | open | stage total | stage max RSS |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| HEAD cold | 48 | 64 | 1 | 740.9 s | 7.8 s | 985.3 s | 1734.0 s | 10.48 GB |
| streaming cold | 48 | 64 | 1 | 739.4 s | 7.8 s | 1059.8 s | 1807.0 s | 6.32 GB |
| streaming + prefix reuse cold | 48 | 64 | 1 | 13.3 s | 7.8 s | 332.8 s | 353.9 s | 6.28 GB |

Whole-run metering (cold page cache; `fincore_before=0` for all three weight
files and the 64 MiB table):

| run | wall | maxrss | minor faults | major faults | read bytes |
|---|---:|---:|---:|---:|---:|
| HEAD cold | 1848.3 s | 11,461,680 KiB | 4,904,378 | 24,719 | 3,292,844,032 |
| streaming cold | 1923.9 s | 11,461,128 KiB | 4,907,097 | 24,610 | 3,292,835,840 |
| streaming + prefix reuse cold | 470.2 s | 11,461,052 KiB | 969,706 | 24,610 | 3,292,975,104 |

First gate-weight batch (one `2^24` chunk, 64 claims) decomposition under
`ZKIE_TRACE_PROVE`:

| phase | HEAD | streaming | streaming + prefix reuse |
|---|---:|---:|---:|
| chunk copy / materialize | 0.79 s first batch (inside `mat`, cold parent) | 0.072 s | 0.044 s |
| claim MLE evaluation | 15.37 s (inside `mat`) | 15.37 s | 0.242 s |
| PCS prover-data restore | 0.18 s | 0.178 s | 0.178 s |
| `W_t` generation | inside `open` | 15.62 s | 0.193 s |
| value EF conversion | inside `open` | 0.045 s | 0.044 s |
| batch sumcheck | inside `open` | 0.606 s | 0.613 s |
| WHIR opening | inside `open` | 7.495 s | 7.507 s |
| total (mat+eval+restore+open) | 38.7 s | 39.4 s | 8.82 s |

The trace shows that, for the split `2^24` chunks with 64 claims, `W_t`
construction plus the per-claim MLE evaluations are the dominant cost (31.0 s
of 39.4 s), not the WHIR opening. Across the whole cold streaming run the same
split is 733.8 s of `W_t` plus 735.9 s of claim evaluation against a 1807.0 s
weight stage (81%), so the reuse was required rather than a documentation-only
conclusion. With reuse the whole stage falls to 353.9 s: `W_t` 7.9 s, claim
evaluation 11.2 s, WHIR 292.1 s, sumcheck 30.5 s, value conversion 2.2 s.
Claim values and `W_t` are mathematically unchanged.

Per proof segment (cold):

| segment | HEAD | streaming | streaming + prefix reuse |
|---|---:|---:|---:|
| B gate (16 batches), commit+prove | 595.2 s | 619.3 s | 132.2 s |
| D up/down (32 batches), commit+prove | 1240.6 s | 1291.8 s | 325.3 s |
| proof bytes B / D | 3,081,547 / 6,582,852 | same | same |
| verify B / D | 181.7 / 400.0 ms | 182.2 / 390.2 ms | 181.0 / 394.4 ms |
| weight-stage peak RSS B / D | 3.90 / 10.48 GB | 1.81 / 6.32 GB | 1.77 / 6.28 GB |

The streaming-only cold run is ~4% slower in wall time than HEAD in this
window, but its weight-stage peak RSS drops from 10.48 GB to 6.32 GB. The
prefix-reuse run keeps that memory reduction and removes the compute
bottleneck. Whole-run maxrss is unchanged because it is set by the offline
weight commit before proving; the O5 stage peak is measured inside the proof.

Minor faults fall from 4.90 M (HEAD) to 0.97 M (prefix reuse) because the
per-claim full `eq` vectors and per-claim EF conversion buffers are no longer
allocated. Major faults and read bytes are unchanged (24,610 / 3.293 GB),
i.e. the cold weight read is not hidden by the change.

`O5-metrics` for every batch reports `claims=64 unique=1 copied_bytes=134217728
tables=1` (plus the fold projection and budget); the full run therefore copies
48 x 128 MiB = 6 GiB of chunk data. HEAD additionally retained 1 GiB (gate) and
2 GiB (up/down) parent buffers for the duration of each proof; those are gone
in the streaming runs.

## Tests

- `cargo test --release -p zkie-ops --lib` (test machine): 233 passed, 0 failed.
- `cargo test --release -p zkie-core --lib` (full suite, including the 32-bit
  PoW production-parameter test): 91 passed, 0 failed, 1 ignored, 832.6 s.
- New tests: `ef_eval_shared_prefix_matches_per_point` (shared-prefix,
  no-prefix, and identical-point cases against per-point `ef_eval`),
  `shared_prefix_weights_match_direct` (partial prefix, all-identical, and
  no-prefix fallback against the direct `W_t` sum), and
  `read_goldilocks_into_matches_window_read` (in-place mmap conversion).
- S7 full-64 harness: `S7-CHAIN-OK`, all A/B/D/C verifiers accept, all
  tamper/root negatives reject.
