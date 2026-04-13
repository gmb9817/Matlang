# HIR

Status: seed draft

HIR preserves MATLAB-level semantics.

Must support:
- multiple-result operations
- indexing and assignment forms
- matrix construction
- workspace operations
- dynamic call sites
- diagnostics/source spans

Invariant:
- lowering to HIR must not erase semantics that are still needed for compatibility or diagnostics.
