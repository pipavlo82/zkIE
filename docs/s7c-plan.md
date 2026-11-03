# S7c implementation plan — Scatter ↔ router selector, top-k, uniqueness, gate, real layer

Status of S7 at this commit: **incomplete**. Implemented and reviewed-in-stage:

- GatherMatMul: expert weights + `ExpertRouting` counts (S7a).
- Gather: authenticated one-hot `GatherRouting` token matrix; verifier opens
  committed columns only at transcript-derived points (S7b batch 4).
- Linear index ops (CombineIdx/IdxBroadcast/PadIdx) with exact shapes,
  u32-domain contract and replay/certification rules (S7b batches 1–3).
- **TopKSelect**: wired with pre-committed witness/thr aux, whole-column
  broadcast, attainment and a strict-tie fail-closed boundary (see status
  below; the deterministic smaller-index tie rule is still pending).
- **Fail-closed**: Scatter, Softmax, BatchedMatMul, BroadcastScale, ScaleVec,
  and every other op without a reverse reduction. A `Scatter` graph is refused
  (`s7c_scatter_still_fail_closed`); the gate-weighted tail (BroadcastScale)
  is refused (`s7b_gate_weighted_tail_fail_closed`).

The public-index-replay prototype was removed (review: it requires the
selector/routing to be public and recomputed by the verifier, which the gist
forbids). Everything below keeps the routing virtual and authenticated by
proof reductions.

## Dependency order

### 1. TopKSelect reverse reduction (router authentication) — first

`Op::TopKSelect { x, sel, thr, gate, d1, d2, m, n, k }`. Relations (from
`topk.rs`, same S5 pattern as the Argmax reduction):

- `sel ∈ {0,1}` and `Σ_e sel[t,e] = k` per row (eq-weighted booleanity + row
  sum virtuals, claim k).
