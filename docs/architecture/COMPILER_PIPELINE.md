# Compiler Pipeline

## Planned Stages

1. Source loading
2. Lexing
3. Parsing
4. AST normalization
5. Name resolution
6. Scope and workspace construction
7. Semantic analysis
8. HIR generation
9. HIR verification
10. MIR lowering
11. Optimization passes
12. LIR lowering
13. Backend emission
14. Packaging/linking
15. Runtime execution

## Stage Contracts

### Lexer
- input: source text
- output: token stream with spans
- must preserve enough trivia/spans for diagnostics

### Parser
- input: token stream
- output: canonical AST
- must represent matrix literals, indexing, function definitions, command-form syntax policy

### Resolver/Semantics
- input: AST
- output: bound program with symbol/workspace information and diagnostics
- must distinguish scripts, functions, nested functions, captures, globals, persistents

### HIR
- preserves MATLAB-level semantics
- supports multiple results, indexing, workspace operations, dynamic dispatch points

### MIR
- lowers structured semantics into more explicit control flow and allocation sites

### LIR
- backend-oriented, explicit temporaries, explicit ownership/lifetime expectations

## Bring-Up Strategy

The project will validate semantics in this order:
1. parser golden tests
2. semantic binder tests
3. interpreter execution
4. bytecode VM parity
5. native backend parity
