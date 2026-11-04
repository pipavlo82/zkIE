# S9 step 1 — end-to-end soundness budget inventory

Date: 2026-11-04. Status: **inventory and error-budget derivation only; no
protocol changes, no mitigation selected.** S9 is not accepted. Step 2 (after
TL review) decides between extension-field challenges and independent
repetition, and adds verifier-enforced budget caps plus measured costs.

Scope: the gist S9 item (“WHIR 90-bit ≠ whole-protocol 90-bit”). We enumerate
every reduction in the current bound proof, its field, degree, number of
rounds, batching checks, and repetitions, then derive the overall acceptance
error under the current Goldilocks challenge field. All counts are derived
from the verifier code and the real layer-10 A/B/D/C statement dimensions;
they are charging-rule estimates, not an attack measurement (the gist
explicitly says an exact attack cost must not be declared from field size
alone).

## 1. Fields and what “90-bit” currently covers

- Base field `F = Goldilocks`, `p = 2^64 − 2^32 + 1 ≈ 2^64`.
- WHIR works over the quadratic extension `EF = BinomialExtensionField<F, 2>`,
  `|EF| ≈ 2^128` (`crates/zkie-core/src/pcs/whir.rs`, `type EF`).
- `security_level = 90` is a **per-WHIR-opening** configuration
  (`Whir::try_with_params`, WHIR capacity-bound analysis and PoW). It does
  not include the outer GKR/sum-check/LogUp reductions, and it does not cover
  the union over many openings in one proof.
- `to_ef_point` embeds each base-field coordinate into EF. This changes the
  arithmetic field but adds **no entropy**: a challenge sampled in `F` has at
  most 64 bits of randomness even when consumed as an EF element.

The current verifier policy (S8, `f67b807`) fixes outer WHIR reconstruction to
`VERIFIER_SECURITY = 90`, rejects proof-carried parameters that would lower
it, and caps `num_variables ≤ 30`, `pow_bits ≤ 128`. It does **not** yet
enforce an aggregate error budget over the outer reductions.

## 2. Charging rules

For a non-zero polynomial of total degree `d` over a field of size `q`, one
uniform challenge hits a root with probability at most `d/q`
(Schwartz–Zippel). We charge every randomized check as follows:

| check | charge |
|---|---|
| Sum-check / virtual sum-check, `r` rounds, max degree `d` | `r·d / q` |
| Random linear combination / batching of `N` equations (coefficient γ) | `N / q` |
| Final deterministic identity (transcript replay, point/value equality, zero-denominator test) | `0` |
| WHIR opening at configured security `s` | `2^-s` |
| Merkle commitment collision / hash binding | assumed; not counted (trusted hash assumption) |

The transcript is a Fiat–Shamir random-oracle heuristic: challenges are
treated as independent uniform. This is the standard ROM heuristic used by
the existing S1–S4 arguments; it is not a proof, and the budget below is
conditional on it.

The union bound over all randomized checks in one proof is the sum of the
charges. Over the A/B/D/C statements, the acceptance probability of a false
composite statement is bounded by the sum over statements (each statement is
verified independently).

All outer reductions currently sample in `F`, so `q = p ≈ 2^64`. Only WHIR
openings use `EF` and the configured WHIR security.

## 3. Primitive inventory

Degrees are the maximum total degree of the polynomial represented by each
proof system's `terms`; `a` is the padded multilinear arity (log2 domain).

| # | primitive | field | degree | rounds | batching | charge per call |
|---|---|---|---|---|---|---|
| 1 | `sumcheck` (MatMul `f·h`) | F | 2 | `log2(k)` | — | `2·log2(k)/p` |
| 2 | `verify_virtual` (eq-weighted relation) | F | ≤3 | `a` | — | `3a/p` |
| 3 | `verify_same_poly` | F | 2 | `a+1` | `N` claims | `2(a+1)/p + N/p` |
| 4 | fractional/LogUp layer `li` | F | 3 | `li` | 2 merges × 2 claims | `3li/p + 2·(2(li+1))/p + 4/p` |
| 5 | TopK tie/rank/range virtuals | F | ≤3 | `a` | — | `3a/p` each |
| 6 | Relu/Gelu range decomposition | F | 2 | `a_r` | — | `2a_r/p` each |
| 7 | Scale/ScaleVec remainder virtual | F | ≤3 | `a` | — | `3a/p` |
| 8 | Gather/GatherMatMul routing + sum virtuals | F | 2 | `a` | — | `2a/p` each |
| 9 | deep-O7 group identity virtual | F | 2 | `a_table` | — | `2a_table/p` |
| 10 | batch-opening `W_t` sum-of-products | F | 2 | `d` (point arity) | — | `2d/p` |
| 11 | WHIR opening (aux / weight / table / selector / token) | EF | — | — | — | `2^-90` per opening |
| 12 | Merkle/hash binding | — | — | — | — | assumed |

