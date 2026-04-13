# MIR

Status: seed draft

MIR lowers high-level semantics into more explicit control flow and runtime operations.

Must support:
- explicit branches and loops
- normalized call forms
- explicit temporary values
- allocation-relevant operations
- shape-aware operations

Invariant:
- MIR should be easy to verify and optimize without reintroducing AST-shaped complexity.
