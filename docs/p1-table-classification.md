# P1 table classification (A: public function tables / B: weight-derived tables)

Status: design inventory for TL review, 2026-11-06. No code changes in this
document. Authority for mechanism/path choices: local FlashProve fork
`/Users/shakku/trustless-ai/FlashProve`, HEAD `289dd85f` (not the network
copy). FlashProve file references below are to that fork.

## 0. TL decisions recorded

1. **Gelu function table -> A (load file + digest, A10).** It is a public
   function table in the same class as FP's SiLU/gate. The offline fixed-point
   artifact uses the second path. If a public bit-exact generator spec is
   published later, it can move to the first path (compute in verifier).
2. **rsqrt -> A, load file + digest** (second path). FP has no rsqrt table;
   FP explicitly permits "takes them as data (what then fixes them: R15.3)".
   Computing it in the verifier remains an option after
   `scripts/extract_rsqrt_piecewise.py` is declared a public bit-exact spec.
3. **A-table digest source -> model-distribution sidecar** (`tables.meta`,
   parameter-file style). The model distribution ships the sidecar and table
   files; the caller loads them into the manifest A-registry. The prover never
   writes a digest. The manifest inside the bundle is a caller copy, not the
   only source; the ultimate anchor is the model distribution.
4. **RoPE cos/sin -> A, load file + digest.** They move from execution inputs
   to A-registry table binding: RoPE claims are evaluated against the
   digest-verified table values. FP's `ropeC` compute path (first path) is a
   possible follow-up, not the initial implementation.

Additional standing rules: A tables are never supplied with a prover-carried
hash; B tables keep their existing commitments; dynamic op-written tables keep
producer-claim authentication.

## 1. Full table inventory

Registry paths in zkIE (`crates/zkie-ops/src/compose.rs`):
`identity_tables` (closed-form range tables), `lookup_roots` (individual
static table commitments), `static_table_commits` (M4a once-per-proof static
commit), `static_lookup_groups`/`LookupGroups` (static `Op::Lookup` tables),
`op_field_writes` (dynamic op-written tables), and `model.batches`
(`commit_model_weights` weight commitments).