### 3.1 Exact per-op virtual counts (from the verifier code)

- **TopKSelect** (`verify_reverse_topk`): 15 virtuals at arity `a`:
  `bools, row_sums, gate, d1, d2s, att_sel, att_gap, broadcast,
  x_range, thr_range, tie_zero, tie_bits, tie_rank, tie_row, tie_less`.
  Charge `≤ 15·3a/p` (conservative degree 3; several are degree 2).
- **Scatter** (`verify_reverse_scatter`): 8 virtuals at arity `a`:
  `bools, mask_tok, mask_exp, counts, order_key_cons, order_key_bits,
  order_d_rel, order_asc`. Charge `≤ 8·3a/p`.
- **Gather** (`verify_reverse_gather`): 2 virtuals at `a`
  (`routing, sum`). Charge `4a/p` (degree 2).
- **GatherMatMul** (`verify_reverse_gather_matmul`): MatMul sum-check plus
  `routing` and `sum` virtuals. Charge `2·log2(k)/p + 4a/p`.
- **Projection**: MatMul sum-check plus booleanity virtual at `a`.
  Charge `2·log2(k)/p + 3a/p`.
- **Relu**: two virtuals (`output` at arity `a`, `range` at `a_r`).
  Charge `2a/p + 2a_r/p` (degree 2).
- **GeluIndex**: upper and lower Relu reductions (2× the Relu charge) plus
  the 25-bit `x_range` virtual at `a_r`. Charge
  `4a/p + 7a_r/p` (degree-3 conservative for `x_range`: `4a + 4a_r` from the
  two Relu proofs plus `3a_r`).
- **Scale (shift > 0)**: remainder/booleanity virtual at `a`. Charge `3a/p`.
- **ScaleVec (shift > 0)**: product virtual at `a` (degree 3), booleanity
  is folded into the gamma-weighted terms, plus one `same_poly` bind with
  arity `a+1`. Charge `3a/p + 2(a+1)/p + 2/p`.
- **Lookup** (ungrouped or group member): the witness-side fractional tree
  has depth `t_w`; the table-side tree has depth `t_t`. A fractional tree of
  depth `t` charges
  `F(t) = Σ_{li=0}^{t-1} [3li + 4(li+1) + 4] = 7t(t−1)/2 + 8t`
  field-scaled round checks (the `+4` per layer is the two `same_poly`
  batching checks; the two merges each have two claims).
  Plus the static-group identity at arity `t` (`2t/p`) and the group table
  tree once per static table.
- **Deep-O7 group relation**: the division-free relation check is
  deterministic; the randomized parts are the group identity virtual and the
  shared table fractional tree (already counted above), plus one group-level
  table opening and one aggregate opening (WHIR).
- **Batch opening** (`verify_batch_open_committed`): one sum-of-products
  sum-check of degree 2 over the point arity `d` (`2d/p`) + one WHIR opening.

## 4. Worked example: real layer-10 D proof (full 64-expert FFN)

Dimensions: `s_pad = 8`, `H = 2048`, `MOE_PAD = 2048`, 64 experts; activation
table 2048, real silu table `2^24`; up/down weights `2^28` each; two Lookups
(activation and silu), each idx column length `s_pad·H = 2^14`.

Using the charging rules above:

| component | count | charge (weighted rounds) |
|---|---|---|
| Lookup witness trees (2 × depth 14) | `2·F(14)` | 1,498 |
| Activation table tree (depth 11) | `F(11)` | 473 |
| Silu table tree (depth 24) | `F(24)` | 2,124 |
| Group identities (arity 11 + 24) | `2·11 + 2·24` | 70 |
| Lookup padding virtuals (multiplicity arity 14) | `2 × 3·14` | 84 |
| GatherMatMul up + down (MatMul 11 + 2 virtuals ×3) | 2 × (22 + 6) | 56 |
| Scale16 up + out (arity 14) | 2 × 28 | 56 |
| GeluIndex (2 Relu + x_range; a=14, a_r=19) | 4·14 + 7·19 | 189 |
| ScaleVec shift 16 (arity 14) | `3·14 + 2·15 + 2` | 74 |
| Weight batch openings up/down (arity 28) | 2 × 2·28 | 112 |
| **D total (base field)** | | **≈ 4,736** |

