# Syntax Grammar

Status: Release 0.1 draft

## Goal

Define the parseable source forms for the Release 0.1 MATLAB-compatible subset.

## Compilation Units

Release 0.1 unit kinds:

- script file
- function file

Rules:
- a script file contains top-level executable statements
- a function file begins with a primary function definition
- local functions after script code may be permitted only if explicitly enabled by later semantic policy

## Statements

Release 0.1 statement families:

- assignment
- expression statement
- `if` / `elseif` / `else`
- `for`
- `while`
- `switch` / `case` / `otherwise`
- `break`
- `continue`
- `return`
- `global`
- `persistent`
- function definition

Deferred:
- full `try` / `catch` unless promoted by later implementation milestone

## Expression Precedence

Highest to lowest, approximately:

1. primary expressions
   - literals
   - identifiers
   - parenthesized expressions
   - matrix literals
   - cell literals
2. postfix operators
   - call
   - indexing
   - field access
   - transpose and dot-transpose
3. power
4. unary prefix
   - unary plus
   - unary minus
   - logical not
5. multiplicative
6. additive
7. colon/range
8. relational
9. elementwise/logical non-short-circuit
10. short-circuit logical
11. anonymous function expression where admitted by grammar

The exact parse table must be formalized during parser implementation.

## Primary Expressions

Primary forms:
- identifier
- numeric literal
- char literal
- string literal
- parenthesized expression
- matrix literal
- cell literal
- anonymous function

## Calls and Indexing

Postfix syntax forms:

- function call: `f(a, b)`
- paren indexing: `a(i, j)`
- cell indexing: `c{1, 2}`
- field access: `s.field`
- chained access: `a(1).field{2}`

Grammar rule:
- the parser must preserve whether a postfix form was call syntax or indexing syntax
- semantic analysis decides whether an identifier-target postfix sequence is a function invocation or array indexing of a variable value

## Assignment Forms

Release 0.1 assignment targets:

- identifier
- paren indexing target
- cell indexing target
- field assignment target

Multiple assignment form:
- `[a, b, c] = expr`

Rule:
- left-hand side parse structure must preserve output tuple order and assignment target kind

## Matrix and Cell Construction

Matrix literal:
- `[expr, expr; expr, expr]`

Cell literal:
- `{expr, expr; expr, expr}`

Parser requirements:
- preserve row/column grouping
- preserve explicit separators where needed for diagnostics

## Function Definitions

Release 0.1 supported function forms:

- `function y = f(x)`
- `function [a, b] = f(x, y)`
- `function f(x, y)`

Nested function definitions:
- allowed inside function bodies
- exact scope/capture rules are semantic, not syntactic

## Anonymous Functions

Release 0.1 form:
- `@(x, y) expr`

Rule:
- body is a single expression
- statement-bodied anonymous functions are out of scope

## Command-Form Syntax

Policy:
- command-form parsing is a compatibility-sensitive feature and should be admitted in Release 0.1 only for a minimal, deliberate subset
- until formally locked down, the parser should prefer standard expression/call forms in ambiguous cases

## Control Flow Forms

Required parse support:

- `if condition ... elseif condition ... else ... end`
- `for name = expr ... end`
- `while condition ... end`
- `switch expr ... case expr ... otherwise ... end`

Parser requirement:
- AST must preserve clause boundaries and source spans for each clause head
