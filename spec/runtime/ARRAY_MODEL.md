# Array Model

Status: Release 0.1 draft

## Goal

Define observable array behavior for the Release 0.1 runtime subset.

## Baseline Guarantees

- arrays are 1-based indexed
- arrays are column-major
- dense arrays are the first required storage family
- shape is first-class metadata
- copy-on-write governs observable mutation

## Metadata

Every dense array value must carry:
- element kind
- rank
- shape vector
- total element count
- pointer or handle to storage
- sharing/mutability metadata

Optional internal metadata:
- stride information if useful for views or reshapes
- lazy range metadata if the runtime chooses not to materialize immediately

## Layout

Release 0.1 observable rule:
- linear storage order is column-major

Internal implementation may use optimized helpers, but any observable indexing/traversal result must match column-major semantics.

## Shapes

The array model must represent:
- scalar as a 1x1 array-compatible value or equivalent scalar category with array interoperability
- vectors
- matrices
- N-dimensional arrays
- empty dimensions

Open design choice:
- whether scalars are stored in a specialized inline form while still behaving as 1x1 arrays at the semantic boundary

## Indexing

### Positional Indexing

Multi-subscript indexing:
- consumes one subscript per addressed dimension, with MATLAB-compatible handling for omitted trailing dimensions

### Linear Indexing

Single-subscript indexing may flatten by column-major order.

### Logical Indexing

Logical masks select elements in traversal order consistent with MATLAB behavior.

### Colon

`:` within indexing means full selection of the addressed dimension.

## Reshape and Permute

Reshape rules:
- element count must be preserved
- no observable element reorder occurs

Permute rules:
- dimensions reorder logically
- element access after permutation must match MATLAB-compatible dimension mapping

## Concatenation

Release 0.1 baseline:
- bracket-based concatenation creates new arrays with compatible dimensions
- scalar expansion behavior for concatenation must follow explicit semantics and never be implicit guesswork

## Scalar Expansion

Release 0.1 policy:
- scalar expansion is allowed only where the semantics document or builtin contract explicitly permits it
- the implementation should not silently generalize this into full NumPy-style broadcasting

## Copy-on-Write

The array engine must support shared storage until mutation requires separation.

Trigger examples:
- indexed write through one alias when another alias still references the same storage
- field or cell update that mutates a shared array payload

Non-trigger example:
- read-only indexing or traversal

## Views vs Materialization

Current design direction:
- internal views may be allowed if they are not externally observable as aliasing that violates MATLAB-compatible copy behavior
- public semantics must behave as though values are independent after copy-on-write boundaries

## Lazy Ranges

Open design choice:
- colon-generated ranges may be represented lazily internally for performance
- if so, they must still expose the same size, indexing, and conversion behavior as a materialized array