`ε_D^base ≤ 4,736 / p ≈ 2^-51.8`.

Segments A/B/C are smaller (estimated `2^-55` … `2^-57`): A is dominated by
15 TopK virtuals at arity 6 and 8 Scatter virtuals at arity 3; B by the
MatMul sum-check (11 rounds), the GatherMatMul virtuals, and one weight
batch opening at arity 28; C by TopK (arity 5) + Scatter (arity 3) + Gather
(arity 3) + Scale/broadcast virtuals. Summing the four independent statement
acceptance errors gives a conservative whole-chain estimate

```
ε_base(total) ≲ 2^-51.5   (dominated by D)
```

with the caveat that the exact counts should be instrumented in step 2 and
the formula re-run. The important invariant is the shape, not the last digit:
`ε_base ≈ (weighted round count) / 2^64`, and the weighted count is in the
low thousands, so the outer protocol is roughly **2^-52**, not 2^-90.

## 5. WHIR openings and the union bound

Each opening is configured at `security_level = 90`. A proof performs many
openings: per aux descriptor (projection bits, scale remainders, routing,
TopK witness/thr/range/thr-range/tie, scatter routing/order, gather routing,
Gelu range, ScaleVec remainder), per weight batch, per group aggregate/table,
per Argmax selector position, and the token binding. For D the count is
`K ≈ 12–20`; larger models or statements can have more. The union bound over
openings is

```
ε_PCS ≤ K · 2^-90.
```

For `K = 16`, `ε_PCS ≈ 2^-86`; for `K = 256`, `≈ 2^-82`. This is already
below 90 bits even before the outer reductions. To claim an overall 90-bit
PCS bound, the per-opening WHIR security must be set to
`90 + ceil(log2 K_max)`, which increases PoW bits (and some proof work) but
not the outer budget.

## 6. Current overall assessment

```
ε_total ≲ ε_base + ε_PCS + ε_FS/hash
        ≲ 2^-51.5 + 2^-85 + assumed hash term
        ≈ 2^-51.5
```

The bottleneck is the **base-field outer reductions** (sum-check, virtual
relations, LogUp, tie/range, batch-opening sum-check): the weighted round
count is about `2^12.2`, so `64 − 12.2 ≈ 51.8` bits, i.e. about 2^-52.
WHIR's 90-bit extension
field setting cannot lift this; increasing WHIR PoW cannot compensate for an
unrelated outer error (gist S9). Therefore, **the current stack is not an
end-to-end 90-bit argument**, even though the PCS is configured at 90-bit.
This is a budget statement, not a demonstrated forgery.

## 7. Verifier-enforced parameter policy (current and proposed)

Currently enforced (S8 `f67b807`):

- WHIR reconstruction security fixed at 90 for aux/weight paths; proof-carried
  WHIR parameters (`WhirParams`) are policy-gated and cannot lower it.
- `num_variables ≤ 30`, `security_level ∈ [min, 256]`, `pow_bits ≤ 128`,
  `pow_bits ≥ required_pow_bits(num_variables, 90)`.
- Proof byte cap 512 MiB, bounded decode, aux/step caps, checked dimension
  arithmetic.

Proposed for step 2 (not implemented here):

- Aggregate budget cap: `W = Σ_i (r_i·d_i + c_i) ≤ 2^26` per proof (and per
  statement), checked by the verifier from the verified op dimensions and aux
  specs. Under EF challenges this gives `≤ 2^-102`; under the current base
  field it caps the damage at `≤ 2^-38`.
- Opening-count cap `K ≤ 2^12`, with per-opening WHIR security
  `90 + ceil(log2 K_max)` when a whole-protocol 90-bit claim is desired.
- Keep S8 caps/limits as the resource envelope; the budget cap is an
  additional security parameter, not a replacement.

## 8. Mitigation options for step 2 (analysis only — no decision)

### Option A: move outer challenges to EF