- `gate = x·sel` (eq-weighted whole-column virtual; endpoints gate/x/sel).
- `d1 = sel·(x − thr)`, `d2 = (1−sel)·(thr − x)` with `thr` row-broadcast
  (eq-weighted virtuals, as in `verify_reverse_argmax`'s d1/d2).
- `d1, d2 ≥ 0` ranges (d1 aux bit decomposition; d2 range decomposition).
- Returned claims: `x` at the virtual endpoints; outputs `sel/thr/gate/d1/d2`
  authenticated as seeds/downstream claims.

This makes `sel` a *claimed, authenticated* column — the anchor Scatter needs.
No verifier read of the selector table: only virtual endpoints and aux
openings at derived points.

Implementation status (review of 978f3f8):
1. DONE: `TopKSelectWitness`/`TopKSelectThr` aux batches; the eq-weighted
   identity Σ eq(q,pos)·thr_b(pos) = thr~(q_hi) has claim thr~(q_hi) supplied
   by the verified thr-aux opening at q_hi = q[lgn..], proving whole-column
   broadcast. Forged per-cell thr_b witnesses are rejected.
2. STAGED BOUNDARY ONLY: `d2s = (1-sel)(thr_b-x-1)` range-proves unselected
   x < thr, so boundary ties fail closed; the malicious x=[5,5], k=1 selection
   sel=[0,1] is rejected. The FINAL requirement remains the deterministic
   smaller-expert-index tie-break of `topk_forward`, to be generated and
   verified on real tied inputs (positive + negative tests). S7 is not
   complete until this lands.
3. DONE: `att` column (Boolean, one per row, att·(1-sel)=0, att·(x-thr_b)=0)
   proves threshold attainment; with d1/d2s signs this forces thr to be the
   k-th largest. The x=[10,0], k=1, thr=5 counterexample is rejected.
3b. DONE (new P0): the 16-bit field gaps alone do NOT imply the to_i64 signed
   order (A=2^63, B=2^63-1 with A-B=1 mod P). Verifier-authenticated signed
   range proofs (23-bit decomposition of value+2^22) for x and the thr_b
   column, with pre-committed aux bound before seeds, exclude the wrap.
   Contract: x, thr in [-2^22, 2^22); out-of-range fails closed. Real
   DeepSeek extracted activations (layers 1-26, seq=1) give max|logits16| =
   248167 and max Q16 probability 11937, both below 2^22; the real layer
   proof has NOT been run, so no real-model support is claimed.
4. DONE: both TopK aux descriptors/roots are bound in `execution_seeds`
   before output claim points and challenges; the reduction commits nothing
   itself. Regressions assert seed dependence on the witness root.

### 2. Scatter routing ↔ selector reduction

`Op::Scatter { sel, token_id, expert_of, m, n }`, with `sel` carried as the
TopKSelect claim (fail-closed if the producer is unsupported).

- Aux batch `ScatterRouting`: token one-hot matrix `R[t,pos]` (m columns) and
  expert one-hot `X[e,pos]` (n columns) over the padded position domain
  (s = len(token_id), from the manifest exact shape).
- Eq-weighted virtuals (claim 0):
  - booleanity of every R/X column;
  - `Σ_t R[t,pos] = Σ_e X[e,pos] = mask` (mask = 1[pos<s]); padding selects
    nothing;
  - ties `token_id(pos) = Σ_t t·R[t,pos]`, `expert_of(pos) = Σ_e e·X[e,pos]`
    (endpoints become claims on the token/expert columns; the counts side is
    therefore virtual, never read by the verifier).
- Counts/multiset identity (NO position weight): for random
  `q = (q_e, q_t)` over the (token, expert) domain,
  `Σ_pos R'(pos)·X'(pos) = sel~(q_e || q_t)`, where
  `R'(pos) = Σ_t eq(q_t,t)·R[t,pos]`, `X'` likewise. The unweighted sum is
  exactly the MLE of the routing count matrix at `(q_t, q_e)`, so equality
  with the sel claim forces `count == sel` pointwise. Endpoints are aux
  openings at the sumcheck point; the RHS is the returned sel claim, later
  authenticated by the TopKSelect committed column. No `eq(z,pos)` factor and
  no `mask~(z)` division appear.
- Consequences: active-pair uniqueness follows from sel Boolean; `k` from the
  TopKSelect statement gives exactly-k routes; dropped routes fail the
  row-sum/counts identity; duplicates cannot occur in a Boolean selector.
- Outputs token_id/expert_of remain claims whose points are checked against
  the committed routing columns by aux openings (no table read).

### 2a. Scatter reduction protocol (implementation-ready)

`Op::Scatter { sel, token_id, expert_of, m, n }` reads `sel` (field, m*n,
row-major flat index `t*n + e`, e = low bits) and writes the index tensors
`token_id`/`expert_of` (length `s` = number of active pairs, from the trusted
manifest; `scatter_forward` enumerates experts outer / tokens inner). The
verifier never reads the routing arrays.

Aux `ScatterRouting` (pre-committed, like the TopK aux): `m` token one-hot
columns `R_t[pos]` and `n` expert one-hot columns `X_e[pos]` over the padded
position domain (arity `A = max(ceil_log2 s, 5)`), descriptor derived from the
graph + manifest and bound in `execution_seeds` before output points.

Reduction (all eq-weighted, transcript order fixed):

1. Booleanity (claim 0, gamma-combined): `eq*(R_t - R_t^2)` for all m columns
   and `eq*(X_e - X_e^2)` for all n columns.
2. Mask (claim `(g0+g1) * mask~(q2)`, verifier-computable closed form
   `mask~(z) = sum_{pos<s} eq_evals(z)[pos]`): `g0*eq*(sum_t R_t)` +
   `g1*eq*(sum_e X_e) = (g0+g1)*mask~(q2)`. Pins the support to exactly
   `[0, s)` (padding selects nothing) and gives each position exactly one
   token and one expert.
3. Output ties (direct checks at the claim points `z_tok`, `z_exp`, length
   `lb = ceil_log2 s`): `sum_t t*R_t~(pad(z_tok)) == v_tok` and
   `sum_e e*X_e~(pad(z_exp)) == v_exp`. Random points, aux committed before
   them, so the one-hot columns are pinned pointwise to the claimed public /
   downstream token and expert arrays (values in range by Boolean one-hot).
4. Counts/multiset identity (claim `counts_total`, prover-supplied but
   forced by the sumcheck): with `q_t`, `q_e` random and NO position weight,
   `sum_pos R'(pos) * X'(pos) = sel~(q_e || q_t)`, where
   `R'(pos) = sum_t eq(q_t,t) R_t(pos)`, `X'` likewise, implemented as the
   virtual `sum_{t,e} eq_t[t] eq_e[e] * R_t * X_e` with claim
   `counts_total` (the MLE of the count matrix at `(q_t, q_e)`). The verifier
   returns `EvalClaim { sel, q_e || q_t, counts_total }`; no verifier table
   read and no mask division. The claim is then
   authenticated by the producing TopKSelect reduction (its committed sel
   column opened at the same point), which is what rejects duplicated,
   dropped or non-active routes: by Schwartz-Zippel the routing pair multiset
   must equal the authenticated sel support exactly.

Consequences after composition with the TopKSelect reduction: exactly `k`
routes per token; each active `(token, expert)` pair appears exactly once
(positions are distinct, each has a unique pair, and the multiset identity
matches the Boolean sel); duplicate or dropped routes make the counts
identity fail against the TopK sel opening; token/expert values stay in
`[0,m) x [0,n)` by construction. `s` must be a power of two for the outputs
to be public seeds at the bound entry (execution_seeds requires power-of-two
public lengths); otherwise fail closed.

### 2b. Scatter expert-major order proof — IMPLEMENTED

The multiset identity is insufficient (TL review f48fcfd): a permuted routing
array with the same (token, expert) multiset is accepted. The forward contract
requires the active pairs in column-major key order key = e*m + t, and a formal
bound counterexample confirms the pre-fix verifier accepted a swap.

Gadget (compatible with the existing LogUp permutation argument):

1. New pre-committed aux `ScatterOrder` over the padded flat position domain:
   - `k[pos]` = key of the pair at pos (0 on padding), with a B-bit
     decomposition, B = log2(m*n), masked to pos < s. This gives the key range
     [0, m*n) for active positions.
   - `k_next[pos]` = cyclic shift of k: k_next[pos] = k[pos-1] for pos >= 1,
     k_next[0] = k[2^A - 1].
   - `d[pos]` = k_next[pos] - k[pos] - 1, committed, with a masked bit
     decomposition proving d >= 0 for 1 <= pos < s (strict increase on the
     real sequence; padding unconstrained).
   - key-consistency virtual (claim 0): k = m * sum_e e*X_e + sum_t t*R_t,
     endpoints opened from the routing aux; this binds k to the same one-hot
     columns the tie checks bind to the arrays.
   - shift bindings: eq-weighted virtuals k_next - k_shift = 0 over the
     committed columns, where k_shift is the pair-permutation gadget below.

2. Shift via pair-multiset permutation (no shift sumcheck needed): sample gamma,
   build a[p] = k_next[p] + gamma*p and b[q] = k[q] + gamma*((q+1) mod 2^A),
   and prove multiset equality with the LogUp permutation argument
   (prove_permutation/verify_permutation). The deterministic second component
   makes the matching unique, so multiset equality forces k_next[p] = k[p-1]
   pointwise (cyclic). The verifier additionally checks the encodings at the
   fractional leaf points against openings of the committed k/k_next columns
   (the identity-index MLE is a closed form the verifier computes), which binds
   the carried a/b vectors to the commitments (Schwartz-Zippel). This is the
   new primitive to implement and review; it needs aux openings at the
   fractional leaf points but no changes to the sumcheck core.

3. Consequences: the returned pairs are distinct values in [0, m*n), strictly
   increasing, and (by the counts identity + sel Booleanity) have exactly the
   sel support as their multiset. A strictly increasing sequence with that
   multiset is unique and equals scatter_forward's expert-major enumeration.
   Padding positions carry no pair (mask), so the compressed length is s.

Until steps 1-3 are implemented and a bound malicious witness for the swapped
order is rejected, Scatter remains fail-closed (commit 5df8e59). The formal
pre-fix counterexample already reproduced: with m=n=2, sel all ones and the
positions 0/2 swapped, the old bound verifier returned true.

### 3. Gate consistency chain — IMPLEMENTED (bound paths; see tracker)

`idx_gate = token_id·n + expert_of` via `CombineIdx` (linear; already
supported), `gate_pos = gate_table[idx_gate]` via `Lookup` (already
supported; table root from the model), `weighted = d·gate_pos` via
`ScaleVec`/`BroadcastScale` (see 4), and `Gather` uses `gate_pos`. The chain
ties the gate used by the scatter-add to the router's `gate` column at the
same `(token_id, expert_of)` pair: `gate_pos` claim → Lookup → idx_gate →
CombineIdx → token_id/expert_of claims → Scatter routing → TopKSelect sel,
with `gate` from the same TopKSelect reduction.

### 4. Multiplicative tail reductions (ScaleVec / BroadcastScale) — DONE (best effort)

Implemented as exact-product eq-weighted virtuals with S4 same_poly output
binding: BroadcastScale (out[i]=x[i]*scale[i/h]) and ScaleVec shift 0
(out[i]=x[i]*scale[i%s]); ScaleVec shift>0 rounding remains fail-closed.
The gate-consistency chain test is still blocked on the Scatter order proof.


Needed for `weighted = d·gate_pos`. Reverse pattern: eq-weighted product
virtual with claim 0 at a fresh point; the *output* endpoint must be
consistent with the op's output claim — merge the endpoint claim with the
merged output claim via `same_poly` (S4 machinery) so the relation and the
claimed value refer to the same polynomial; return claims on x/scale at the
virtual endpoint. Rounding (ScaleVec) reuses the committed remainder aux
(`ScaleRemainder`).

### 5. Real DeepSeek single layer

Remaining ops in the layer: Softmax, StableSoftmaxIndex/SoftmaxIndex,
LayerNorm/RmsNorm (centered LN), RoPE, Transpose, BatchedMatMul, plus the
S7c pieces above. Real weights are available on the test machine at
`/data/zkIE/models/deepseek-v2-lite/weights/` (492 files). Acceptance: build
layer 0 via the model builder, commit weights, prove/verify the bound
single-layer statement with the public input/output, plus the S7 malicious
regressions (forged counts/table, swapped expert roots, duplicate/dropped
routes, tampered gate) through the bytes entry.

## Acceptance gate

S7 is complete only when, on the real layer, the gist's S7 acceptance list
passes: equal-length forged counts/table, wrong expert roots, swapped
experts, duplicated routes, dropped active pairs, tampered gate all rejected,
and the honest small MoE plus the real DeepSeek single layer both verify.
Until then every incomplete op stays fail-closed and S7 is reported as
incomplete; S8/S9 are not started.
