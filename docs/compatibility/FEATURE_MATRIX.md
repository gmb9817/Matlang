# Feature Matrix

Status legend:
- `UNPLANNED`
- `PLANNED`
- `SPEC_COMPLETE`
- `IN_PROGRESS`
- `IMPLEMENTED`
- `TESTED`
- `COMPATIBLE`
- `PARTIAL`
- `DIVERGENT`

| Area | Scope | Status | Notes |
| --- | --- | --- | --- |
| Scripts | top-level execution | TESTED | Interpreter and bytecode VM both run a meaningful script subset. |
| Functions | local and file-based | TESTED | Local, nested, external, package-qualified, and bundled execution paths exist for the current subset. |
| Nested functions | capture semantics | PARTIAL | First closure/capture/shared-binding baseline exists; sibling/transitive edge cases remain. |
| Anonymous functions | basic subset | PARTIAL | Basic capture and handle execution exists; broader parity still remains. |
| Arrays | dense numeric N-D arrays | PARTIAL | Strong first matrix/shape baseline exists, but exact N-D/runtime fidelity is still open. |
| Logical indexing | baseline behavior | PARTIAL | First-class logical values and logical indexing exist for the current subset. |
| Linear indexing | baseline behavior | PARTIAL | Current read/write/delete baseline exists, but fuller parity still remains. |
| Cell arrays | basic subset | PARTIAL | Cell indexing, assignment, deletion, and current comma-separated-list expansion exist. |
| Structs | basic subset | PARTIAL | Scalar structs, practical struct arrays, nested assignment, and current struct builtins exist. |
| Strings/chars | basic subset | PARTIAL | Char arrays, string scalars, and a first string-array/text-helper baseline exist. |
| classdef | limited subset | PARTIAL | `classdef` now supports a first core subset: class files, package classes, builtin `handle` inheritance, first single inheritance beyond `handle` in both source-backed and first bundled class paths, public instance properties with defaults, first inline `Access=private` coverage for properties and methods, public instance methods, `methods (Static)` for inline static methods, scalar objects, a first homogeneous object-matrix baseline for `class` / `isa` including package-qualified class identities, property reads, indexed property updates, slice-indexed and logical-mask property updates on homogeneous object matrices like `objs(1:2).x = ...` and `objs([true false true]).x = ...`, block-concatenated matrix-valued property reads over homogeneous object arrays, concatenated object-array-valued property reads plus indexed updates back into the owning objects, first uniform growth/deletion through aggregated matrix-valued and object-array-valued property projections, outer brace assignment over cell-valued property aggregates like `objs.items{2} = ...`, scalar-object `()` indexing as a 1x1 object-array baseline including `end`, constructor-backed gapped scalar-to-array growth within the homogeneous subset including empty-matrix roots for zero-arg-constructible classes, explicit rejection of mixed-class or object/non-object matrix construction, explicit `horzcat` / `vertcat` / `cat` support for homogeneous scalar objects, `repmat` and scalar-object `repelem` support for homogeneous arrays, scalar-object `transpose` / `ctranspose` / `reshape` / `permute` / `ipermute` / `flip` / `circshift` / `pagetranspose` / `pagectranspose` support, matrix-literal concatenation of matrix-valued expressions for the current homogeneous object subset (so methods can build larger object arrays with `[obj obj]`), first property-subindex assignment over homogeneous object-array property results like `objs.child(2).x`, first property-produced homogeneous object-array slice indexing like `objs.child(1:2)` plus chained method dispatch, superclass constructor chaining across nested constructors in the current single-inheritance subset, first method dispatch (`objs.method()` and `method(objs)` for the current homogeneous matrix subset) including bound method handles over homogeneous object matrices, property-produced homogeneous object arrays, and broader indexed receiver syntax like `@objs(:,2).total` and `@objs.duplicate()(3).total`, now carried structurally through parser/HIR instead of only runtime reinterpretation, and MAT/save-load roundtrip including the current private-member/runtime-metadata subset plus constructor/static/instance/bound method handles and current handle-alias preservation, including `save -append`, constructor calls, dot-call methods, function-form first-arg method dispatch, `@obj.method` bound-method-handle syntax, class-qualified constructor/instance/static method handles, class-name static dispatch, `@ClassName` and `+pkg/@ClassName` folder methods plus class-folder `private` helper lookup, bundled plain, packaged, and folder-based class dependencies, source-free bundle save/load roundtrip for packaged and inherited folder-method bound handles plus folder-style inherited static dispatch, and `class` / `isa` integration. Broader attributes, events, metaclass, richer bundled hierarchy breadth, richer object-array semantics, and deeper indexing still remain open. |
| MAT-file | read/write subset | PARTIAL | Current MAT-file-backed `save` / `load` subset exists for the implemented runtime surface. |
| MEX compatibility | policy pending | PLANNED | Likely partial/deferred |
| Graphics | plotting/UI stack | PARTIAL | Broad first plotting/export baseline plus native Windows figure viewing exists, but MATLAB UI/property parity does not. |
| Simulink | full model ecosystem | UNPLANNED | Explicitly deferred |
