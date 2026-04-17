# Matlang

Matlang is a modular, compiler-first reimplementation of a MATLAB-compatible language system.

Matlang is an independent project and is not affiliated with or endorsed by MathWorks.
MATLAB is a trademark of The MathWorks, Inc.

Current project intent:
- Build a CLI-first compiler and runtime before any desktop UX work.
- Target a practical MATLAB-compatible core subset for Release 0.1.
- Keep the repository decomposed for long-running multi-AI and human collaboration.

Primary documents:
- `docs/architecture/OVERVIEW.md`
- `docs/architecture/RUST_WORKSPACE.md`
- `docs/handoff/CURRENT_STATE.md`

Current default technical direction:
- Implementation language: Rust
- Execution bring-up path: parser -> semantics -> interpreter -> bytecode VM -> native backend
- Compatibility strategy: explicit feature matrix with documented divergences
- Helper build scripts: `scripts/cargo-msvc.cmd` and `scripts/cargo-msvc.ps1`

Immediate focus:
1. Expand semantic binding from first-pass symbols/scopes into real workspace and resolution rules.
2. Expand parser and lexer coverage for more MATLAB edge cases and ambiguous forms.
3. Broaden fixture coverage and hook it into fuller test runs once `link.exe` is available.
4. Bring up runtime MVP and interpreter.

Current usable CLI surface:
- `matc parse <file.m>`
- `matc check <file.m>`

Current public-facing project name:
- `Matlang`
- CLI binary currently remains `matc`

License:
- Apache License 2.0

Contributors must read:
1. `docs/handoff/CURRENT_STATE.md`
2. `docs/handoff/FUTURE_ONLY.md`
3. relevant `SPEC.md` and `DECISIONS.md` before editing a module
