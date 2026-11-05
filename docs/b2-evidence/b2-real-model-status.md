# B2 real-model run status (2026-11-05)

## Weights

Extracted weight binaries for Gemma3 and DeepSeek-V2-Lite already exist on the
test machine (`/data/zkIE/models/{gemma3,deepseek-v2-lite}/weights`; 2.7 GiB
and 85 GiB) and were linked into `models/*/weights/`; no extraction/download
was needed. GPT-2/TimesFM weight sets are present as well.

## GPT-2 / TimesFM (documented boundary, as instructed)

Both examples fail closed exactly at the previously declared out-of-scope
`LayerNormCentered` op, before any proof:

- `docs/b2-evidence/gpt2-exit2.log`: exit 2, 13.57 s, peak RSS 5,982,908 KiB.
- `docs/b2-evidence/timesfm-exit2.log`: exit 2 (see log).

No graph change was made.

## Gemma3 / DeepSeek: monolithic B2 proofs are blocked by the 97-bit openings cap

`verify_committed_shard_dag_bound` runs `security_policy_ok` before any
cryptographic check and rejects every proof with
`ProofBudget.openings > MAX_EFFECTIVE_OPENINGS = 64` (A1 policy, deliberately
conservative). A monolithic full-model proof counts one PCS opening per aux
commitment (plus one per weight batch/static table), so the new B1 aux
reductions dominate:

- RmsNorm has 9 openings per op: row, out, rsqrt pairs, rsqrt multiplicity,
  rsqrt table (non-identity), limb-lo pairs, limb-lo multiplicity, limb-hi
  pairs, limb-hi multiplicity.
- Gemma3 has 6 RmsNorm ops per layer x 18 layers (plus a final norm):
  6 x 9 = 54 openings from RmsNorm alone in ONE layer; a full graph adds
  Softmax (up to 6), StableSoftmaxIndex (1), RoPE (2 lookups x 4 openings),
  projections/lookups/weights, far above 64.
- DeepSeek has 4 RmsNorm ops per layer x 27 layers: 36 openings from RmsNorm
  alone per layer; the full graph (Softmax x2, StableSoftmaxIndex x2, top-k /
  scatter / gather matmul aux) is much further above 64.

Consequently a full Gemma3/DeepSeek proof would be generated and then rejected
by the verifier's openings policy with certainty. Launching the prover first
would also be dominated by the accepted per-op rsqrt table cost
(O(RSQRT_TABLE_LEN)=2^23 work and a multiplicity commitment per RmsNorm op,
>100 ops per model), i.e. hours-to-days of proving for a result that cannot
verify.

This is a policy/architecture conflict between monolithic full-model B2
statements and the A1 per-proof openings cap, not an op-level soundness defect.
Decisions needed from TL/boss:

1. Raise `MAX_EFFECTIVE_OPENINGS` for full-model statements and compensate with
   a stricter per-opening security target (e.g. 128-bit) -- policy change.
2. Implement deep cross-op aux/table aggregation (O7-deep) so the monolithic
   proof fits under 64 openings -- large implementation.
3. Define B2 verification over chained subgraph proofs, each under 64
   openings, with the LM contract checked at the chain ends -- contract change;
   S9 segmented core-chain evidence already exists for this mode.

Until one is chosen, the honest status is: B2 bundle/independent-verifier
infrastructure accepted; GPT-2/TimesFM boundary documented; Gemma3/DeepSeek
real-weight monolithic runs blocked by the openings cap and not launched.

## Refresh 2026-11-05 (same_poly output-binding fix program)

Rerun on `fix/soundness-repair` after the eight per-op out-witness fixes
(commits 6b4a4a0..abb1751):

- GPT-2 and TimesFM still fail closed at the out-of-scope
  `LayerNormCentered` op before any proof (logs refreshed:
  `gpt2-exit2.txt`, `timesfm-exit2.txt`). No graph change.
- Gemma3 / DeepSeek monolithic B2 proofs remain blocked by the A1
  `MAX_EFFECTIVE_OPENINGS = 64` policy. The fixes add one out-witness
  opening per affected op and extend RmsNorm's out witness to include the
  signed out limbs + two out-limb lookups: RmsNorm now performs about
  12 PCS openings per op before table openings (row, out, rsqrt pairs/mult,
  rem limb lo/hi pairs/mult, out limb lo/hi pairs/mult), and
  BroadcastScale/ScaleVec/ScaleGate/Softmax/StableSoftmaxIndex/RoPE each
  add one. The cap is therefore reached earlier than the pre-fix 9/op
  RmsNorm count. No full-model prover was launched (it would be rejected
  by policy after hours of proving).
- B2 bundle/independent-verifier infrastructure remains accepted. The
  path to a monolithic real-model bundle is O7-deep M1 (AuxPlan with
  aggregated out-witness commitments and deferred pooled openings).

## Refresh 2026-11-05 (LNC + Softmax 17-bit window)

- LayerNormCentered is now covered by the LNC reverse reduction
  (`docs/lnc-reverse.md`), landing directly on the pooled aux wire. GPT-2 and
  TimesFM `public_execution` now accepts the op (the old exit-2 boundary is
  gone), and both full graphs pass the LNC path.
- The B1 Softmax remainder window is being widened after a full real-graph
  scan. The measured maxima are GPT-2 573,417 (op 142 row 15) and TimesFM
  674,671 (op 328 row 15); `ceil(log2(max)) = 20`, so the applied
  `SOFTMAX_REM_BITS` is 21 (one bit of margin), covering S <= 2^21.
  The values 227,312 (GPT-2 op 12 row 8) and 164,266 (TimesFM op 29 row 4)
  were the first 17-bit failures, not the maxima.
- Caveat recorded in `docs/pr45-op-coverage.md`: the fixed window contains at
  most `ceil(2^B/S)` candidate outputs; with B=21 the "at most two" threshold
  is S >= 2^20, so very small row sums have looser uniqueness than under the
  17-bit window. This is flagged for the TL.
- B2 harness fixes needed to reach these boundaries: use the canonical
  16-token input for `m <= 16` and the real 3-token prompt length.
- Real-weight LNC-only bound proofs over every LayerNormCentered op in the
  two models are run as separate evidence (`models/*/examples/lnc_real_check.rs`),
  so LNC coverage does not depend on the remaining Softmax window.

- LNC audit fixes (TL review of b65eb9e): the verifier now pins the mean and
  var virtual rho final evaluations (CRITICAL), the row witness decomposes
  mean into a bounded signed raw/rem pair so E1 uniquely pins the rounded
  mean (HIGH), and the var budget degree is charged as 4 (LOW). Regression
  `audit_lnc_forged_rho_must_reject` is red with the rho gates disabled and
  green with them enabled.

- Full GPT-2 B2 with the 21-bit Softmax window now passes the Softmax aux
  stage and stops at the pre-existing StableSoftmaxIndex range contract:
  `op 11` has masked=-1,073,800,612, rmax=-7,380, w=-1,071,696,080 and
  `tr_w=1,071,696,080 > 2^22`. TimesFM `op 52` likewise has
  masked=-5,849,165, rmax=1,458,654, w=-5,210,667, tr_w=5,210,667 and
  mdiff=7,307,819, both above 2^22. The mask magnitude exceeds the SSI
  reduction's 22-bit `w/tr_w/diff` bound. This is the next boundary and is
  not changed in this stage.
