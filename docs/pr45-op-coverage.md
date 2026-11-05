# PR#45 B1 — four-model op coverage inventory (current head)

Date: 2026-11-05. Scope: the real graph builders in `models/*/src/lib.rs` and
the bound verifier's reverse whitelist (`reverse_op_io`, compose.rs). This is
the evidence requested before wiring the five missing reverse reductions. It
does not claim model-level acceptance: B2 (statement migration + independent
verifier) and B3 (single-statement MoE) remain open.

## 1. Op use by the four model builders

Counts are `Op::` occurrences in each model crate (builders; examples repeat
only a subset).

| op | GPT-2 | Gemma3 | DeepSeek-V2-Lite | TimesFM | reverse whitelist |
|---|---|---|---|---|---|
| Add | 6 | 3 | 6 | 9 | yes |
| MatMul | 3 | 3 | 7 | 2 | yes |
| Relu | 0 | 0 | 0 | 1 | yes |
| Transpose | 1 | 1 | 2 | 1 | yes |
| Projection | 6 | 7 | 13 | 12 | yes |
| Lookup | 2 | 2 | 19 | 2 | yes |
| GeluIndex | 1 | 1 | 6 | 2 | yes |
| Scale | 4 | 3 | 23 | 3 | yes |
| ScaleVec | 0 | 1 | 6 | 1 | yes |
| Argmax | 1 | 1 | 1 | 0 | yes |
| TopKSelect | 0 | 0 | 11 | 0 | yes |
| Scatter | 0 | 0 | 6 | 0 | yes |
| BroadcastScale | 0 | 0 | 14 | 0 | yes |
| Gather | 0 | 0 | 6 | 0 | yes |
| GatherMatMul | 0 | 0 | 16 | 0 | yes |
| CombineIdx | 0 | 0 | 4 | 0 | yes |
| IdxBroadcast | 0 | 0 | 9 | 0 | yes |
| PadIdx | 0 | 0 | 13 | 0 | yes |
| **Softmax** | 1 | 1 | 2 | 1 | **no (fail-closed)** |
| **StableSoftmaxIndex** | 1 | 1 | 2 | 1 | **no (fail-closed)** |
| SoftmaxIndex | 0 | 0 | 0 | 0 | no (not used) |
| **RmsNorm** | 0 | 7 | 4 | 1 | **no (fail-closed)** |
| **RoPE** | 0 | 2 | 2 | 0 | **no (fail-closed)** |
| **ScaleGate** | 0 | 0 | 0* | 0 | **no (fail-closed)** |
| LayerNormCentered | 3 | 0 | 0 | 1 | **no (fail-closed)** |
| Layernorm | 0 | 0 | 0 | 0 | no (not used) |
| BatchedMatMul | 0 | 0 | 0 | 0 | no (not used) |

\* ScaleGate is not emitted by the current DeepSeek builder; it is retained
in `Op`/`OpProof::ScaleGate` for the router-gate path that B3 needs. The B1
priority still includes it because B3's single-statement MoE chain requires
`Softmax → ScaleGate` router normalization.

## 2. Status matrix

Columns: **code** = an op-level proof module/OpProof exists; **bound** = the
bound any-op verifier accepts it today; **real model** = a full real-model
statement through the bound entry (B2/B3, not yet).

| op | code | bound entry | real model layer | action |
|---|---|---|---|---|
| Add / MatMul / Transpose / Projection / Lookup / GeluIndex / Scale / ScaleVec | yes | yes | no (B2) | — |
| Relu | yes | yes | TimesFM (B2) | — |
| Argmax / TopKSelect / Scatter / BroadcastScale / Gather / GatherMatMul / CombineIdx / IdxBroadcast / PadIdx | yes | yes | DeepSeek segmented (B3 for single statement) | — |
| Softmax | softmax_scaled.rs | **no** | all four models | B1 priority 1 |
| ScaleGate | OpProof::ScaleGate | **no** | B3 router | B1 priority 1 |
| StableSoftmaxIndex | softmax_scaled.rs | **no** | all four models | B1 priority 2 |
| RmsNorm | rms_norm.rs | **no** | Gemma3/DeepSeek/TimesFM | B1 priority 2 |
| RoPE | rope.rs | **no** | Gemma3/DeepSeek | B1 priority 2 |
| LayerNormCentered | layer_norm_centered.rs | **no** | GPT-2/TimesFM | out of the five-op scope; stays fail-closed and is listed as a remaining support gap |
| SoftmaxIndex / Layernorm / BatchedMatMul | some | no | none of the four | out of scope; fail-closed |

## 3. Implementation constraints for B1

- New reductions must be EF (claim points/values/terminals), integrate with
  `ReverseReduction`/`ReverseDagProof`, and return verifier-derived claims for
  inputs and aux columns; they must not re-open the legacy base verifier.
- Relations must be aligned with the forward semantics in compose.rs
  (`apply_op`): rounding/shift, masks, index clamping, row normalization and
  endpoints. Tag/label checks alone are insufficient.
- Aux columns are precommitted and descriptor-pinned like the existing
  reductions; endpoints must be authenticated by aux/weight openings.
- Until each op is implemented and tested, `reverse_op_io` must keep
  returning `None` for it (fail-closed). The old verifier stays closed.

## 4. B2/B3 dependencies

- B2 must move GPT-2/Gemma3/DeepSeek examples to `LanguageModel` and run an
  independent verifier process; TimesFM stays `Generic`. The current examples
  are still `Generic` and same-process.
- B3 needs the Softmax/ScaleGate reductions plus a single-statement MoE
  route→TopK→Scatter→expert→gate→Gather chain. This inventory is not B3
  acceptance.
