# Rust Workspace Topology

## Current Direction

The repository uses a multi-crate Rust workspace aligned to the architectural module map.

## Workspace Members

- `src/frontend`: lexing, parsing, AST, diagnostics
- `src/resolver`: name lookup, package/path resolution
- `src/semantics`: binding validation, workspace model, semantic analysis
- `src/ir`: HIR, MIR, LIR, verifiers
- `src/optimizer`: IR transformations
- `src/runtime`: value model, arrays, memory, invocation, errors
- `src/stdlib`: builtin and library-layer functionality
- `src/execution`: interpreter, bytecode VM, execution orchestration
- `src/codegen`: backend emission and linking support
- `src/interop`: MAT-file, FFI, MEX-compat layers
- `src/platform`: OS/path/environment abstractions
- `src/cli`: user-facing `matc` driver

## Boundary Rules

- `src/cli` may orchestrate but must not own compiler semantics.
- `src/frontend` must not depend on runtime implementation details.
- `src/optimizer` may depend on `src/ir`, but not the reverse.
- `src/execution` must share runtime semantics, not redefine them.
- `src/stdlib` should depend on `src/runtime`, not the other way around.

## Current State

- Crate manifests and placeholder entrypoints exist.
- The local machine does not currently have `cargo` or `rustc`, so the workspace has not been compile-verified yet.
- Once Rust is installed, the first validation step should be `cargo check --workspace`.