| # | table / purpose | source op (models) | current registration / authentication | class | suggested path | reason / FlashProve reference |
|---|---|---|---|---|---|---|
| A1 | Softmax exp table (`e = exp_table[idx]`, padded 2^21 in GPT-2/Gemma3/DeepSeek; model-specific in TimesFM) | `Op::Softmax.table` (GPT-2, TimesFM, Gemma3, DeepSeek-V2-Lite) | `lookup_roots` single-table root + `static_table_commits` (M4a) + internal per-table batch opening | A | **compute in verifier + digest** | Public exp function. FP `verifier/src/vf_attention_shape.h:206` builds `Tt` with `nearbyint(ldexp(1,S)*exp(-u/2^rx))`; `verifier/src/cp_verify.cpp:2010` hashes `AF[0].Tt` with `earlier_hash_h_digest(hh,72,...)` -> `h.hTexp` -> `tblHeld(3,"table-digest-exp")`; FORMAT R15.3 |
| A2 | StableSoftmaxIndex "exp" table | `Op::StableSoftmaxIndex` has only `table_len`; the exp values come from the following `Op::Softmax.table` | No table of its own; reuses A1 | A | **same as A1** | SSI computes index/clamp/relu only; FP's softmax table is the same public table path |
| A3 | RmsNorm rsqrt table (7,340,032 entries -> padded 2^23) | `Op::RmsNorm.rsqrt_table` (Gemma3, DeepSeek, TimesFM) | `lookup_roots` + M4a static commit + internal per-table batch opening | A | **load file + digest** (decision 2) | FP has no rsqrt table; FP's second path "takes them as data (R15.3)" applies. zkIE generator: `scripts/extract_rsqrt_piecewise.py` (piecewise f64 `1/sqrt`, `np.round` ties-to-even, scale 2^16) |
| A4 | LayerNormCentered rsqrt table | `Op::LayerNormCentered.rsqrt_table` (GPT-2, TimesFM) | `lookup_roots` + M4a | A | **load file + digest** | Same as A3 |
| A5 | legacy Layernorm rsqrt table | `Op::Layernorm.rsqrt_table` (legacy/tests) | `lookup_roots` | A | **load file + digest** | Same as A3 |
| A6 | RoPE cos/sin tables | `Op::RoPE.cos/sin` (Gemma3, DeepSeek, TimesFM RoPE) | Currently execution inputs: reverse claims are checked against caller execution values; not in `lookup_roots`/`static_table_commits` | A | **load file + digest** (decision 4), claims evaluate against verified table values | FP computes rotary tables (`cp_verify.cpp:1609 ropeC`) and hashes them into `h.hRope` (`:1426`, `tblHeld(2,"table-digest-rope")`); FP's compute path is a later option |
| A7 | RoPE range table | `Op::RoPE.range_table` (models pass `limb_t`, 2^16 identity) | `identity_tables`, no commitment/opening | A | **compute (closed form, row t=t)** | 11-1 *Public tables*: "A range table has no listed rows: row t is the number t" |
| A8 | RmsNorm limb table | `Op::RmsNorm.limb_table` (2^16 identity) | `identity_tables` | A | **compute (closed form)** | 11-1 ranges of 16 bits |
| A9 | SoftmaxIndex/GeluIndex/Relu/Scale/ScaleVec range tables | `Op::SoftmaxIndex.table_len`, `Op::GeluIndex.table_len`, `Op::Relu` (2^22), `Op::Scale/ScaleVec` (2^shift) | `ensure_public_tables`/`identity_tables` (+2^22 identity tables for SoftmaxIndex/GeluIndex) | A | **compute (closed form)** | 11-1 ranges; row t=t; no digest needed |
| A10 | Gelu function table (2^24) | `Op::Lookup.table = gelu_t` (GPT-2, Gemma3) | `lookup_roots` + `LookupGroups` | A | **load file + digest** (decision 1) | Public function table; FP gate/SiLU is the same class but zkIE's tanh-GELU has no FP counterpart; offline generator `scripts/extract_gpt2.py` (0.5x(1+tanh(sqrt(2/pi)(x+0.044715x^3))), scale 2^16). Future: public bit-exact spec -> compute path |
| A11 | SiLU function table (TimesFM 2^20; DeepSeek model-specific) | `Op::Lookup.table = silu_t` (DeepSeek, TimesFM) | `lookup_roots` + `LookupGroups` | A | **compute in verifier + digest** | FP `gateTableEntry` (`cp_verify.cpp:202`, double round) + hashed `h.hTs` (`:1418`, `tblHeld(0,"table-digest-silu")`); FORMAT R15.3 |
| A12 | Sigmoid table (reserved; no current zkIE model) | future `Op::Lookup.table` | — | A | **compute in verifier + digest** if added | FP `sigmoidTableEntry` (`cp_verify.cpp:183`) + `h.hSig` (`:1426`); R15.3 |
| B1 | Embedding / wte table | `Op::Lookup.table`: GPT-2/Gemma3 `wte_owned`, DeepSeek `embed_tokens` clone | `lookup_roots` + `LookupGroups` | **B** | keep committed | Weight-derived; FP 11-5 committed matrix, embedding kind 12 |
| B2 | LM head / output head table | GPT-2 `lm_head` used as Lookup table; Gemma3/DeepSeek head weights | `lookup_roots`/`LookupGroups`; or `model.batches` for MatMul b | **B** | keep committed | FP 11-5 output head kind 13 |
| B3 | Dense weights: MatMul b, Projection w/bias, BatchedMatMul w, RmsNorm w, LNC w/b, Layernorm w/b | dense/attention/FFN ops | `commit_model_weights` -> `model.batches` + weight batch opening | **B** | keep committed | FP 11-5 committed matrix W units |
| B4 | Expert/router weights: GatherMatMul/BatchedMatMul `eg/eu/ed`, router | DeepSeek/Gemma3 MoE | weight batches; GatherMatMul expert slices bound by offline manifest | **B** | keep committed | FP 11-5 kinds 7-11 (routed/shared experts, router) |
| B5 | Dynamic op-written tables (activation/routing derived) | `Op::Lookup.table` where `op_field_writes(ops)` contains the id (e.g. DeepSeek `table: x`, `table: gate`) | No `lookup_roots`; dynamic Lookup returns producer `EfClaim`, authenticated by the producing op's committed cells/claims | **B (must remain authenticated)** | keep producer-claim path | Not a public function; derived from committed activations/weights. FP has no "dynamic activation table"; principle is committed cells + claims (11-5 spirit) |

