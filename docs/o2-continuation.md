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

## TL decisions (2026-11-04)

- Q3 approved: remove the prover-side `verify_reverse_dag_inner` replay; keep
  self-verify time separately accounted.
- Q2: no reduction change for the C gate/index multi-claims (safety-critical,
  small; keep the S4 merges). Route B is limited to the weight path; any
  "redundant same_poly" removal there needs a written redundancy argument and
  the S4 negatives must stay green.
- Q1 conditionally approved: route A only as an isolated path (feature flag or
  separate function); main path unaffected until chosen; strict FS order
  (statement/roots/input claims before challenges; merged endpoint must be the
  exact S4 output; one opening against the correct root); route A must pass
  all existing negatives as an admission test; if chosen, full TL crypto
  review + full regression + real segmented/full64 rerun.

## Q3 implemented

`prove_reverse_dag_inner` now returns `(ReverseDagProof, Vec<EvalClaim>)`; the
bound prover uses its own residual claims as leaves and does not call
`verify_reverse_dag_inner` by default. `ZKIE_O2_SELFVERIFY=1` enables the old
replay only for measurement: it prints the separately-accounted self-verify
time and checks the prover leaves and replay leaves as multisets (exact Debug
strings; mismatch fails closed). No transcript or proof-byte structure change
(the replay was read-only).

Measured on the real A/B/C example (warm cache):

| proof | before (replay) | default (no replay) | optional replay time |
|---|---|---|---|
| C (gate + Gather) | commit+prove 282.2 ms | 275.1 ms | 10.7 ms |
| B (8-expert) | 5.34 s | 5.20 s | 2 × 10.8 ms (B and B2) |
| A | ~0.16 s | unchanged structure | 10.0 ms |

Verify times unchanged (C 15.7 ms, B 29.4 ms); the optional consistency check
(prover leaves == replay leaves) passes; `zkie-ops` 157/0. Logs:
`/data/zkie-soundness-fix/logs/o2-q3-default.log`,
`/data/zkie-soundness-fix/logs/o2-q3-selfverify.log`.

## Route B (weights): redundancy argument and status

The bound path has **no same_poly on weight tensors**: weight claims are reads
of a `GatherMatMul`/`MatMul` weight, never op outputs, so
`prove_reverse_dag_inner` never merges them. `group_reverse_weights` groups all
leaf claims of a weight tensor by table and opens them with one combined-
equality `W_t` batch opening against the single committed root (O5). Each
claim is therefore authenticated by the same committed root's opening, so the
claims are evaluations of one polynomial by construction; a same_poly merge
would be redundant. There is nothing to remove on this path; route B is the
incumbent. The S4 negative suite (wrong boundary, cross-tensor merge, missed
merge then opening) remains green (`zkie-ops` 157/0). The legacy committed
path's cross_binds are retired (the committed verifier permanently rejects)
and are not part of the bound O2 scope.

## Route A measurement (isolated, env-gated)

The route-A experiment is isolated: the example only calls S4 `prove_same_poly`
on the real weight-opening claims when `ZKIE_O2_ROUTE_A_MEASURE=1`; the main
proof path, proof bytes, transcript and verifier are unchanged. A read-only
`BoundExecutionProof::weight_openings()` accessor was added for tooling. This
measures the merge cost only (not a complete route-A protocol; a complete A
path would still need the single opening and all negatives, and is not
implemented because the measurement rejects the route).

Real 8-expert case (table 2^25, 8 real claims from the B proof):

```
O2-routeA-merge claims=8 table_len=33554432 merged_point_len=25 ms=1673 rss_kb=6185444
```

- S4 merge alone: 1,673 ms and +~2.7 GB RSS over the B run's 3.5 GB.
- Current route B for the same claims: one O5 `W_t` batch opening, open_ms
  ≈ 3.1 s, which already contains the combined-equality merge math and the
  single WHIR opening at the sumcheck point.
- Route A would add the 1,673 ms merge on top of a one-point opening (still a
  full-table sum-of-products + WHIR), so A is strictly more work than B for
  the same table.
- Full64 scaling: `prove_same_poly` materializes one `eq_evals` vector per
  claim; 64 claims × 2^28 × 8 B = 128 GiB, plus the table and a 2 GiB W
  buffer. This cannot satisfy the full64 admission condition without a new
  memory-efficient merge design (which would be a separate TL-approved item).

Conclusion: route B (current O5 `W_t` multi-point batch opening, no same_poly
on weights) is selected. Route A is rejected by measurement/scaling, not
chosen, so its full proof-integration and negative suite are not built. The
C gate/index S4 merges stay as-is (Q2: no reduction change; cost is
negligible: len 64/8 tensors).

Status: Q3 committed (`ce6dc60`); route B selected; the route-A measurement
hook is isolated and env-gated. O2 is not complete pending TL acceptance; S7
is not accepted. No transcript/authentication/security change was made by
this comparison step.

## Project-level finding: proof bytes were not byte-reproducible (fixed)

Observed on the same real inputs across separate processes: C 354,735 /
355,119 / 355,247 B; B 464,564 / 462,132 / 461,940 B; A constant 241,874 B
(A has no WHIR openings). Every variant verified. Root cause:
p3-challenger 0.7.0's `DuplexChallenger::grind` used Rayon
`find_map_any`/`find_any`, returning ANY valid proof-of-work witness. The
witness is absorbed into the transcript, so different thread scheduling
selected different valid witnesses and changed the proof bytes. Red test
(`pow_grind_returns_minimal_witness`, upstream crate): witness
4,647,996,289,340,866,574 vs the minimal valid witness 1,109.

Fix: p3-challenger 0.7.0 is vendored at
`crates/zkie-core/vendor/p3-challenger` and patched in the workspace
`[patch.crates-io]`. The PoW grind now uses an adaptive deterministic segment
search: a segment starts at ~4x the expected number of batches
(`2^bits / lanes`), the segment is scanned in parallel and its MINIMUM
matching batch is taken; on a miss the next segment doubles. Segments are
ordered, so the first segment containing a match yields the global smallest
valid witness. Low-pow grinds stay on a tiny segment; 32-bit PoW uses a few
large parallel segments. The set of valid witnesses, the verification
predicate, the transcript order and the security parameters are unchanged;
only the deterministic choice among valid witnesses changes. PoW witnesses
are still checked by the verifier.

Regressions:

- `pow_grind_returns_minimal_witness` (zkie-core): red on the upstream
  parallel `find_any`, green after the ordered search.
- `s7c_bound_proof_bytes_deterministic` (zkie-ops): two proofs of the same
  statement in one process serialize byte-identically.
- Cross-process: the real A/B/C example run twice with
  `ZKIE_O2_DUMP_PREFIX` produced `cmp`-identical A/B/C proof files
  (241,874 / 462,836 / 355,183 B).
- `zkie-ops` 158/0.

Performance note: adaptive segments keep low-pow grinds cheap. On the real B
proof, `commit+prove` moved from 5.20 s to 5.50 s (~6%); C stayed at ~0.28 s.
The pow-32 core WHIR test (upstream ordered-any search: 35 s) is slower under
the deterministic first-match rule and is measured separately; it exercises a
test-only high PoW target, not the production proof parameters. The byte-level
variance of the earlier O2/O5 metrics (±~0.1%) is explained by this PoW
witness nondeterminism; post-fix proof bytes are deterministic. No timing/RSS
number in O5 changes meaning because of the byte fix itself.

S7 is not accepted; O2 is not complete. No security parameter or
authentication relation is changed by this baseline step.
