# Builtins

Status: seed draft

## Classification

Tier 0: intrinsics
- primitive arithmetic
- comparisons
- indexing primitives
- allocation primitives

Tier 1: core builtins
- `size`
- `length`
- `numel`
- `zeros`
- `ones`
- `eye`
- `reshape`
- `permute`
- `sum`
- `prod`
- `min`
- `max`

Tier 2: expansion library
- higher-frequency math
- filesystem helpers
- string helpers
- time/date helpers

## Rule

If a function changes semantics-critical behavior or unlocks core performance guarantees, it should be treated as an intrinsic or runtime-owned builtin rather than a plain library helper.
