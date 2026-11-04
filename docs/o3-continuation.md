# O3 continuation record — virtual sumcheck allocation removal

State: `a8900ad` + this O3 change on `fix/soundness-repair` (test machine
`/data/zkie-soundness-fix`). O3 is NOT complete pending TL review.

## Baseline (gist O3)

`prove_virtual`'s round loop built the round polynomial with a parallel
`map` that allocated one `Vec<Goldilocks>` (length `max_deg+1`) per element
pair, `half` times per round, then `reduce`d the vectors. The same pattern
was in `prove_virtual_with_challenges`. A dedicated measurement example
(`crates/zkie-core/examples/o3_virtual_alloc.rs`, global counting allocator,
`n = 2^18`, 3 MLEs, terms of degrees 1/2/3, release) measured the pre-O3
reference copy:

| threads | allocations | allocated bytes | time |
|---|---|---|---|
| 64 | 384,535 | 18.60 MB | 29.63 ms |
| 1 | 262,278 | 14.69 MB | 37.89 ms |

## Implementation

- `map + reduce` → Rayon `fold` with one reused accumulator
  (`Vec<Goldilocks>` of length `max_deg+1`) per worker/folder, then the same
  `reduce`.
- `.with_min_len(half / (threads * 4))` bounds the folder count so the
  optimization is not lost to per-split accumulator allocation on many cores.
- Applied to both `prove_virtual` and `prove_virtual_with_challenges`.
- Field addition is exact and associative; each element's contribution is
  still accumulated into the same per-`k` slots; the round messages are
  absorbed in the same order and the proof bytes are unchanged.

## Measurements after

| threads | allocations | allocated bytes | time |
|---|---|---|---|
| 64 | 5,637 | 6.47 MB | 7.82 ms |
| 1 | 141 | 6.30 MB | 33.93 ms |

At 64 threads: ~68x fewer allocations and ~3.8x faster in the microbenchmark.
The proof is 760 bytes with hash `bafebfd2fedfcd97` for optimized vs
reference and for 1 vs 64 threads (field-exact, thread-count independent).

## Regression

- `virtual_tests` 4/4, including
  `virtual_fold_matches_map_reduce_reference`: round polys, final evals,
  challenges and serialized proof bytes equal to the pre-O3 reference (both
  transcript and fixed-challenge paths).
- `zkie-ops --lib`: 158/0.
- `zkie-core --lib` (release, `--skip whir_prescribed`): 70/0, 1 ignored.
  The pow-32 `whir_prescribed` test exercises the deterministic PoW grind, not
  this sumcheck change; it was green in 836 s on `a8900ad`.
- Real A/B/C proof bytes are identical to the pre-O3 dumps (A 241,874; B
  462,836; C 355,183 B), so O3 changes neither proof bytes nor size.
- Real full64: `claims=64 unique=1 moved_bytes=2,147,483,648 cloned_bytes=0`;
  `CORE-CHAIN-OK`; B proof 510,969 B; verify 154.7 ms. (B prove wall time is
  WHIR-opening dominated and host-load dependent; O3's microbenchmark gain is
  not expected to move it materially.)
- No transcript/authentication/security-relation change; security 90-bit.

## Hygiene

Removed the accidentally vendored OMC session state
(`.omc/state/hud-state.json`, `hud-stdin-cache.json`) and registry metadata
(`.cargo-ok`, `.cargo_vcs_info.json`) from the vendored p3-challenger; the
vendor directory now contains only the upstream source, README/CHANGELOG and
our grind patch. Added `.omc/` to `.gitignore`.

## Remaining

O3 awaits TL review. O4/O6/O7 are not started; S7 is not accepted; S8/S9
remain postponed.
