# ADR-0002: Execution Bring-Up Strategy

## Status
Accepted

## Decision

Execution will be brought up in this order:
1. interpreter
2. bytecode VM
3. native backend

## Why

- interpreter validates semantics fastest
- bytecode creates a stable executable target before native codegen
- native backend can then focus on performance and packaging

## Consequences

- runtime APIs must be shared across all execution modes
- semantics tests should be differential wherever possible
- native compilation is not allowed to invent separate semantics
