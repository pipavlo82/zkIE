# O2 continuation record — boundary claim merge/open strategy

State: `58fa40e` on `fix/soundness-repair` (test machine
`/data/zkie-soundness-fix`). O2 is NOT complete. This step is the baseline
inventory only; routes A/B change proof/transcript structure and are reported
to the TL before implementation per the task constraint.

## Step 1 — baseline inventory (measured)

Instrumentation (temporary, not committed): `prove_reverse_dag_inner` prints
`O2-dag-merge id/len/claims` for every same-tensor claim merge; the bound
prover prints `O2-reverse` and `O2-selfverify` timings around the reverse DAG
and the prover-side `verify_reverse_dag_inner` replay. All other numbers are
the example's existing prints.

Commands (test machine, warm page cache; same 90-bit parameters as the
examples, `Whir::new_testing(5)`):

```
cd /data/zkie-soundness-fix
export DS_WEIGHTS=/data/zkIE/models/deepseek-v2-lite/weights \
       ZKIE_CACHE=/dev/shm/zkie-core-chain-cache DS_LAYER=10
target/release/examples/prove_core_chain          # A/B/C real segmented
target/release/examples/prove_core_chain_full64   # full 64-expert weight
```

Logs:

- `/data/zkie-soundness-fix/logs/o2-baseline-seg.log`
- `/data/zkie-soundness-fix/logs/o2-baseline-full64.log`

| proof | same-tensor merges | weight opening | W_t claims | reverse DAG | self-verify | prove | verify | proof bytes |
|---|---|---|---|---|---|---|---|---|
| A (routing) | none | none | 0 | 137.5 ms | 9.9 ms | ~0.16 s | — | 241,874 |
| B (8-expert gate projection) | none | 1 batch opening | 8 | 185.3 ms | 10.6 ms | 5.34 s | 30.0 ms | 464,244 |
| C (router-gate + Gather) | 2 | none | 0 | 185.3 ms | 10.6 ms | 0.28 s | 15.4 ms | 355,119 |
| full64 (64-expert gate projection) | none | 1 batch opening | 64 | 1,071.0 ms | 11.9 ms | 151.0 s | 151.7 ms | 512,185 |

Same-tensor multi-claim cases observed:

- **C**: `gate` has two claims — the public execution-output seed and the
  dynamic Lookup table claim (`len=64, claims=2`); the padded token index has
  two claims (`len=8, claims=2`). Both are merged by S4 `same_poly` inside
  `prove_reverse_dag_inner`. These tensors are **not** PCS-opened; the merged
  claim is consumed by the producing op (TopKSelect / PadIdx).
- **Weights (B/full64)**: 8/64 claims on the same weight tensor are NOT
  `same_poly`-merged in the DAG. They are grouped per table by
  `group_reverse_weights` and opened once by the O5 combined-equality
  `W_t(X) = Σ_i γ^i eq(r_i,X)` batch opening. There is no per-claim opening
  and no same_poly on this path.
- **A**: no same-tensor multi-claims.

Other PCS openings: one token-binding aux opening for the first Lookup per
proof (B, C, full64); no Argmax selector openings in these graphs. Aux
witness-column openings inside the reductions are unchanged and not counted
here.

Self-verification: the prover calls `verify_reverse_dag_inner` after
`prove_reverse_dag_inner` to obtain the leaves used for weight grouping;
measured separately above (9.9-11.9 ms; 10.6 ms for C) and never included in
the user-facing verify time.

## Findings

1. The gist's "open each claim separately, then same_poly" pattern does **not**
   exist on the current bound weight path: O5 already batch-opens all points of
   one table in a single `W_t` opening, with no same_poly.
2. The only same-tensor `same_poly` merges are small intermediate tensors
   (C gate len 64, index len 8) whose merged claim is reduced by the producing
   op; they are not opened.
3. The gist's prover self-verification overhead is real but modest in these
   measurements (≈10-12 ms per proof); it is the verifier replay used only to
   collect leaves.

## Proposed route implementations (require TL approval)

Route A (merge then open) for weights — implementable with the existing S4
primitive, but it changes the proof/transcript structure:

- In the weight-opening loop, for each table with >1 claims, merge the claims
  with `prove_same_poly` over the table (S4), producing ONE (r*, v*) endpoint;
  then open the table at r* with one point. The verifier must verify the merge
  proof before the opening at the merged endpoint.
- Requires a new optional `SamePolyProof` field in `WeightBatchOpening` (or a
  new proof variant) and a defined FS absorption order (claims → same_poly
  messages → merged point/value → opening). Authentication relation is the
  same table claims; transcript version changes.
- Expected cost: an extra full-table sumcheck over 2^28 for the 64-expert case,
  on top of the existing `W_t` opening; likely slower than route B. Must be
  measured on the 8-expert and full64 real inputs.

Route B (keep multi-point batch opening, drop redundant same_poly):

- For weights this is already the current implementation (O5 `W_t`, no
  same_poly) — formalize as route B and measure it as the incumbent.
- For the C gate/index multi-claims, removing `same_poly` requires the
  producing reduction (TopKSelect / PadIdx) to consume multiple claims per
  output tensor, or an alternative binding of the public seed to the dynamic
  Lookup table claim. That is a reduction/transcript change; needs TL approval
  and a concrete design before implementation.

No-transcript-change optimization (in scope of the gist's self-verify note):

- `prove_reverse_dag_inner` can return its residual claims (the leaves)
  directly; the prover then skips the `verify_reverse_dag_inner` replay.
  This removes the measured 9.9-11.9 ms per proof without changing the proof
  bytes or transcript. Tests will check prover leaves == verifier leaves.
  Requesting approval to implement this as the first route-B-side commit.

## Open questions for TL

1. Approve the `WeightBatchOpening` + transcript change for route A, to be
   measured (8-expert and full64) but not necessarily chosen?
2. For route B at the C gate/index multi-claims, approve the reduction change
   scope, or keep the small S4 merges (route A) there and limit route B to the
   weight path?
3. Approve the no-transcript self-verify removal as part of O2?

S7 is not accepted; O2 is not complete. No security parameter or
authentication relation is changed by this baseline step.
