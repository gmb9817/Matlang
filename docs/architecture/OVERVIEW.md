# Architecture Overview

## Mission

Build a modular MATLAB-compatible compiler and runtime with a CLI-first workflow and a clear path to native compilation.

## System Layers

1. Front-end
   - lexing
   - parsing
   - AST
   - diagnostics
2. Resolution and semantics
   - names
   - scopes
   - workspaces
   - shape/type annotations
3. IR pipeline
   - HIR
   - MIR
   - LIR
   - verification
4. Execution and codegen
   - interpreter
   - bytecode VM
   - AOT/native backend
5. Runtime
   - value model
   - arrays
   - memory
   - invocation
   - errors
6. Standard library
   - intrinsics
   - core builtins
   - expansion libraries

## Architectural Rules

- Parser never depends on runtime internals.
- Optimizations run on IR, not AST.
- Compatibility decisions are written before broad implementation.
- Runtime semantics are centralized and shared by interpreter, bytecode, and compiled modes.
- Every module owns its own spec, tasks, and test plan.

## Release 0.1 Scope

Included:
- scripts and functions
- dense numeric arrays
- logical arrays
- strings/chars basic subset
- structs and cell arrays basic subset
- control flow
- multiple returns
- anonymous functions basic subset
- CLI compile/run workflow

Deferred:
- full graphics stack
- full toolbox parity
- Simulink
- advanced MEX compatibility
- GPU/distributed execution
