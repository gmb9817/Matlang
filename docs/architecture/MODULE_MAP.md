# Module Map

## Frontend

Path: `src/frontend`

Owns:
- lexing
- parsing
- AST
- source locations
- syntax diagnostics

Must not own:
- runtime behavior
- optimization policy
- builtin execution

## Resolver

Path: `src/resolver`

Owns:
- symbol lookup
- package/path resolution
- import handling
- function/script resolution scaffolding

Must not own:
- low-level array semantics
- code generation details

## Semantics

Path: `src/semantics`

Owns:
- workspace model
- binding validation
- type/shape effect annotations
- semantic diagnostics

Must not own:
- backend-specific lowering

## IR

Path: `src/ir`

Owns:
- HIR/MIR/LIR data models
- pass framework
- verifiers
- serialization/dump formats

Must not own:
- parser grammar
- runtime allocation internals beyond modeled operations

## Optimizer

Path: `src/optimizer`

Owns:
- IR-level transformations
- canonicalization
- performance-oriented lowering improvements

Must not own:
- semantic rule invention

## Runtime

Path: `src/runtime`

Owns:
- value model
- memory model
- dense arrays
- strings/chars
- structs/cells
- errors
- invocation machinery

Must not own:
- parser or syntax structure

## Standard Library

Path: `src/stdlib`

Owns:
- library-level builtins
- math/matrix helpers
- strings/filesystem/time helpers

Must not own:
- low-level memory policy

## Execution

Path: `src/execution`

Owns:
- interpreter
- bytecode VM
- JIT and AOT orchestration

Must not own:
- primary value semantics separate from runtime

## Codegen

Path: `src/codegen`

Owns:
- backend emission
- linking/packaging support
- target-specific lowering glue

Must not own:
- front-end syntax policy

## Interop

Path: `src/interop`

Owns:
- MAT-file support
- FFI
- MEX-compat policy and implementation

Must not own:
- general runtime ownership outside interop boundaries

## CLI

Path: `src/cli`

Owns:
- user-facing commands
- configuration
- compile/run orchestration

Must not own:
- core compiler semantics
