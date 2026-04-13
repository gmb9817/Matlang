# Testing Strategy

## Layers

1. unit
2. parser golden
3. semantic expectation
4. runtime behavior
5. conformance
6. compatibility differential
7. regression
8. performance
9. fuzz

## Rules

- every bug fix gets a regression test
- IR changes should update verifier expectations
- compatibility claims require tests
- performance optimizations must have regression coverage when practical
