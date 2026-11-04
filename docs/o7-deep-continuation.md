# Deep O7 continuation record

Plan: `/tmp/tl_plan_deep_o7_and_geluindex.txt` (approved). Deep O7 is NOT
complete. This record starts with step (a); steps (b)/(c)/(d) follow after TL
review of each step.

## Step (a) — shared lookup-group α/β in the bound transcript

State: `184bf82` + step (a) on `fix/soundness-repair`.

### Grouping (unchanged for dynamic/identity)

- Group key: the trusted static table tensor id. Only `Op::Lookup` tables that
  are NOT written by an op (dynamic) and NOT in `model.identity_tables` are
  grouped; the manifest must carry the table's trusted root and length.
- Different ids are never merged even if the contents coincide. Members are
  the op indices in ascending order; groups are processed in ascending
  trusted-id order (BTreeMap), so prover and verifier derive identical groups.
- Dynamic and identity tables keep their existing per-op paths (including the
  per-op α/β squeeze); plain `prove_reverse_dag`/`verify_reverse_dag` paths
  also keep the fallback.

### Fiat–Shamir order (documented)

`execution_seeds` absorbs the model, execution, aux descriptors and the
statement, then step (a) derives groups on that transcript BEFORE the reverse
DAG. Per group, in ascending id order:

```
absorb  b"zkie-lookup-group-ab-v1"
absorb  table_id (u64)
absorb  padded-table arity (u64; model lookup length, min 32)
absorb  member count (u64)
absorb  each member op index (u64, ascending)
alpha = field(); beta = field()
```

The reverse DAG then proceeds as before. For a grouped Lookup the op still
absorbs `output_index` at its position, but the two per-op `field()` squeezes
for α/β are omitted; the group (α, β) is used directly. Every subsequent
challenge shifts accordingly; per-round challenge discipline and
messages-before-challenges are unchanged. The verifier runs the identical
derivation on its replay transcript (and on the optional self-verify replay).

### Regression and new baseline (deterministic PoW)

- `zkie-ops --lib`: 160/0; `zkie-core --lib` release: 72/0, 1 ignored
  (pow-32 `whir_prescribed` skipped; it is grind-only and green on `a8900ad`).
- Real A/B/C:
  - A unchanged: 241,874 B (no Lookup).
  - C unchanged: 355,183 B (only the dynamic gate table; no static group).
  - B changed: 462,836 -> **463,604 B** (static activation-table Lookup now
    uses the group α/β). C verify 15.6 ms, B verify 29.6 ms; SEGMENT-OK.
- Full64: B changed 510,969 -> **509,305 B**; `CORE-CHAIN-OK`; 64 claims, one
  `W_t` opening; verify 158.3 ms.
- The new baselines are byte-deterministic; all existing negative suites stay
  green. No 90-bit change.

### Scope

Step (a) only restructures α/β. Aggregated multiplicity M, the shared table
fraction tree and group relation, and the shared table opening are steps
(b)/(c). Deep O7 must not be reported complete before those pass.

## Step (b) — aggregated multiplicity, shared table tree, group relation

State: `f874157` + step (b) on `fix/soundness-repair`.

### Aggregated multiplicity aux (precommitted before α/β)

One `LookupMultiplicityAgg` aux column per static group, over the same padded
table domain as the per-op multiplicity columns, with `M[j] = Σ_i m_i[j]`
over the group's member ops. It is a normal aux descriptor, so its commitment
is absorbed by `execution_seeds` together with the model, execution,
statement and every other descriptor BEFORE the group α/β derivation. Group
key/order is the trusted table id (ascending); the descriptor and proof carry
the member op indices and are checked for exact equality on replay. Dynamic
(op-written), identity and plain paths have no aggregate descriptor and keep
their exact previous bytes.

### Fiat–Shamir order (step (b))

```
execution_seeds: model, execution, aux descriptors (incl. M), statement, seeds
derive_lookup_group_ab (step (a)): alpha, beta per group
reverse_setup: op graph + seed claims
for each group in ascending table-id order:
    absorb b"zkie-lookup-group-m-v1", table id, member count, members
    z = field_vec(table arity)
    identity virtual sumcheck: eq(z,x) * (M(x) - sum_i m_i(x)) == 0
    absorb b"zkie-lookup-group-table-v1"
    shared table fraction tree (prove_lookup_table(M), common alpha/beta)
    open M at [identity point, table numerator leaf]
reverse op loop:
    per grouped Lookup: witness tree, padding proof, pairs opening,
      m_i opened at [identity point, padding point], per-member table opening
      at the shared table denominator leaf; common alpha/beta (no per-op squeeze)
after the loop: group relation and final_den checks
```