Change the outer sum-check/virtual/same_poly/LogUp/tie/range challenges (and
the batching γ) to be sampled in `EF` while tensors and evaluations remain in
`F`. `to_ef_point` already exists at the PCS boundary; MLE evaluation over `F`
at an EF point is a linear combination of base-field values with EF weights.
Charging uses `|EF| ≈ 2^128`:

```
ε_base ≈ W / 2^128,  W ≈ 2^12 → ≈ 2^-116.
```

with the proposed `W ≤ 2^26` cap giving `≥ 2^-102`. Interfaces affected:
sumcheck round polynomials and final evaluations (EF), same_poly merged point
and value (EF), LogUp α/β/γ and leaf claims (EF), transcript absorbs EF
elements (2 base-field lanes for a quadratic extension), and the point
conversion at every PCS call. Tensors/commitments can stay in `F`.

Costs to measure in step 2: prove/verify wall time, proof size for outer
round messages/final evals (roughly 2× those fields), and transcript
sensitivity. WHIR proof sizes dominate total bytes, so the relative size
increase should be small; arithmetic cost is expected to be a small multiple
(quadratic-extension multiplication is ~3 base multiplications).

### Option B: independent repetition

Run the full outer proof `k` times with independent Fiat–Shamir transcripts
(i.e. independent verifier randomness), and require all `k` instances to
accept. Under independence, a false statement with per-run error `ε` has
overall error `ε^k`:

```
ε ≈ 2^-52, k=2 → ≈ 2^-104.
```

This doubles proof size, prover time, and verifier time for the outer parts
(WHIR openings can be shared only if they are statement-identical; sharing
would reintroduce correlated randomness). It does not require changing the
field interfaces. The gist’s warning applies: duplicating bytes or reusing
the same transcript is not independent repetition and proves nothing.

### Option C: combination / hybrid

If step 2 wants a margin at lower arithmetic cost, EF challenges for the
high-count reductions (fractional/LogUp/range trees) plus independent
repetition for the remaining scalar checks is possible, but must be specified
as one budget with independent randomness, not “repeat twice”.

PoW cannot replace either option: it is a PCS grinding cost, not a remedy for
the outer reduction error.

## 9. Adjacent S8 observation (out of S9 scope)

The expanded S8 malformed-proof fuzz (commit `06dedb5`, tests only) showed
that a small number of single-byte mutations are still accepted. The affected
bytes identified so far are proof data the verifier deliberately does not
consume in these paths:

- `SamePolyProof.proof.final_evals` — the verifier derives the endpoint weight
  in closed form and never reads these stored evals (S4 design). This matches
  the repeated small offsets inside the reverse steps of the O7 fixture.
- `BoundExecutionProof.token_binding` — ignored for `PublicStatement::Generic`
  (it is checked only for `LanguageModel`); this matches the large accepted
  block at the end of the serialized `Generic` proof.

This is **non-canonical proof encoding (malleability), not an extra
acceptance path**: no verified relation depends on those bytes. It is recorded
here so step 2/TL can decide whether to canonicalize/reject dead fields in a
separate hardening item; it does not change the budget above.

## 9b. Step 2a status — EF core layer (committed separately)

The TL approved Option A and the step-2 hardening list. Step 2a introduces the
extension-field outer layer without changing any existing proof/transcript
bytes:

- `crates/zkie-core/src/common/ef_outer.rs`: EF sum-check (degree-2 round
  polynomials, `f·h` terminal), EF virtual sum-check (degree ≤3 terms,
  `rounds: Vec<Vec<Ef>>`, `final_evals: Vec<Ef>`), EF `same_poly` (gamma and
  merged point/value in EF), EF equality-polynomial helpers, and
  `mle_eval_at_ef` (base-field MLE folded with EF weights).
- `Transcript::absorb_ef` absorbs each EF element as its two base-field lanes;
  `Transcript::ef_field` samples two independent base squeezes. The
  `ChallengeSource` trait gained default `ef_field`/`ef_field_vec` so the test
  PRNG remains usable.
- EF `verify_virtual` now binds the carried `final_evals` against the
  replayed final claim (`virtual_eval(terms, final_evals) == final_claim`),
  closing the same dead-field malleability on the EF path.
- Tests: 6/6 (`sumcheck_ef_roundtrip_and_tamper`,
  `virtual_ef_roundtrip_and_tamper`, `same_poly_ef_roundtrip_and_tamper`,
  `ef_mle_eval_matches_base_on_subfield_points`, `ef_wire_encoding_is_two_base_lanes`,
  `transcript_ef_absorption_is_deterministic_and_bound`). Existing ops suite
  175/0; core suite re-run for the step baseline.