Note: `StableSoftmaxIndex` itself has no exp table; `GeluIndex` itself has only
identity range tables (A9); the gelu function-value table is the following
`Op::Lookup.table` (A10).

## 2. Mechanism conclusions

### 2.1 Two A-table paths (mirroring FP)

**Path 1 - compute in verifier.** The verifier computes the table to the bit
from public mathematics and public parameters, hashes it with the
domain-separated digest (FP uses the second hash function
`K(72; table)`, FORMAT R15.3), stores the digest in verifier-owned header
words (FP `hTs/hSig/hTexp`), and compares it with the parameter/sidecar line
when present (mismatch rejects). Used by A1 (exp), A11 (SiLU), A12 (sigmoid
if ever added); RoPE A6 may move here later.

**Path 2 - load file + digest.** The A table is an offline file. The verifier
loads it, checks length/arity/shape, streams a hash, and compares it with the
digest from `tables.meta` (model distribution sidecar). Missing/mismatched
file rejects before proof verification; table values are cached. Used by
A3-A5 (rsqrt), A10 (Gelu), and A6 (RoPE cos/sin) per decisions 1, 2, 4.

**Range/identity tables** (A7-A9) are computed closed-form (`row t = t`);
they need no file or 32-byte digest (an optional generator-version digest may
be recorded).

### 2.2 Digest channel and independence

- **Sidecar**: each model distribution ships `tables.meta`, parameter-file
  style, with one entry per A table: table id, kind (`compute` or `file`),
  length, arity, file path (for `file`), 32-byte digest (8 x 32-bit words,
  FP style), and hash domain. Model files are distributed with the sidecar.
- **Caller path**: the caller passes the model directory/sidecar to the
  verifier; the verifier loads it into the manifest A-registry. The manifest
  inside `bundle.bin` is a caller copy for reproducibility, **not the only
  source**; the verifier must take digests from the distribution sidecar and
  reject a bundle whose copy disagrees. The prover never writes a digest.
- **Commitment parameter set**: the A digest set enters the configuration
  digest / commitment parameters before challenges, matching FP's chain:
  `earlier_hash_h_digest(...,72,...)` -> verifier-owned header word ->
  parameters/statement -> configuration digest (R4.9) -> commitment ID
  (R4.11). A digest mismatch is fatal; the prover cannot substitute a table
  or a hash.
- **All-or-none / required entries**: `file` entries require a digest and a
  readable table file; `compute` entries may carry a digest line (if present,
  compare; if absent, the verifier's computed table is authoritative and the
  digest is still recorded in the verifier header). A table registry entry
  not justified by the trusted graph rejects.

### 2.3 Verifier: load + hash + async parallel

1. Parse `tables.meta`; validate table ids/kinds/lengths/arities against the
   trusted op graph and manifest; load A-registry.
2. For `file` entries: open the table file under the model directory; check
   byte length/shape; stream-hash; compare digest; cache `digest -> values`.
