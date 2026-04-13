# ADR-0001: Primary Implementation Language

## Status
Accepted

## Decision

The primary implementation language is Rust.

## Why

- strong memory safety for a large compiler/runtime codebase
- good performance characteristics
- strong support for CLI tools, libraries, and test infrastructure
- good modularity through crates and packages
- better long-term safety than a large C/C++ codebase for a multi-contributor project

## Consequences

- repository scaffolding should eventually map major subsystems to Rust crates or crate groups
- FFI boundaries must be explicit
- LLVM integration remains optional and can be introduced after the core pipeline stabilizes

## Revisit Conditions

Revisit only if:
- a critical backend requirement cannot be met in Rust
- tooling cost becomes unacceptable
- a future ADR supplies a materially better architecture justification
