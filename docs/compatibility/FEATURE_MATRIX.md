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
| classdef | limited subset | PARTIAL | `classdef` now supports a first core subset: class files, `handle` inheritance, public instance properties with defaults, public instance methods, scalar objects, constructor calls, dot-call methods, function-form first-arg method dispatch, `@ClassName` folder methods, and `class` / `isa` integration. Advanced attributes, static methods, events, metaclass, user-defined inheritance, and object arrays remain open. |
| MAT-file | read/write subset | PARTIAL | Current MAT-file-backed `save` / `load` subset exists for the implemented runtime surface. |
| MEX compatibility | policy pending | PLANNED | Likely partial/deferred |
| Graphics | plotting/UI stack | PARTIAL | Broad first plotting/export baseline plus native Windows figure viewing exists, but MATLAB UI/property parity does not. |
| Simulink | full model ecosystem | UNPLANNED | Explicitly deferred |