All absorbing steps are identical on the prover and verifier (and on the
optional self-verify replay). The group identity challenge vector z is
derived after the M/m commitments and α/β, so neither can be adapted to it.

### Group relation (important algebraic correction)

The aggregated LogUp identity is `Σ_i S_i == T`, where `S_i` is witness tree
`i`'s root `w_i.final_num / w_i.final_den` and `T` is the shared table tree
root `t_num / t_den`. The plan wrote the relation as
`(Π_i w_i.final_num)·t_den == t_num·(Π_i w_i.final_den)`; for a group with
k ≥ 2 members that is the product identity `Π_i S_i == T`, which an honest
prover does not satisfy (the table tree sums the per-lookup contributions).
The implementation uses the division-free form of the intended sum identity:

```
(Σ_i w_i.final_num · Π_{j≠i} w_j.final_den) · t_den
    == t_num · Π_j w_j.final_den
```

For k = 1 this is exactly the plan's cross-product. All witness `final_den` and
the table `final_den` must be non-zero (S3). This deviation is flagged for TL
review; it is required for honest k ≥ 2 groups to verify and for the aggregate
to prove the intended multiset equality.

### Padding and active-bit discipline

Per-op padding proofs are unchanged: each `m_i` is proven zero on the table
padding. `M = Σ_i m_i` is enforced by the random-point identity above, so M's
padding entries are zero too; M is never allowed to be freely zeroed. The
aggregate table tree sums only the true per-table multiplicities, and the
group relation ties the sum of all witness rows (including each op's active
rows) to that table tree. The aggregate M leaf is also tied to the table tree's
numerator leaf by the aggregate opening, so a forged M cannot steer it.

### Regression and new baseline

- `zkie-ops --lib`: 164/0 (160 + four new deep-O7 negative tests).
- Real A/B/C: A unchanged 241,874 B; C unchanged 355,183 B (only the dynamic
  gate table, no static group); B 463,604 -> **491,978 B**. SEGMENT-OK; the
  second run reproduced the same bytes.
- Full64: B 509,305 -> **540,239 B**; `CORE-CHAIN-OK`; verify 154.0 ms
  (prove 234.8 s). A unchanged 241,874 B.
- Four new negatives: forged aggregate M (sum violation), active-entry
  multiplicity zeroing, cross-table merge, dynamic table mixed into a group.
- Wire compatibility: `ReverseDagProof` keeps its single `steps` field; group
  proofs ride in a trailing sentinel step with no outputs, emitted only when a
  static group exists. `ReverseLookupProof.table` stays a plain
  `FractionalProof` (empty placeholder for grouped ops). Hence A/C and any
  no-static-group proof keep their exact pre-(b) bytes.
- Deterministic PoW and 90-bit security unchanged. The grouped O7 test proves
  the same statement twice and asserts byte-identical proofs; the real B
  segment reproduced 491,978 B on a second run.

### First 2^18 same-table signal (step (d) reworks the harness)

`crates/zkie-ops/examples/o7_lookup.rs` (two 2^18 Lookups, same static
table): default cache and `ZKIE_O7_TABLE_CACHE=0` both give one table commit,
858,517 proof bytes and hash `2d430a14d3c6954c`, ~0.55 s prove, HWM
123,332 / 113,604 kB. The step-(b) grouped proof is larger than the previous
per-op 793,525 B for this two-member group; this is reported as measured,
not as a win. Step (d) will replace this harness with the full before/after
table-tree / opening / scan-byte / time comparison.

### Scope

Step (b) still copies the shared table opening into every member's Lookup
step (one opening computed by the prover but duplicated in the proof and
verified per member). Step (c) collapses this to one group-level opening;
step (d) adds the synthetic ≥2 same-table example and the full before/after
measurements.

Grouping is over the op graph, not over reverse-DAG liveness: every static
Lookup op is a group member, and the relation requires one witness-tree root
per member. A dead static Lookup (its output is never consumed and is not a
final output) therefore fails closed rather than being silently dropped. This
is intentional: dropping it would let its multiplicity contribute to M while
its witness rows were unchecked. Real A/B/C/full64 have no dead static
lookups.

## Remaining

Steps (c) shared table opening; (d) measurements + docs. S7 is not accepted;
S8/S9 postponed.
