# Lexical Grammar

Status: Release 0.1 draft

## Goals

Define the token stream consumed by the parser for the Release 0.1 MATLAB-compatible subset.

## Token Categories

The lexer must emit:

- identifiers
- keywords
- numeric literals
- string literals
- char vector literals
- operators
- delimiters and punctuation
- newline markers
- comments and trivia spans

The lexer must preserve source spans for every emitted token.

## Identifiers

Identifier pattern:
- leading character: ASCII letter or `_`
- trailing characters: ASCII letters, digits, `_`

Release 0.1 rule:
- identifiers are case-sensitive
- builtin lookup may later consult compatibility rules, but lexing does not normalize case

## Keywords

Release 0.1 reserved keywords:

- `if`
- `elseif`
- `else`
- `end`
- `for`
- `while`
- `break`
- `continue`
- `return`
- `switch`
- `case`
- `otherwise`
- `try`
- `catch`
- `global`
- `persistent`
- `function`
- `classdef`
- `properties`
- `methods`

Rule:
- keywords lex as keywords only in canonical keyword spelling
- future compatibility relaxations should be handled intentionally, not incidentally

## Numeric Literals

Release 0.1 literal families:

- decimal integers
- decimal floats
- scientific notation floats

Examples:
- `0`
- `42`
- `3.14`
- `.5`
- `10.`
- `6.02e23`
- `1e-9`

Deferred:
- hexadecimal and binary literals unless explicitly added by later spec revision

Lexing rules:
- sign characters are not part of the numeric literal token
- exponent markers are part of the literal
- `1.'` must lex as numeric literal followed by dot-transpose operator if supported by syntax

## String and Char Literals

Release 0.1 policy:

- single-quoted text lexes as char-vector literal
- double-quoted text lexes as string scalar literal

Examples:
- `'abc'`
- `"abc"`

Escaping policy for Release 0.1:
- doubled delimiter escapes itself inside the same literal family
- additional escape forms may be added later only by spec update

Examples:
- `'can''t'`
- `"a ""quoted"" token"`

## Comments

Release 0.1 comment forms:

- line comment starting with `%`

Deferred:
- block comments unless and until explicitly specified

Comments:
- do not appear in the semantic token stream
- do remain available as trivia/source map data

## Whitespace and Newlines

The lexer must distinguish:
- horizontal whitespace
- newline boundaries

Newline significance:
- newline tokens are emitted because statement boundaries, command-form handling, and matrix literal parsing may depend on them
- later parser stages may suppress specific newline tokens when inside grouping constructs or line continuation contexts

## Line Continuation

Release 0.1 line continuation form:
- `...` followed by optional whitespace/comment and then newline

Behavior:
- line continuation suppresses the newline as a statement boundary
- the token stream should preserve source mapping so diagnostics still point to physical lines

## Operators and Punctuation

Release 0.1 operators include:

- arithmetic: `+`, `-`, `*`, `/`, `\`, `^`
- elementwise arithmetic: `.*`, `./`, `.\`, `.^`
- relational: `<`, `<=`, `>`, `>=`, `==`, `~=`
- logical: `&`, `|`, `~`, `&&`, `||`
- assignment: `=`
- range: `:`
- member access: `.`
- transpose forms: `'`, `.'`
- function handle marker: `@`

Delimiters and punctuation include:

- `(` `)`
- `[` `]`
- `{` `}`
- `,`
- `;`

## Matrix Literal Separators

Inside `[` `]` matrix construction:
- comma separates columns
- whitespace may separate columns when syntactically unambiguous
- semicolon or newline separates rows

Lexer note:
- ambiguity resolution belongs mostly to the parser, but newline tokens must be preserved so row boundaries can be recovered

## Ambiguity Notes

The lexer must preserve enough distinction for the parser to resolve:

- transpose operator vs char literal closing quote
- command-form statements vs function calls with identifiers
- matrix whitespace separators vs statement whitespace
- dot-prefixed operators vs member access plus operator

## Output Contract

Each token must carry:
- token kind
- lexeme slice or normalized value view
- source span
- line/column mapping handle
