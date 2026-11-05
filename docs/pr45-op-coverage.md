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
| Softmax | softmax_scaled.rs | yes (`908e00b`) | all four models | done (B1) |
| ScaleGate | OpProof::ScaleGate | yes (`c47ed80`) | B3 router | done (B1) |
| StableSoftmaxIndex | softmax_scaled.rs | yes (`74088e4`) | all four models | done (B1) |
| RmsNorm | rms_norm.rs | yes (`990b709`) | Gemma3/DeepSeek/TimesFM | done (B1) |
| RoPE | rope.rs | yes (`2346cf3`) | Gemma3/DeepSeek | done (B1) |
| LayerNormCentered | layer_norm_centered.rs | **no** | GPT-2/TimesFM | out of the five-op scope; stays fail-closed and is listed as a remaining support gap |
| SoftmaxIndex / Layernorm / BatchedMatMul | some | no | none of the four | out of scope; fail-closed |

## 2b. Softmax row-sum window: S <= 2^17

The reverse Softmax normalization is

    out(i,j) = round(e(i,j) * 2^B / S(i)),     S(i) = sum_j e(i,j),

and the proof commits `rem = e*2^B - out*S + 2^(B-1)` with a B-bit Boolean
decomposition, enforcing `rem in [0, 2^B)` via

    out*S - 2^B*e - rem + 2^(B-1) = 0.

The true rounding error is `e*2^B - round(e*2^B/S)*S`, whose absolute value is
at most `S/2`. The half-offset window `[-2^(B-1), 2^(B-1))` therefore contains
the error exactly when `S <= 2^B`; above that, the honest remainder leaves the
window and the aux preparation fails closed.

`SOFTMAX_SCALE_BITS` was raised from 16 to **17** after the GPT-2 real graph
produced an honest row sum of **86,741 (`op 12, row 2`) > 2^16 = 65,536**.
The new bound is `S <= 2^17 = 131,072`; larger row sums still fail closed.
Uniqueness is unchanged: the 17-bit decomposition plus Booleanity is
canonical, and the window `[2^(B-1)-S/2, 2^(B-1)+S/2)` still admits at most
two consecutive integers for `out`. The forward remains `i64`
(`out = round_div(e*2^16, S)`); only the proof's remainder width and the
rescaling virtual coefficients change. Budget: one extra remainder column per
Softmax op, one extra opening claim, and three extra gamma terms for the added
bit (one reconstruction, two Booleanity); the PCS opening count K is
unchanged.

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

## 5. B1 closure and B2 bundle status

All five priority ops now have bound reverse reductions on
`fix/soundness-repair` (Softmax `908e00b`, ScaleGate `c47ed80`,
StableSoftmaxIndex `74088e4`, RmsNorm `990b709`, RoPE `2346cf3`). Each
reduction commits its witnesses before the transcript challenges, proves the
forward rounding/range/index semantics with EF virtuals, and authenticates
the endpoints against the committed tensors/tables; the old unsafe entries
remain permanently closed.

B2 adds the public bundle + independent verifier contract (`src/bundle.rs`,
`src/bin/verify_bundle.rs`):

- `bundle.bin` carries only the trusted op graph, the public statement, the
  root-only model manifest and the caller-owned execution I/O; `proof.bin`
  carries the bound proof. The verifier process loads exactly these two files
  and nothing else (no weights, store or prover caches). **bundle.bin 不含任何
  权重数据**：权重只以 manifest 中的 commitment root 形式出现，roots 是唯一
  的权重认证源；`verify_bundle_dir` 的 whir 参数是遗留签名占位，verifier
  内部按 `VERIFIER_SECURITY`/各 opening arity 重建 WHIR。
- The token binding accepts both canonical embedding layouts: column-major
  (`d*base + token`, GPT-2's transposed lm_head) and row-major
  (`token*base + d`, Gemma3/DeepSeek embed_tokens). Prompt, response, output
  and embedding-token tampering are rejected, as are model-root changes and
  argmax/tie forgeries (test
  `b2_public_bundle_independent_verifier_and_tamper_matrix`).
- The independent process test covers missing/corrupt artifacts and the
  fail-closed binary contract.

Remaining B2 work for the four real-model examples is the per-model public
execution assembly (input tables, execution outputs) and full-graph proving.
GPT-2/TimesFM still hit the out-of-scope fail-closed `LayerNormCentered` gap;
Gemma3/DeepSeek are fully covered by the five B1 reductions. The per-op
RmsNorm rsqrt table side is O(2^23); cross-op aggregation is the deeper O7/B3
work agreed with the TL.