- Not yet wired into composers: the base-field sum-check/virtual/same_poly
  remain the production path until step 2b/2c. No proof bytes changed.

## 9c. Step 2b-1 status — EF MatMul / W_t / LogUp core

Step 2b is split into sub-steps because the composer migration touches every
outer reduction. Sub-step 2b-1 adds the remaining EF algebra cores, still
without changing any production path or proof byte:

- `common/ef_matmul.rs`: EF MatMul sum-check (`claimed`, round messages and
  terminals in EF; A/B/C tensors stay F, restricted with
  `partial_eval_at_ef`; C evaluated with `mle_eval_at_ef`).
- `common/ef_batch.rs`: EF sum-of-products sum-check for the batch-opening
  `W_t` arithmetic (weights/values EF; `b_eval` EF).
- `common/ef_logup.rs`: EF fractional/LogUp chain with EF α/β/γ and leaf
  claims; EF witness/table/permutation column builders; EF leaf helper;
  split witness/table identity and zero-root-denominator rejection.
- `ef_outer.rs`: `prove_sumcheck_ef`, `partial_eval_at_ef`, and
  `prove_same_poly_ef` (same-poly over EF-valued fraction columns).

Tests: 10/10 EF (`ef_` filter; adds MatMul, batch sum-of-products, EF LogUp
roundtrip/tamper, lookup split identity + zero-den rejection). Existing ops
175/0 and the base-field protocols are untouched; no proof bytes changed.
Composer wiring, EF PCS boundary/batch-opening wrapper, budget caps,
dead-field canonicalization, and A/B/D/C measurements remain for 2b-2/2b-3.

## 9d. Step 2b-2a status — EF PCS boundary and batch opening

Sub-step 2b-2a adds the EF end of the committed-data boundary; tensors and
Merkle roots stay base-field:

- `Whir::open_ef` / `verify_ef`: open/verify a base-field committed MLE at an
  EF challenge point (LSB→MSB coordinate reversal, no extra randomness). The
  returned evaluation is EF and equals `mle_eval_at_ef` of the F table.
- `Whir::open_batch_multi_ef` / `verify_batch_multi_ef`: the same for a batch
  of tables opened at one EF point.
- `pcs/ef_batch_open.rs`: `EfBatchOpeningProof` plus
  `prove/verify_batch_open_committed_ef`. Fiat–Shamir absorbs the commitment
  and EF points/claims as two-lane values, γ is an EF squeeze, the
  sum-of-products `W_t` sumcheck is EF (`ef_batch`), the WHIR opening runs at
  the EF challenges, and the verifier reconstructs `Σ W_g(r)·eval_g` in EF.
- Test: honest roundtrip, duplicate-table grouping, EF open matches EF MLE
  evaluation, tampered sumcheck/evals rejected.

Existing base-field paths remain untouched; no proof bytes changed. Composer
claim/reverse migration, budget caps, dead-field canonicalization and A/B/D/C
measurements remain for 2b-2b/2b-2c.

## 9e. Step 2b-2b status — composer reverse layer on EF

Sub-step 2b-2b migrates the composer's outer claim/reduction layer to EF.
The committed data plane is unchanged: tensors, Merkle commitments, table
contents and public execution values stay in the base field.

- `EfClaim` (tensor id, EF point, EF value) replaces `EvalClaim` across
  `reverse_setup` / `prove_reverse_dag` / `verify_reverse_dag` and every
  per-op reverse prover/verifier (Gather, TopKSelect, BroadcastScale,
  ScaleVec[Shifted], Scatter, GeluIndex, Scale, Argmax, GatherMatMul,
  Lookup, Projection, Relu, and the reduction steps).
- `ReverseReduction` scalar variants (Add, CombineIdx, IdxBroadcast,
  PadIdx) and all reverse proof structs carry EF points, claimed values and
  sumcheck terminals. Round messages use the EF cores from 2a/2b-1;
  `common/ef_batched.rs` adds the per-position batched MatMul reduction and
  `StreamedWeightsEf` streams the folded weight matrix in EF.
- The LogUp fractional chain (α, β, γ, per-layer claims and leaf values),
  tie/rank/signed-range/remainder relations, and the O7 deep weight groups
  are EF throughout.
