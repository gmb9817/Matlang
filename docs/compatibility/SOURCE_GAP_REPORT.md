# Source Gap Report

This is a source-driven compatibility snapshot for the current repository state.

Scope notes:
- This is not a claim of full MATLAB coverage.
- It is based on the current builtin tables, explicit runtime `Unsupported(...)` branches, and the forward backlog docs already in the repo.
- MATLAB's total function surface is too large to finish in one patch; this file is the working map for the remaining compatibility campaign.

## Updated In This Pass

- Windows figure viewing now renders through a native host window again instead of the crashing browser-app path.
- The native host now loads an IE-edge-compatible figure page so SVG plots actually render.
- The figure viewer now has first-pass inspection tools: pan mode, data-tip mode, clear-tips, live coordinate readout, and SVG point metadata for interactive picking.
- The figure viewer now also includes a first client-side brush-selection mode with native-host toolbar/menu wiring and point highlighting on metadata-backed series.
- The figure viewer now also includes a first client-side 3-D rotate/orbit mode with native-host toolbar/menu wiring, stored axes view metadata, and live re-projection for current 3-D series and surface-patch SVG content.
- Graphics layout coverage now includes `tiledlayout` / `nexttile` in the current figure-layout subset.
- Graphics plotting coverage now includes a first `fplot` subset for function handles over finite intervals, routed through the existing line plot pipeline.
- Graphics plotting coverage now also includes a first `fplot3` parametric subset routed through the existing 3-D plot pipeline.
- Graphics plotting coverage now also includes a first `errorbar` subset with vertical, horizontal, and both-direction forms plus symmetric/asymmetric errors, line specs, and property-pair styling.
- Graphics plotting coverage now also includes first-pass `barh`, `stem3`, and `plotyy` support on top of the current bar/3-D/dual-axis state.
- Graphics plotting coverage now also includes a first `bar3` subset built on the current surface/patch renderer.
- Graphics plotting coverage now also includes a first `bar3h` subset for horizontal 3-D bars in the current surface/patch renderer.
- Graphics plotting coverage now also includes a first `pie3` subset with exploded slices, labels, extruded faces, and current 3-D view support.
- Graphics plotting coverage now also includes a first `contour3` subset that lifts extracted contour levels into the current 3-D view pipeline.
- Graphics plotting coverage now also includes a first `meshz` subset with base curtain geometry on top of the current mesh/surface renderer.
- Graphics plotting coverage now also includes a first `waterfall` subset with row-strip rendering and base-plane curtains in the current 3-D surface pipeline.
- Graphics plotting coverage now also includes a first `ribbon` subset with 3-D strip surfaces in the current surface renderer.
- Graphics plotting coverage now also includes a first `fill3` subset for projected 3-D filled polygons.
- Graphics plotting coverage now also includes first `fsurf` / `fmesh` subsets for scalar function surfaces on sampled rectangular domains with `MeshDensity`.
- Graphics plotting coverage now also includes first `fcontour` / `fcontour3` subsets for sampled scalar function contours on rectangular domains with `MeshDensity`.
- Graphics plotting coverage now also includes a first `fimplicit` subset for sampled zero-level implicit curves on rectangular domains with `MeshDensity`.
- Graphics interaction/runtime coverage now also includes a first `rotate3d` builtin subset with `on` / `off` / `toggle` behavior for the current figure.
- Graphics multi-axes workflow coverage now also includes a first `linkaxes` subset for same-figure axes handles with `x`, `y`, `xy`, and `off` modes plus runtime limit propagation through `xlim`, `ylim`, `axis`, and current axes-property updates.
- Graphics plotting coverage now also includes first-pass `semilogx`, `semilogy`, and `loglog` support with stored log-axis state and log-spaced tick rendering for the current X/Y subset.
- Graphics axes/property coverage now also includes first-pass direct `xscale` / `yscale` control plus `XScale` / `YScale` property queries and updates in the current subset.
- Graphics legend coverage now also includes first-pass `Location` and `Orientation` option handling in the current subset.
- Graphics annotation object coverage now includes a first figure-level `annotation(...)` framework for `line`, `arrow`, `doublearrow`, `textarrow`, `textbox`, `rectangle`, and `ellipse`, with handle creation, rendering, deletion, and basic get/set property support.
- Graphics annotation coverage now includes first-pass `xline` / `yline` support with labels, line specs, and property-pair styling in the current subset.
- Graphics title coverage now includes first-pass `sgtitle` / `subtitle` support for figure-level and axes-level secondary titles in the current subset.
- Graphics axes coverage now includes a first `yyaxis left/right` subset with side-aware y-limits, y-labels, y-ticks, and rendering.
- Runtime/session builtins now include:
  - `clc`
  - `clear` (current active-workspace variable subset, plus `all` / `global` / `functions` modes)
  - `clearvars` (current active-workspace variable subset with `-except` and regex support)
  - `save` (current MAT-file-backed subset, including `-append`, `-regexp`, `-struct`, and accepted `-mat` / `-v6` / `-v7` aliases)
  - `load` (current MAT-file-backed subset, including struct-return form plus wildcard/regexp filtering)
  - `who` (including `-file`)
  - `whos` (including `-file`)
  - `tic`
  - `toc`
