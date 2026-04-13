# LIR

Status: seed draft

LIR is backend-oriented and close to executable form.

Must support:
- explicit temporaries
- explicit runtime calls
- explicit ownership/lifetime assumptions
- backend-friendly control flow

Invariant:
- LIR should be simple enough to emit to bytecode, C, or LLVM without semantic ambiguity.
