# Known Differences

This file tracks any confirmed divergence from MATLAB behavior.

Rules:
- Do not add speculative differences.
- Every entry must include:
  - affected feature
  - expected MATLAB behavior
  - current project behavior
  - reason
  - status

Current status:

- affected feature: `classdef` attributes and advanced blocks
  - expected MATLAB behavior: `classdef` admits broader attribute lists (`properties (...)`, `methods (...)`), `events`, `enumeration`, validation blocks, access modifiers, and broader class metadata.
  - current project behavior: the first public-instance subset, inline `methods (Static)`, a first inline `Access=private` subset for properties/methods/static methods, first source-backed single inheritance, and explicit builtin-only validation for property default expressions are implemented; the remaining advanced forms still produce explicit unsupported diagnostics.
  - reason: the current implementation intentionally starts with the narrow constructor/property/method core before broader object-model fidelity.
  - status: confirmed

- affected feature: object arrays and richer object indexing
  - expected MATLAB behavior: user-defined classes participate in scalar and array object semantics, including array construction, indexing, and method/property behavior across object arrays.
  - current project behavior: a first homogeneous object-matrix baseline now exists for `class`, `isa`, direct property reads, direct/indexed property updates, slice-indexed and logical-mask property updates on homogeneous object matrices, block-concatenated matrix-valued property reads over homogeneous object arrays, concatenated object-array-valued property reads plus indexed updates back into the owning objects, first uniform growth/deletion through aggregated matrix-valued and object-array-valued property projections, outer brace assignment over cell-valued property aggregates like `objs.items{2} = ...`, scalar-object `()` indexing as a 1x1 object-array baseline including `end`, constructor-backed gapped scalar-to-array growth within the homogeneous subset including empty-matrix roots for zero-arg-constructible classes, first method dispatch including bound method handles over homogeneous object matrices, property-produced homogeneous object arrays, and broader indexed receiver syntax like `@objs(:,2).total` and `@objs.duplicate()(3).total`, first property-produced homogeneous object-array slice indexing and chained dispatch, explicit rejection of mixed-class or object/non-object matrix construction, explicit `horzcat` / `vertcat` / `cat` support for homogeneous scalar objects, scalar-object `transpose` / `ctranspose` / `repmat` / `repelem` / `reshape` / `permute` / `ipermute` / `flip` / `circshift` / `pagetranspose` / `pagectranspose` support, matrix-literal concatenation of matrix-valued expressions for the current homogeneous object subset, first property-subindex assignment over homogeneous object-array property results, superclass constructor chaining across nested constructors in the current single-inheritance subset, explicit rejection of assignment through temporary method results like `objs.method().prop = ...`, and MAT/save-load roundtrip, but richer object-array semantics are still missing, especially broader MATLAB parity for array-returning method/property behavior beyond the current homogeneous direct-read/write subset and deeper indexing behaviors beyond the current scalar-object/gapped-growth path.
  - reason: the current classdef work now builds on the generic matrix runtime enough for a first array baseline, but not yet the fuller MATLAB object-array model.
  - status: confirmed

- affected feature: bundled user-defined inheritance
  - expected MATLAB behavior: packaged execution artifacts should preserve inheritance behavior for user-defined class hierarchies with the same fidelity as direct source-backed execution.
  - current project behavior: a first bundled inheritance slice now works for inherited defaults, inherited instance methods, inherited static methods including folder-style inherited static handles, nested superclass constructor chaining, inherited folder-method bound-handle save/load, and `isa(base)` checks, but broader packaged hierarchy coverage still remains open, especially beyond the current single-inheritance / current classdef subset.
  - reason: superclass targets are now carried through bundled class metadata, but the implementation still only covers the current narrow classdef feature surface.
  - status: confirmed