- Context-aware semantic analysis now also recognizes literal statement-form `load(...)` side effects, including the current wildcard/regexp selection subset, well enough for later same-script references in the current supported subset.

Primary implementation files:
- `src/cli/src/main.rs`
- `src/execution/src/lib.rs`
- `src/semantics/src/binder.rs`

## Current Implemented Surface

The repo already has a broad first-pass builtin/runtime base.

Major builtin entry table:
- `src/stdlib/src/lib.rs`

Builtin semantic classification:
- `src/semantics/src/binder.rs`

Graphics execution and rendering:
- `src/execution/src/graphics.rs`
- `src/execution/src/lib.rs`

This includes a large subset of:
- numeric/math helpers
- matrix/linear-algebra helpers
- text/string helpers
- struct/cell helpers
- warnings/errors/MException helpers
- a substantial headless graphics baseline

## Major Remaining Gap Categories

These are the important remaining MATLAB parity gaps visible from the current source.

### 1. Full Graphics Tool Parity

The figure window now opens and renders plots, but it is still not pixel-for-pixel MATLAB.

Remaining work includes:
- exact MATLAB toolbar/menu/icon layout
- fuller pan/zoom/data-cursor behavior parity
- linked axes, plot edit tools, and fuller brush parity beyond the current first rotate/brush tool subsets
- docking/undocking and multi-figure management
- print/export behavior beyond the current SVG-focused path
- callback/event parity for figure and axes objects
- much broader handle-graphics property coverage

Primary files:
- `src/cli/src/main.rs`
- `src/execution/src/lib.rs`
- `src/execution/src/graphics.rs`

### 2. Graphics Function Breadth

The current graphics stack already supports a meaningful subset, but many MATLAB plotting functions and options are still missing or partial.

Examples of likely next work:
- richer `fplot` / `fplot3` option parity and adaptive sampling
- broader `errorbar` family coverage such as additional option flags and property parity
- broader `plotyy` / dual-axis compatibility beyond the current wrapper subset
- broader `barh` / `stem3` property and option parity
- broader 3-D chart/helper coverage beyond the current `bar3` / `bar3h` / `pie3` / `contour3` / `meshz` / `waterfall` / `ribbon` / `fill3` / `fsurf` / `fmesh` / `fcontour` / `fcontour3` / `fimplicit` subsets
- fuller log-axis/property parity beyond the current X/Y subset
- fuller legend/property parity beyond the current location/orientation subset
- fuller annotation/property parity beyond the current normalized-figure subset
- broader 3-D plotting families and property coverage
- richer `legend`, `colorbar`, `axes`, and annotation options
- more image/surface/3D property parity
- richer export/save formats
- fuller `linkaxes` / linked-pan viewer behavior beyond the current runtime/export limit-synchronization subset

