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
  - expected MATLAB behavior: `classdef` admits attribute lists (`properties (...)`, `methods (...)`), `events`, `enumeration`, validation blocks, access modifiers, static methods, and broader class metadata.
  - current project behavior: only the first public-instance subset is implemented; those advanced forms produce explicit unsupported diagnostics.
  - reason: the current implementation intentionally starts with the narrow constructor/property/method core before broader object-model fidelity.
  - status: confirmed

- affected feature: object arrays and richer object indexing
  - expected MATLAB behavior: user-defined classes participate in scalar and array object semantics, including array construction, indexing, and method/property behavior across object arrays.
  - current project behavior: only scalar objects are implemented; array-style object behavior is not yet supported.
  - reason: the first classdef slice is built on the current scalar-first runtime/value model.
  - status: confirmed

- affected feature: artifact and bundle execution of class dependencies
  - expected MATLAB behavior: packaged execution artifacts should carry their class dependencies with the same fidelity as direct filesystem execution.
  - current project behavior: source execution and direct bytecode execution support the new class subset, while artifact/bundle class loading still relies on the current source-path-based class loading path rather than a richer packaged class-module format.
  - reason: the first implementation preserves class semantics first and leaves richer packaged-class metadata as follow-on backend work.
  - status: confirmed
