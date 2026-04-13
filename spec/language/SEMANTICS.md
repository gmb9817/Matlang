# Semantics

Status: Release 0.1 draft

## Governing Rule

No subsystem may claim MATLAB compatibility for a behavior unless that behavior is defined here or in a referenced specialized spec.

## Execution Model

Release 0.1 treats a program as:
- a script executed in a workspace, or
- a function invoked with positional inputs and outputs

Primary semantic concerns:
- workspace ownership
- value mutation and copy-on-write
- indexing behavior
- name resolution
- builtin precedence

## Workspace Model

### Script Workspace

A script executes in a mutable workspace frame.

Rules:
- variables introduced by assignment become part of the script workspace
- subsequent statements in the same script can observe those variables
- scripts do not declare explicit parameter lists

### Function Workspace

A function executes in its own local workspace frame.

Rules:
- input parameters are bound on function entry
- output parameters are local names whose final values are returned on function exit
- local variables shadow outer names unless global/persistent rules say otherwise

### Nested Functions

Nested functions capture bindings from enclosing function scopes.

Release 0.1 capture policy:
- capture by shared binding identity, not by value copy
- mutations through a shared captured binding are visible to closures sharing that binding

### Anonymous Functions

Anonymous functions capture referenced outer bindings.

Release 0.1 rule:
- captured names bind to the same logical variable identity visible at creation time
- precise runtime representation belongs in `spec/runtime/VALUE_MODEL.md`

## Name Resolution

Resolution priority for an unqualified identifier in expression position:

1. local variable or parameter in current workspace
2. captured variable from enclosing function workspace
3. global or persistent binding if explicitly declared in the active workspace
4. function visible through local file or path resolution
5. builtin function

Rule:
- variable existence in the active workspace can shadow a function/builtin interpretation
- command-form exceptions, if supported, must be specified separately

## Function Resolution

Function invocation semantics must distinguish:
- direct call to a named function
- call through a function handle
- indexing on a variable value that happens to be callable in some future extension

Release 0.1 rule:
- if a bare identifier resolves to a variable in the active workspace, postfix parens bind as indexing/call-on-value semantics only if the value model allows it
- otherwise it resolves as a function call candidate

## Assignment

Assignment target kinds:
- simple variable
- paren-index target
- cell-index target
- field target
- multiple-output binding list

General rule:
- the right-hand side is evaluated before the assignment is committed
- updates to indexed or field targets must honor copy-on-write rules

Multiple assignment:
- extra outputs beyond produced values become empty/default only if that behavior is explicitly specified
- otherwise mismatched arity is a runtime or compile-time error depending on what is knowable

## Indexing

All indexing is 1-based.

### Paren Indexing

Paren indexing on arrays returns array-like values.

Release 0.1 baseline:
- scalar indices select one-based positions
- multi-dimensional indices follow column-major layout
- omitted trailing dimensions behave according to array rank rules defined in the array model

### Cell Indexing

Cell indexing with `{}` extracts contained values.

Rule:
- `{}` dereferences cell contents
- `()` preserves cell container structure

### Linear Indexing

Linear indexing maps the array to column-major linear order.

Rule:
- a single subscript in parens may invoke linear indexing when semantic conditions match MATLAB behavior

### Logical Indexing

Logical indexing selects elements for which the mask is true.

Release 0.1 baseline:
- mask cardinality must be compatible with the indexed shape or linearized extent
- result ordering follows MATLAB-compatible traversal order

### `end` Keyword

Inside indexing expressions, `end` refers to the upper bound of the relevant indexing dimension or linearized extent according to index form.

Rule:
- the parser treats `end` syntactically as a keyword
- semantic lowering rewrites it using container shape knowledge

## Colon Operator

Release 0.1 baseline supports:
- `a:b`
- `a:s:b`

Rule:
- colon semantics produce a numeric range compatible with MATLAB behavior
- whether ranges are materialized eagerly or lazily is a runtime design decision, but observable semantics must match the array model

## Evaluation Order

Minimum Release 0.1 rule:
- explicit subexpressions are evaluated left-to-right unless a more specific rule overrides this for compatibility
- short-circuit operators evaluate the right-hand side only when required
- assignment commits after right-hand side evaluation and target resolution

## Global and Persistent Variables

### Global

Global declarations refer to process-level shared bindings by name.

Rule:
- a name must be declared global in a workspace to access the global binding through that name

### Persistent

Persistent declarations refer to storage retained across function invocations for the defining function.

Rule:
- persistent storage is scoped to the function identity, not individual call frames

## Multiple Returns

Functions may produce multiple results.

Rules:
- the call site determines how many outputs are requested
- builtin and user-defined functions must be able to observe requested output arity when semantics require it
- IR must preserve multiple-result operations explicitly

## Errors and Warnings

Release 0.1 baseline:
- syntax and resolvable semantic failures should be diagnostics before execution
- dynamic errors during indexing, invalid calls, and incompatible assignments become runtime errors
- warning behavior is deferred until a concrete warning model is specified
