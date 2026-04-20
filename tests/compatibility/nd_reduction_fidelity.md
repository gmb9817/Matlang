# N-D Reduction Fidelity

- Source fixture: `tests/fixtures/execution/interpreter/builtin_nd_reduction_helpers.m`
- Target behavior:
  - `sum`, `prod`, `max`, and `min` preserve N-D shape when reducing along higher dimensions.
  - `cumsum`, `cumprod`, `cummax`, and `cummin` accumulate across page dimensions without flattening.
  - logical inputs follow MATLAB-style numeric outputs for `sum`/`cumsum` in the covered subset.
