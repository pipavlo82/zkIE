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

## Remaining

Steps (b) aggregated M + shared table tree + group relation; (c) shared table
opening; (d) measurements + docs. S7 is not accepted; S8/S9 postponed.
