# S7c implementation plan — Scatter ↔ router selector, top-k, uniqueness, gate, real layer

Status of S7 at this commit: **incomplete**. Implemented and reviewed-in-stage:

- GatherMatMul: expert weights + `ExpertRouting` counts (S7a).
- Gather: authenticated one-hot `GatherRouting` token matrix; verifier opens
  committed columns only at transcript-derived points (S7b batch 4).
- Linear index ops (CombineIdx/IdxBroadcast/PadIdx) with exact shapes,
  u32-domain contract and replay/certification rules (S7b batches 1–3).
- **Fail-closed**: Scatter, TopKSelect, Softmax, BatchedMatMul,
  BroadcastScale, ScaleVec, and every other op without a reverse reduction.
  A `Scatter` graph is refused (`s7c_scatter_still_fail_closed`); the
  gate-weighted tail (BroadcastScale) is refused
  (`s7b_gate_weighted_tail_fail_closed`).

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

REQUIRED before re-wiring TopKSelect (security review of 978f3f8):
1. Prove thr_b is the row broadcast of the authenticated thr column for the
   WHOLE column (eq-weighted broadcast sumcheck anchored at the thr column's
   endpoints), or reduce the d1/d2 relation endpoints directly to the
   authenticated row thr. The current all-zero column check is insufficient.
2. Deterministic tie rule identical to `argmax_forward`/forward topk (first
   index on ties), with a malicious-witness regression (x = [5,5], k = 1).
3. Prove the threshold is ATTAINED by a selected position (thr is the k-th
   largest), e.g. an aux `att` column with att·(1−sel)=0, att·(x−thr)=0 and
   Σ att = 1, plus the x=[10,0], k=1 counterexample.
4. Pre-commit the TopK witness root as an aux descriptor bound in
   `execution_seeds` BEFORE any output point is derived (no adaptive column
   collisions).

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
- Counts/multiset identity: for random `q` over the (token, expert) domain and
  `z` over positions,
  `Σ_pos R'(pos)·X'(pos)·eq(z,pos) = (Σ_pos eq(z,pos)) · sel~(q)`, where
  `R'(pos) = Σ_t eq(q_t,t)·R[t,pos]`, `X'` likewise. `R'`, `X'` are prover
  witness vectors; endpoints are aux openings at the sumcheck point; the RHS
  uses `sel~(q)` from the TopKSelect claim. This binds the routing multiset to
  the authenticated selector exactly.
- Consequences: active-pair uniqueness follows from sel Boolean; `k` from the
  TopKSelect statement gives exactly-k routes; dropped routes fail the
  row-sum/counts identity; duplicates cannot occur in a Boolean selector.
- Outputs token_id/expert_of remain claims whose points are checked against
  the committed routing columns by aux openings (no table read).

### 3. Gate consistency chain

`idx_gate = token_id·n + expert_of` via `CombineIdx` (linear; already
supported), `gate_pos = gate_table[idx_gate]` via `Lookup` (already
supported; table root from the model), `weighted = d·gate_pos` via
`ScaleVec`/`BroadcastScale` (see 4), and `Gather` uses `gate_pos`. The chain
ties the gate used by the scatter-add to the router's `gate` column at the
same `(token_id, expert_of)` pair: `gate_pos` claim → Lookup → idx_gate →
CombineIdx → token_id/expert_of claims → Scatter routing → TopKSelect sel,
with `gate` from the same TopKSelect reduction.

### 4. Multiplicative tail reductions (ScaleVec / BroadcastScale)

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
