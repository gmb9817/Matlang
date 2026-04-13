# Future-Only Handoff

If you are the next AI working on this repo, start here.

## Read Order

1. Read this file.
2. Use `docs/handoff/KNOWN_BREAKAGE.md` only for confirmed limitations or edge-case caveats.
3. Use `docs/handoff/NEXT_STEPS.md` only for the broader post-plan backlog.
4. Do not reread `docs/handoff/CURRENT_STATE.md`, the master plan, or the architecture/spec docs unless you truly need deep historical context for a specific bug or design question.

## Status

- The original repository-plan bring-up is effectively complete.
- Every major planned source crate now has real implementation.
- The active repo is no longer in "build the skeleton" mode; it is in post-plan MATLAB fidelity and breadth mode.
- The supported green validation path is:
  - `.\scripts\cargo-msvc.cmd test --workspace`

## What The Next AI Should Assume

- The end-to-end path already exists and works:
  - parse
  - semantics/resolution
  - HIR
  - optimizer
  - bytecode codegen
  - artifact/bundle packaging
  - interpreter
  - bytecode VM
  - workspace snapshot interop
- The plain PowerShell environment still does not automatically provide the MSVC toolchain on `PATH`.
- Use `scripts/cargo-msvc.cmd` for reliable full validation from a normal shell.
- Do not spend time rebuilding completed scaffolding or re-auditing the whole repo plan.
- The runtime now has a meaningful first complex-number baseline:
  - first-class complex scalar values
  - builtin imaginary-unit values `i` / `j` when unshadowed
  - suffix-style imaginary literals like `1i`, `2j`, and `3e-2i`
  - current unary `+` / `-`, arithmetic `+` / `-` / `*` / `.*` / `/` / `./` / `\` / `.\`, and `==` / `~=`
  - snapshot/rendering coverage plus parser/execution fixtures
- The best next structural move is no longer "get complex numbers started"; it is "broaden complex parity or move up to true N-D array/value-model depth."

## Highest-Value Remaining Work

Focus on one of these. These are the real future-facing tracks now.

1. Graphics and plotting breadth
   - expand the current headless SVG plotting baseline beyond the current `subplot` / `plot` / `plot3` / `scatter` / `scatter3` / `quiver` / `quiver3` / `pie` / `histogram` / `histogram2` / `area` / `stairs` / `bar` / `stem` / `contour` / `contourf` / `mesh` / `meshc` / `surf` / `surfc` / `image` / `imagesc` / `imshow` / `axis` / `view` / `grid` / `box` / `shading` / `caxis` / `colormap` / `colorbar` / `legend` / `zlabel` / `xticks` / `yticks` / `zticks` / `xticklabels` / `yticklabels` / `zticklabels` / `xtickangle` / `ytickangle` / `ztickangle` / `zlim` subset
   - add more MATLAB helpers like RGB `imshow`, richer quiver/quiver3 property parity, and broader axes/property controls beyond the current `axis equal` / `axis image` / `axis normal`, current `view()` / `view(2)` / `view(3)` / explicit azimuth-elevation subset, simple `grid on/off`, current `box on/off` toggle subset, current `shading faceted` / `flat` / `interp` subset, current numeric `xticks` / `yticks` / `zticks` vector control subset, current string/cell `xticklabels` / `yticklabels` / `zticklabels` control subset, current numeric `xtickangle` / `ytickangle` / `ztickangle` query/set subset, current `xlim` / `ylim` / `zlim` and `xlabel` / `ylabel` / `zlabel` control subset, current first contour/`contourf`/`meshc`/`surfc` subsets, current first vector-only `plot3` / `scatter3` / `quiver3` subsets, current first `quiver(U, V)` / `quiver(X, Y, U, V)` autoscaled 2D subset, current first `histogram2(x, y)` / `histogram2(x, y, bins)` / `histogram2(x, y, xedges, yedges)` colormapped tile subset, and current first `mesh` / `surf` subsets
   - deepen figure/axes state instead of stopping at the current first subplot layout, direct/scaled image subset, named-colormap subset, and default-style model
   - likely files:
     - `src/execution/src/graphics.rs`
     - `src/execution/src/lib.rs`
     - `src/semantics/src/binder.rs`
     - `tests/fixtures/execution/interpreter/graphics_*`
     - `src/execution/tests/graphics_*`

2. Command-form fidelity
   - richer quoting/escaping
   - harder command-vs-expression ambiguity
   - more MATLAB-like raw argument behavior
   - likely files:
     - `src/frontend/src/parser.rs`
     - `tests/fixtures/frontend/parser/command_form.*`
     - `tests/fixtures/execution/interpreter/command_form_text.*`

3. Deeper comma-separated-list and struct/object behavior
   - forwarded CSL edge cases not yet covered
   - richer struct-array/index/call interactions
   - object-like behavior beyond the current struct-backed `MException`
   - likely files:
     - `src/execution/src/lib.rs`
     - `src/execution/src/bytecode.rs`
     - `src/codegen/src/lib.rs`
     - `tests/fixtures/execution/interpreter/comma_separated_*`

4. Runtime fidelity for arrays/logicals/strings
  - exact logical-array behavior
  - more-than-2D indexing
  - deeper `end` / shape / expansion compatibility
  - fuller char-vs-string semantics
   - likely files:
     - `src/execution/src/lib.rs`
     - `src/runtime/src/lib.rs`
     - `src/stdlib/src/lib.rs`

5. Complex-number parity
   - broaden beyond the current suffix-literal / scalar-expansion baseline
   - likely next wins:
     - broader builtin parity over complex inputs
     - decide whether to add more literal forms beyond the current suffix-style imaginary literals
     - keep complex work coordinated with the eventual N-D array/value-model redesign
   - likely files:
     - `src/frontend/src/lexer/scanner.rs`
     - `src/execution/src/lib.rs`
     - `src/execution/src/bytecode.rs`
     - `src/runtime/src/lib.rs`
     - `src/stdlib/src/lib.rs`
     - `tests/fixtures/frontend/parser/complex_literals.*`
     - `tests/fixtures/execution/interpreter/builtin_complex_numbers.*`

6. Stdlib depth
   - option parity for existing builtins
   - broader multi-output conventions
   - additional MATLAB core helpers beyond the current `meshgrid` / `ndgrid` / `histcounts` / `histcounts2` / `interp1` / `accumarray` / array-construction baseline
   - likely files:
     - `src/stdlib/src/lib.rs`
     - `src/semantics/src/binder.rs`
     - execution fixtures under `tests/fixtures/execution/interpreter`

7. Backend and interop maturity
   - richer bundle/artifact manifests
   - debug tooling
   - versioning/evolution hooks
   - MAT-file-compatible interop
   - likely files:
     - `src/platform/src/lib.rs`
     - `src/interop/src/lib.rs`
     - `src/cli/src/main.rs`

8. Workspace/closure depth
   - sibling nested function sharing
   - transitive shared mutation
   - broader global/persistent edge cases
   - likely files:
     - `src/semantics/src/binder.rs`
     - `src/execution/src/lib.rs`
     - semantics/execution fixtures

## Good First Command

Run this before and after your change:

```powershell
.\scripts\cargo-msvc.cmd test --workspace
```

If you are working on a narrow slice, run the relevant targeted suite first, then finish with the full workspace test.

## Practical Rule

Do not optimize for rereading history.

Optimize for:
- picking one remaining fidelity track
- making a real code change
- adding or updating fixtures
- keeping `.\scripts\cargo-msvc.cmd test --workspace` green

## Escalation Rule

Only dive back into the larger handoff/spec/history files if one of these happens:

- you hit a semantic rule that is genuinely unclear
- you suspect an older design decision conflicts with the current code
- you need to recover intent for a very specific subsystem edge case

Otherwise, stay in forward-implementation mode.
