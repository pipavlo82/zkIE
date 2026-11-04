# O4 continuation record — closed-form broadcast eq in the batched verifier

State: `7597a0a` + this O4 change on `fix/soundness-repair` (test machine
`/data/zkie-soundness-fix`). O4 is NOT complete pending TL review.

## Problem (gist O4)

`batched_matmul::verify` rebuilt the public broadcast selector
`eq_b[pos*K + d] = eq_u[pos]` as an `S x K` vector and evaluated its MLE at
the sumcheck challenge. The column depends only on position, not on the
contraction index `d`, so the expansion and the O(S*K) evaluation are
redundant verifier work.

## Implementation

- For the row-major layout `index = pos*K + d` with power-of-two `S` and `K`,
  the MLE of the broadcast selector at `ch` is
  `eq_u(ch[logK..])` — the low `logK` challenge variables multiply a constant
  with respect to `d` and factor out. `verify` now computes this closed form
  directly; no `S*K` buffer is allocated.
- Verifier guards: non-power-of-two `S`/`K` and a wrong `u` length fail
  closed; the challenge length is checked before slicing. Padding remains the
  caller's responsibility (zeroed/masked `seg` domain), as in the GatherMatMul
  path; coordinates are never mechanically truncated.
- `prove` is unchanged: the prover still materializes the public `eq_b`
  column for the virtual sumcheck. Only the verifier-side public structural
  value is optimized; witness evaluations are untouched.
- No transcript/authentication relation change.

## Measurements

Dedicated example `crates/zkie-core/examples/o4_verify_alloc.rs` (counting
allocator, honest proof, `S=512, K=2048, N=8`, release):

| verifier | result | allocations | allocated bytes | time |
|---|---|---|---|---|
| pre-O4 reference (`S*K` expansion) | ok | 9 | 16.78 MB | 4.364 ms |
| current (closed form) | ok | 8 | 8,488 B | 0.039 ms |

Same verification result and challenges. The allocation count is essentially
unchanged; the large allocation and its O(S*K) evaluation are removed.

Real segmented A/B/C (`S_pad=8, K=2048`, so `S*K=16K`): proof bytes identical
to the pre-O4 dumps (A 241,874; B 462,836; C 355,183 B); verify times ~15.6 ms
(C) and ~29.7 ms (B), dominated by other work. Full64: `CORE-CHAIN-OK`,
B 510,969 B, verify 153.9 ms (pre-O4 154.7 ms), 64 claims / one `W_t` opening.

## Regression

- `o4_tests::broadcast_eq_closed_form_matches_expansion`: closed form vs the
  full expansion for `(S,K) = (1,1), (2,4), (8,16), (32,64), (4,1), (1,8)` at
  random points.
- `o4_tests::verify_rejects_non_power_of_two_shapes`: fail closed for S=3 or
  K=3.
- `streaming` tests 3/3; `zkie-ops --lib` 158/0; `zkie-core --lib` release
  72/0, 1 ignored (pow-32 `whir_prescribed` skipped as grind-only; green on
  `a8900ad`).
- Real A/B/C proofs byte-identical; full64 `CORE-CHAIN-OK`.
- No transcript/security change; proof size unchanged; security 90-bit.

## Remaining

O4 awaits TL review. O6/O7 are not started; S7 is not accepted; S8/S9 remain
postponed.
