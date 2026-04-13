# Value Model

Status: Release 0.1 draft

## Goal

Define the executable value categories visible to the runtime and shared by interpreter, bytecode, and compiled modes.

## Core Value Categories

Release 0.1 must represent:

- numeric scalars
- logical scalars
- dense numeric arrays
- char vectors
- string scalars
- cell arrays
- structs
- function handles
- empty arrays

Deferred:
- sparse arrays
- tables
- categorical arrays
- advanced object system completeness

## Representation Strategy

Current direction:
- use a tagged runtime value container that can reference heap-owned composite values
- allow scalars to remain cheap to pass while arrays and composite values carry shared metadata handles

Why:
- this fits MATLAB-like scalar/array duality
- it supports copy-on-write for arrays and containers
- it gives execution modes a common ABI-like internal contract

## Scalars

Release 0.1 scalar families:
- double-precision real
- double-precision complex
- logical

Deferred:
- integer scalar family details unless specifically introduced by stdlib or syntax expansion

## Arrays

Arrays are heap-owned values with:
- element kind
- rank
- shape
- storage layout metadata
- mutability/copy-on-write metadata

Dense arrays should share one broad representation family even if element kinds differ.

## Empty Values

Release 0.1 requires an explicit empty-array representation.

Rule:
- emptiness is a shape/property question, not an out-of-band null
- an empty value still has a value category and shape metadata

## Char vs String

Release 0.1 distinction:
- single-quoted literals map to char-oriented values
- double-quoted literals map to string scalar values

Runtime rule:
- char vectors and string scalars are distinct value categories even if interop helpers later allow conversions

## Function Handles

Function handle values must be first-class.

Required capabilities:
- refer to named functions
- refer to anonymous functions
- carry capture environment metadata for closures
- be invocable through the common call machinery

## Struct and Cell Values

Struct values:
- own field-name metadata plus field values
- must support scalar struct baseline in Release 0.1

Cell values:
- own indexed containers of arbitrary values
- support `{}` extraction and `()` slicing/container preservation

## Identity and Mutation

Identity-sensitive categories in Release 0.1:
- function handles
- shared heap-owned array/container values through copy-on-write metadata

Mutation rule:
- writes to shared heap-owned values must trigger copy-on-write before mutation if another live reference can observe the old contents

## Ownership

Runtime ownership goals:
- no execution mode should invent a different value lifetime model
- stack-local temporaries may reference heap-owned values
- heap-owned values carry enough metadata for safe cloning/sharing decisions

## Error Values

Errors do not need to be first-class user values in Release 0.1, but the runtime must define an internal error object carrying:
- message
- source location if available
- stack trace frames
- optional identifier