Primary evidence:
- `docs/handoff/FUTURE_ONLY.md`
- `src/execution/src/graphics.rs`

### 3. Workspace / Session Builtin Depth

This pass added `clc`, `clear`, `clearvars`, `who`, `whos`, `tic`, and `toc`, but MATLAB session tooling is much broader.

Still missing or clearly incomplete:
- fuller `clear` / `clearvars` option parity:
  - global/function/class/import/java/mex cases
  - broader command-form and bytecode-VM parity
- broader `save` / `load` options such as:
  - fuller version/format semantics beyond the current accepted alias subset
  - fuller command-form and option-form parity beyond the current baseline
  - dynamic/nonliteral path and option inference beyond the current literal-load semantic support
  - fuller MAT-file type breadth such as broader object/class fidelity beyond the current string and function-handle subsets
- `diary`
- `format`
- workspace browser-style metadata fidelity

Primary files:
- `src/execution/src/lib.rs`
- `src/semantics/src/binder.rs`

### 4. Command-Form Fidelity

MATLAB command-form parsing is still a major parity area.

Remaining work includes:
- richer quoting and escaping
- harder command-vs-expression ambiguities
- more MATLAB-like raw argument behavior

Primary files:
- `src/frontend/src/parser.rs`
- `tests/fixtures/frontend/parser`
- `tests/fixtures/execution/interpreter/command_form_*`

### 5. Array / Indexing / Runtime Fidelity

The runtime supports a strong first subset, but exact MATLAB behavior is still broader.

Remaining work includes:
- deeper N-D indexing and assignment parity
- more `end` / colon / shape edge cases
- fuller scalar expansion compatibility
- more exact logical-array behavior
- fuller char-vs-string behavior

Primary files:
- `src/execution/src/lib.rs`
- `src/execution/src/bytecode.rs`
- `src/runtime/src/lib.rs`
- `src/stdlib/src/lib.rs`

### 6. Stdlib Option Parity

Many builtins exist, but their full MATLAB option surface is not complete.

This is visible from the large number of explicit `Unsupported(...)` branches in:
- `src/stdlib/src/lib.rs`
- `src/execution/src/graphics.rs`
- `src/execution/src/lib.rs`
- `src/execution/src/bytecode.rs`

The major remaining work is often not "missing function name entirely" but:
- missing option flags
- missing alternate calling forms
- missing multi-output conventions
- missing edge-case compatibility

### 7. Bytecode VM Parity

The interpreter and bytecode VM are close, but not perfectly identical.

Any builtin that depends on richer current-workspace inspection or UI/session state should be treated as a parity checkpoint across both paths.

Primary files:
- `src/execution/src/lib.rs`
- `src/execution/src/bytecode.rs`

### 8. Object System / classdef / MATLAB Ecosystem

These are still large future areas:
- `classdef`
- object dispatch beyond the current struct-backed `MException`
- MAT-file compatibility depth
- MEX compatibility
- Simulink is still explicitly out of scope

Primary docs:
- `docs/compatibility/FEATURE_MATRIX.md`
- `docs/handoff/FUTURE_ONLY.md`

## Practical Reading Of The Current State

The repo is not missing "everything".

It already has:
- a broad builtin base
- interpreter + bytecode VM
- workspace snapshot support
- a real graphics subsystem
- a working native Windows figure window

What is still missing is the long tail of MATLAB fidelity:
- more functions
- more options
- more edge cases
- more UI parity
- more exact runtime behavior

## Recommended Next Implementation Order

1. Keep expanding graphics/window parity until the figure tool feels much closer to MATLAB.
2. Deepen graphics breadth and option fidelity, especially more 3-D tools, annotations, and interactivity.
3. Close parser command-form fidelity gaps.
4. Deepen array/index/runtime parity, especially N-D and exact expansion behavior.
5. Continue builtin breadth and option parity in source-backed batches.
