# GeluIndex big-table reduction (project ②) — continuation record

Plan: `/tmp/tl_plan_deep_o7_and_geluindex.txt` (approved). Step 1 is
implemented; steps 2 (real silu 2^24 table + FFN segment) and 3 (full 64-expert
S7 retry) remain and await TL acceptance of step 1.

## Step 1 — parameterized ReLU + 25-bit GeluIndex domain witness

### ReLU parameterization

`relu_range_terms(gamma, bits)`, `prove_reverse_relu(..., bits, ...)` and
`verify_reverse_relu(..., bits, ...)` now take the bit width. The Relu op
keeps `RELU_BITS = 23`, `RELU_BOUND = 2^22`; its columns, transcript order and
proof bytes are unchanged (verified by the unchanged real A/B/C and full64
bytes and the existing Relu tests). GeluIndex uses `GELU_BITS = 25`,
`GELU_BOUND = 2^24`, i.e. the signed domain `[-2^24, 2^24)`.

### Domain contract

`prove/verify_reverse_gelu_index` now require:

- `table_len` a power of two and `table_len <= 2^24`;
- `offset.unsigned_abs() <= 2^24`;
- input length a power of two with `point.len() == log2(input.len())`;
- per-row `w = x + offset` and `high = w - (table_len - 1)` both inside
  `[-2^24, 2^24)` (the two 25-bit ReLU inputs).

`Relu` itself remains unchanged and still enforces its 23-bit domain.

### GeluIndexXRange aux (precommitted)

One `AuxRole::GeluIndexXRange` aux per GeluIndex op:

- `GELU_XRANGE_BITS = 25` boolean columns over the padded input domain,
  decomposing `x + 2^24` (zero-padding rows encode `0 + 2^24`, i.e. bit 24
  set);
- the descriptor carries `len = input length`, `arity = max(log2 len, 5)` and
  is part of the aux set absorbed by `execution_seeds`, so its commitment is
  fixed before every GeluIndex challenge (TopKSelectXRange pattern);
- `validate_aux_descriptors` includes the exact GeluIndex group specs, and
  the bound prover prepares it after the forward pass. The prover checks the
  committed bits against the actual input (and padding) and fails closed on
  any mismatch. The plain path (no model/aux) builds and commits the same
  witness inside the proof, absorbing the commitment before the range
  challenge, so that path stays sound too; the bound path requires the
  precommitted descriptor (`model.is_some()` + missing aux -> fail closed).

### GeluIndex proof / Fiat–Shamir order

`ReverseGeluIndexProof` additionally carries `range_commitment`, the
`x_range` virtual proof, the opened bit values and the batch opening. Order
inside the reduction:

```
absorb lower_at_output
absorb b"zkie-gelu-range-v1" + range commitment
q = field_vec(arity); gamma = field()
x_range virtual: eq(q,x)*(x + 2^24 - sum_b 2^b bits_b) == 0
                 + gamma-weighted booleanity of every bit (claim 0)
open the 25 bit columns at the range point; match the virtual final evals
upper ReLU (25 bits); absorb its input claims
lower ReLU (25 bits)
return upper/lower x claims + the x claim from the range virtual
```

The range virtual's claim-0 sumcheck is sound because the booleanity terms
vanish on the Boolean cube; the weighted identity forces `x + 2^24` to be the
25-bit integer represented by the committed bits, hence `x ∈ [-2^24, 2^24)`.
The sumcheck final eval at the random point is not required to be zero (the
virtual polynomial is not identically zero as a polynomial), only consistent
with `virtual_eval`; this is the same convention as the other range proofs.

### Wrap attack

The attack is `x = -(2^24+1)` (so `to_i64(x)` is outside the domain) with
`offset = 2^24+1`, making `x + offset mod p = 0` land inside the ReLU domain.
There is no 25-bit decomposition of `x + 2^24 = -1`; `build_gelu_range_aux`
returns None, and a proof built with a witness for a different input is
rejected by the prover's bit-consistency check. On the verifier side the
precommitted bits are opened at the range point and tied to the `x_range`
virtual final evals, so a forged x eval cannot be re-pointed at the wrapped
value; the test tampers the range x eval and verifier rejects.

### Tests and regression

- `zkie-ops --lib`: 167/0.
- New tests:
  - `gelu_index_signed_domain_boundaries_roundtrip`: `±2^24` domain edges,
    `table_len = 2^24`, `offset = 2^24` and their combination on in-range
    inputs.
  - `gelu_index_domain_wrap_and_contract_rejected`: the wrap input (prover
    fail-closed), out-of-domain `-2^24-1` / `2^24` (no 25-bit witness),
    non-power-of-two and `> 2^24` table_len, `|offset| > 2^24` (prover and
    verifier reject), and a forged range x eval (verifier rejects).
  - `gelu_index_big_table_bound_roundtrip_and_oob_rejected`: bound proof with
    `table_len = 2^24`, `offset = 2^23` (the real silu parameters), the
    `GeluIndexXRange` auxiliary present, two proofs byte-identical
    (determinism), and an out-of-domain bound input fails closed.
- Existing `gelu`/`relu` suites green; the existing committed
  Projection→GeluIndex→Lookup→Projection chain test green and now asserts the
  precommitted `GeluIndexXRange` descriptor.
- Real A/B/C: A 241,874 / B 491,978 / C 355,183 bytes, unchanged; full64
  `CORE-CHAIN-OK`, B 540,239, verify 154.7 ms. These proofs contain no
  GeluIndex reduction (the segmented FFN's GeluIndex at 2^24 is still only
  forward-evaluated in step 1), and the 23-bit Relu path is byte-identical.
- Deterministic PoW and 90-bit security unchanged.

## Remaining

Step 2: wire the real silu 2^24 table (`silu_v` in the segmented example) into
the proved FFN segment using the new reduction; Step 3: full 64-expert S7
retry + negative suite + docs. S7 is not accepted; S8/S9 stay postponed.
Deep O7 (a)–(d) is accepted; project ② step 1 is submitted for TL review.