- `execution_seeds` lifts each public F tensor by evaluating it at the EF
  challenge point (`mle_eval_at_ef`), producing EF claims; transcript
  absorption of EF values uses the two-lane encoding from 2a.
- PCS boundary: aux, weight-batch, selector and token openings all use
  `prove/verify_batch_open_committed_ef`. Openings are checked against the
  original base-field commitments at the EF challenge points, so EF proofs
  cannot re-point a base commitment; the verifier never trusts an EF value
  without replaying the corresponding opening.
- The legacy committed-DAG verifier was already permanently closed; its
  dead inner verifier is deleted in this step. The legacy prover keeps a
  base-field token-evaluation helper because its output is never accepted
  (the byte entry rejects before decoding).
- The S8 verifier caps run unchanged before any EF challenge derivation.
  All S1–S8 negative and fuzz tests pass unchanged, including the
  `s8_*_reject_without_panic` wire-mutation tests.

Verification: `cargo test -p zkie-core --release` 88 passed / 1 ignored;
`cargo test -p zkie-ops --release` 175 passed (all S1–S8 negatives included);
`cargo check --workspace --release` clean. Verifier budget caps, dead-field
canonicalization and the real A/B/D/C before/after measurements remain for
2b-2c/2b-2d.

## 9f. Step 2b-2c status — verifier caps and dead-field canonicalization

Sub-step 2c makes the S9 budget verifier-enforced and removes the remaining
wire malleability.

- `ProofBudget` walks the verified proof structure and accumulates an upper
  bound on `W = Σ(r·d + c)`: every EF virtual/sumcheck round is charged at
  degree 3, same-poly and batch sum-of-products at degree 2, fractional trees
  per layer (including both merges and their batching), permutations on both
  sides, and each opening's sum-of-products rounds. `verify_committed_shard_
  dag_bound` rejects `W > 2^26` and `openings > 2^12` before any challenging
  or PCS work beyond the structural caps.
- Per-opening security: the protocol's WHIR instances are provisioned at
  `VERIFIER_SECURITY = 96` (= 90 + ceil(log2 64)); the verifier additionally
  requires `security ≥ 90 + ceil(log2 K)` for the proof's actual opening
  count `K`. A proof therefore needs `K ≤ 64` under the current provisioning;
  the `2^12` hard cap bounds decoder/PCS work, and statements with more
  openings fail closed instead of degrading below 90 bits. Raising the
  provisioned constant to 102 would cover the full 2^12 cap at higher PoW.
- All protocol-side WHIR constructions (aux, weight commit, table, selector,
  token) use the same 96-bit constant; the weight-cache filename includes
  the security so a 90-bit cache can never be reused with a 96-bit manifest.
- Dead-field canonicalization:
  - base `SamePolyProof` stored final evaluations are not consulted by the
    verifier; the prover now emits them as zero and `verify_same_poly`
    rejects any non-zero value. The prover-side `fractional_leaf` helper
    recovers LogUp leaf values from the merge sum-check and the recorded
    challenge vector instead of the dead field.
  - under a `Generic` statement the token binding is never consulted; the
    prover omits it and the verifier rejects any proof that carries one.
  - EF `EfSamePolyProof` needs no change: `ef_outer::verify_virtual` already
    binds `final_evals` into the checked virtual identity, so the field is
    consumed rather than dead.

Verification: `cargo test -p zkie-ops --release` 176 passed / 0 failed (all
S1–S8 negatives and S8 fuzz included with the caps active; the new
`s9_budget_caps_cover_honest_fixture` asserts the honest fixture is inside
both caps and inside the provisioned security); `cargo test -p
zkie-core --release` 88 passed / 1 ignored, plus the `same_poly` suite
10/10 including the new `s9_nonzero_final_evals_rejected`;
`cargo check --workspace --release` clean. Because WHIR security changed
90→96, all proof bytes change; the new baseline and EF cost measurements
are step 2d.

## 10. Acceptance status

Step 1 delivers only this inventory and budget. The S9 acceptance items

- auditable end-to-end ≥90-bit parameter basis,
- verifier-enforced parameter caps, and
- measured extension-field cost

are **not yet met** by the current base-field protocol. Step 2, after TL
review, will choose Option A or B (or a specified hybrid), implement the
verifier checks and the size/round caps, measure prove/verify/size costs on
the real A/B/D/C chain, and then submit the end-to-end argument for review.

No protocol, transcript, or proof-byte behavior was changed by this step.