3. For `compute` entries: compute the table (double-precision formula and
   rounding as specified); hash; compare optional sidecar digest; cache.
4. Hash/load tables concurrently (rayon/task pool) with bounded concurrency
   and memory; fail-closed on missing/short/mismatched/duplicate entries.
5. Absorb the A digest set into the transcript/config before any challenge.
6. Use the cached A table values in the reverse reductions; no A-table PCS
   opening or commitment is read from the proof.
7. Range/identity tables are generated locally (no file I/O).

### 2.4 Proof / manifest / bundle landing points

- `TrustedModelManifest`: add A-table registry/digest fields (serde schema);
  keep `lookup_roots` only for B static tables and other non-A tables.
- `BUNDLE_VERSION`: 4 -> **5**; `verify_bundle_dir` checks the version,
  resolves the model-distribution sidecar, loads/validates A tables, then
  calls the proof verifier with the loaded A tables. Old versions reject.
- Prover/proof paths: A tables lose `lookup_roots`, `static_table_commits`,
  `internal_table_openings`, and `LookupGroups` table openings; M4a
  per-table batch openings remain for B static lookup tables
  (embedding/head) if applicable. `opening_upper_bound` and `proof_budget`
  drop A-table openings; expected K decreases further.
- Tests (future implementation): sidecar parse; missing file; wrong digest;
  wrong length/arity; async determinism; bundle version rejection; A table
  values used in lookups; B commitments unchanged; dynamic claims unchanged.

### 2.5 Security boundaries

- A: trust = digest collision resistance (FlashProve second hash K/TK or
  zkIE's selected domain-separated hash) + the independent model-distribution
  sidecar + the configuration/commitment chain. No prover-supplied hash.
- B: trust = existing PCS commitments (lookup roots, weight batches,
  LookupGroups). B tables never move to digest-only.
- Dynamic B5: trust = producer op's committed cells/claims; do not classify
  as A even if the values happen to look like a function table.

## 3. FlashProve references (local fork 289dd85f)

- `verifier/src/vf_attention_shape.h:206` - softmax exp table (`Tt`).
- `verifier/src/cp_verify.cpp:183` - `sigmoidTableEntry`.
- `verifier/src/cp_verify.cpp:202` - `gateTableEntry` (SiLU/gate).
- `verifier/src/cp_verify.cpp:1418`, `:1426` - table hashing
  (`earlier_hash_h_digest(...,72,...)`; `h.hTs`, `h.hSig`, `h.hRope`).
- `verifier/src/cp_verify.cpp:1609` - RoPE `ropeC`; FE integer recurrence.
- `verifier/src/cp_verify.cpp:1734` - SiLU `tblHeld(0,...)`.
- `verifier/src/cp_verify.cpp:2010` - exp `tblHeld(3,...)`.
- `docs/format/11-1-machinery.md` - *Public tables*: range table row t=t;
  SiLU/sigmoid/rotary/softmax public tables; `tsilu/tsig/trope/texp` digests;
  verifier computes or takes as data and rejects digest mismatch.
- `docs/format/FORMAT.md` - **R15.3** second hash `K(72;table)` / 32-byte
  digest; **R4.9** configuration digest; **R4.11** commitment ID; 12.6
  second hash; 13.4 step 4 / 17.1 item 8-9 / 17.2 item 7 public-table
  handling.
- `docs/format/11-5-weights-in-the-table.md` - committed weight matrix W;
  embedding/head kinds 12/13; expert/router kinds 7-11; weight claims.

## 4. Decision status

Resolved by TL: Gelu (A, load+digest), rsqrt (A, load+digest), digest source
(model-distribution `tables.meta` sidecar), RoPE cos/sin (A, load+digest,
migrated from execution inputs). Open for implementation time: optional
compute paths for rsqrt/Gelu/RoPE after public bit-exact generator specs; the
exact sidecar hash algorithm/domain ids; and any future A-table not listed
here. B tables and dynamic producer claims are unchanged.
