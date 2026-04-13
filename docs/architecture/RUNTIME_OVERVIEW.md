# Runtime Overview

## Purpose

The runtime defines executable MATLAB semantics. It is not a helper layer; it is a core product boundary.

## Major Runtime Subsystems

1. Value representation
2. Memory management
3. Dense array engine
4. Numeric and complex operations
5. String and char support
6. Struct and cell support
7. Object/function-handle invocation
8. Error, warning, and stack trace reporting
9. File/path/environment support

## Design Direction

- Use a shared runtime across interpreter, bytecode VM, and compiled artifacts.
- Prefer explicit runtime APIs over ad hoc backend-specific helpers.
- Keep array semantics and copy-on-write rules centralized.
- Introduce reflection only after core value and dispatch models stabilize.

## Immediate Runtime Milestones

1. finalize value container strategy
2. finalize array metadata model
3. implement dense array core
4. implement copy-on-write baseline
5. implement errors and stack traces
