# Matlang

Matlang is a modular, compiler-first reimplementation of a MATLAB-compatible language system.

Matlang is an independent project and is not affiliated with or endorsed by MathWorks.
MATLAB is a trademark of The MathWorks, Inc.

Current project intent:
- Keep the project CLI-first while the language, runtime, and compatibility surface continue to grow.
- Ship a practical MATLAB-compatible subset for Release 0.1 rather than a parser-only prototype.
- Keep the repository decomposed for long-running multi-AI and human collaboration.

Primary documents:
- `docs/architecture/OVERVIEW.md`
- `docs/architecture/RUST_WORKSPACE.md`
- `docs/handoff/CURRENT_STATE.md`

Current status:
- The end-to-end pipeline is live: parser -> semantics -> HIR -> optimizer -> codegen -> serialized bytecode artifact/bundle -> interpreter/bytecode execution -> workspace snapshot export.
- `.\scripts\cargo-msvc.cmd test --workspace` passes in the supported MSVC environment.
- The project is still a subset, but it is no longer a minimal bring-up. Large portions of execution, bytecode parity, object support, and builtin coverage are real and regression-tested.

Current default technical direction:
- Implementation language: Rust
- Execution path: parser -> semantics -> interpreter -> bytecode VM -> native backend
- Compatibility strategy: explicit feature matrix with documented divergences
- Helper build scripts: `scripts/cargo-msvc.cmd` and `scripts/cargo-msvc.ps1`

Immediate focus:
1. Keep pushing MATLAB fidelity instead of subsystem bring-up: more builtins, edge semantics, and compatibility polish.
2. Broaden documented divergences and fixture coverage around the current object/runtime/bytecode surface.
3. Deepen backend and interchange layers such as artifact evolution, MAT-file interoperability, and tooling/debuggability.
4. Continue broadening command-form, classdef, and library behavior where MATLAB semantics are still incomplete.

Current compatibility highlights:
- Interpreter and bytecode VM both execute a broad shared fixture corpus with golden parity.
- The runtime supports meaningful array, cell, struct, logical, text, and object behavior instead of numeric-only execution.
- The current array runtime also covers folded N-D indexing/assignment behavior when MATLAB uses fewer subscripts than array rank, including growth through the folded trailing dimension.
- Logical values and constructor helpers now follow a broader MATLAB-compatible slice too, including bare `true`/`false` values, logical array constructors, the current `like=` subset for `zeros` / `ones` / `eye`, the current real-floating-point `like=` subset for `Inf` / `NaN`, preserved logical class on current empty logical constructor/conversion results, current empty string-array results keeping string class, current `logical(...)` conversion errors for `NaN` and complex inputs, and current `isreal(...)` behavior that keeps explicit complex values nonreal even when their imaginary parts are zero, including current empty explicit-complex arrays and string-array matrix inputs.
- The current shallow reflection/helper slice also keeps improving: `class` / `isa` remain compatibility-layer helpers, `isnumeric` now treats current integer scalars and integer matrices as numeric while still keeping logical and string values nonnumeric, `isfloat` / `isinteger` now cover the current floating-point and integer scalar-or-matrix subset including both empty integer arrays and direct `int64([])` / `uint64([])` casts keeping their real class identity, `isdouble` no longer misclassifies those empty integer arrays as double, empty struct arrays now preserve both `struct` identity and their field-name schema for the current `fieldnames` / `isfield` / `rmfield` / `orderfields` / `struct2cell` subset, empty object arrays created through the current object-array deletion path now keep `class` / `isobject` / `isa` identity and preserve that typed-empty metadata through the current concat helpers too, and the current transpose / reshape / permute / squeeze / `flip` / `rot90` / `circshift` / `repelem` shape-helper slice now also preserves logical, string, integer, struct, and object empty-array identity instead of collapsing those empties back to `double`, while plain `[]` no longer trips struct, handle, or object reflection predicates by accident, empty string scalars now stay nonempty for `isempty("")` while empty char arrays still report empty, and `isa` now recognizes the current built-in category names `numeric`, `float`, and `integer` in addition to direct class-name matches.
- Command-form parsing now covers a broader MATLAB-like quoting slice too: single-quoted values lower as real char vectors, double-quoted text with spaces now splits into multiple char-vector command inputs when it appears standalone or embedded in a larger top-level raw command argument, and grouped raw chunks preserve inner spacing and quoted whitespace like `nested{1, 2}` and `value(1,"two words",3)` as one command input.
- Matrix and cell literal parsing now also keeps MATLAB-like signed whitespace column splitting when those literals are nested inside call arguments, so forms like `int64([0 -1])` and `int64([1 +2])` stay two-column literals while grouped arithmetic like `int64([(1 +2)])` still stays arithmetic.
- The current `classdef` slice includes value and handle objects, method dispatch, method handles, inheritance, package/folder class layouts, and save/load roundtrip for the implemented subset.
- Numeric/library coverage is broad and includes linear algebra, reductions, text helpers, ordering/shape helpers, and a substantial signal/filter slice, including current integer-aware `bounds` / `range` / direct integer-scalar `iqr` / `quantile` / `prctile` / `zscore` / `mad` / `mode` / `geomean` / `harmmean` / `rms` / `skewness` / `kurtosis` support together with the implemented `median` / `var` / `std` / `"all"` / `vecdim` / current NaN-handling and multi-output forms, direct integer-scalar support for the current promoted-output floating-point and transcendental unary helper slice such as `eps` / `log10` / `log2` / `pow2` / `exp` / `log` / `sin` / `cosh` / `sqrt` / `isfinite` / `isinf` / `isnan` / `sign`, direct integer-scalar support for the current promoted-output binary helper slice such as `mod` / `rem` / `hypot` / `mag2db` / `db2mag` / `pow2db` / `db2pow`, direct integer-scalar support for the current promoted-output complex-helper slice such as `angle` / `real` / `imag` / `conj` / unary `complex(x)` / binary `complex(x, y)`, direct integer-scalar support for the current text/formatting helper slice such as `num2str` / `int2str` / `mat2str` / `dec2bin` / `dec2hex` / `dec2base`, direct integer-scalar support for the current numeric-or-complex vector-entry helper slice such as `toeplitz` / `hankel` / `vander` / `polyder` / `filter` / `freqz`, integer size-vector and integer scalar dimension support for the current `reshape` subset, and current class-preserving integer scalar support for `floor` / `ceil` / `fix` / `round` / `abs` / unary `+` / unary `-`, current row-wise char-matrix and N-D char-array behavior across the implemented literal/formatted `compose`, current logical/integer structural helpers such as `diag` / `trace` / `issymmetric` / `ishermitian` / `istriu` / `istril` / `triu` / `tril` / `blkdiag` for the implemented scalar-and-matrix subset including empty-result class preservation where relevant, current integer-aware `sort` / `sortrows` / `issorted` / `issortedrows` support for the implemented scalar-and-matrix subset including preserved integer class on `sort` / `sortrows` outputs and typed-empty value results, plus direct integer/logical/complex scalar identity through the current reshape/repmat/concat and transpose/flip/rotate/circshift/page-transpose helper slices, doc-aligned `join`/`strjoin`/`strtok`/`split`/`splitlines`/`strsplit` text-input handling, documented `strfind(...,'ForceCellOutput',...)` behavior, text set-operation vector validation, multi-pattern `contains` / `startsWith` / `endsWith`, append/strcat, pad, extract/replace/search, path-helper, and struct field-name text-array subsets.
- Signal Processing compatibility now includes a nontrivial SOS/CTF/state-space/`digitalFilter` conversion and analysis surface, plus basic plotting behavior for functions such as `freqz`, `phasez`, `grpdelay`, `impz`, `stepz`, and `zplane`, including the current integer scalar point-count subset for the implemented `freqz`-family and `freqs` response helpers, a first `invfreqz` / `invfreqs` equation-error subset for `[b, a] = invfreqz(h, w, n, m[, wt])` and `[b, a] = invfreqs(h, w, n, m[, wt])`, a broader `lpc` subset that now includes the real-vector default-order form `lpc(x)` together with the current real matrix column-channel `lpc(x, p)` slice, a first `aryule(x, p)` subset for real/complex vector input and current real matrix column-channel input with `[a, e, rc]` outputs, a broader reflection/prediction conversion seam through `ac2rc(r)`, `ac2poly(r)`, `poly2rc(a[, eFinal])`, and `rc2poly(k[, r0])` for the current vector/matrix subsets together with a first real `rlevinson(a, eFinal)` subset for `[r, u, k, eEvolution]`, direct integer zero-location support for the implemented `zp2ss` / `zp2tf` subset, a first named-window helper slice (`hamming`, `hann`, `blackman`, `blackmanharris`, `barthannwin`, `bartlett`, `bohmanwin`, `chebwin`, `taylorwin`, `triang`, `rectwin`, `flattopwin`, `gausswin`, `kaiser`, `nuttallwin`, `parzenwin`, `tukeywin`) together with a first `enbw` subset for direct window vectors, a broader `window(fhandle, n, winopt...)` gateway subset over those current built-in windows together with docs-aligned required `winopt` handling for `chebwin`, `kaiser`, and `tukeywin` plus the current two-element numeric-vector `winopt` form for `taylorwin`, a first `kaiserord` subset covering current lowpass/highpass/bandpass/stop specs together with `[n, Wn, beta, ftype]` outputs and the current `'cell'` form for `fir1`, a broader `fir1` slice covering low/high/bandpass/stop forms together with explicit or named windows, current `scale` / `noscale` handling, and the current `DC-0` / `DC-1` two-cutoff aliases, a first `fir2` subset covering MATLAB-like default-Hamming `fir2(n, f, m)` together with the documented odd-order Nyquist-passband promotion, current explicit-window forms, current `npt` / `lap` dense-grid forms, and the current no-auto-extension rule for explicit windows under Nyquist promotion, all on the current normalized piecewise-linear frequency-sampled path, a broader first `firls` slice covering the default even-symmetry linear-phase forms `firls(n, f, a)` and `firls(n, f, a, w)` together with the current antisymmetric `firls(..., "h"/"hilbert")` / `firls(..., w, "h"/"hilbert")` and `firls(..., "d"/"differentiator")` / `firls(..., w, "d"/"differentiator")` subsets, weighted bands, and current odd-order Nyquist-passband promotion for the default symmetric path, a first `designfilt` FIR window-method subset for `lowpassfir` / `highpassfir` / `bandpassfir` / `bandstopfir` with current `FilterOrder`, `CutoffFrequency*`, `Window`, `ScalePassband`, and `SampleRate` support, a first `designfilt('differentiatorfir', ...)` least-squares subset with current `FilterOrder`, optional `PassbandFrequency` / `StopbandFrequency`, `PassbandWeight`, `StopbandWeight`, `SampleRate`, and explicit `DesignMethod="ls"` support, a first `designfilt('hilbertfir', ...)` least-squares subset with current `FilterOrder`, `TransitionWidth`, optional `SampleRate`, and explicit `DesignMethod="ls"` support, a broader `designfilt('arbmagfir', ...)` slice with direct `Frequencies` / `Amplitudes` / optional `Weights`, the current `NumBands` / `BandFrequenciesN` / `BandAmplitudesN` / optional scalar `BandWeightsN` form, optional `SampleRate`, explicit `DesignMethod="ls"`, plus a first explicit `DesignMethod="freqsamp"` subset for both the direct-vector and current banded forms with optional `Window`, a first `designfilt('fracdelayfir', ...)` direct-order subset with current `FilterOrder`, `FractionalDelay`, and optional `SampleRate` support, a broader `designfilt` IIR subset for `lowpassiir` / `highpassiir` / `bandpassiir` / `bandstopiir` that now covers both the earlier explicit-`FilterOrder` Butterworth/Chebyshev path and a first minimum-order path driven by passband/stopband specs plus `DesignMethod="butter"` / `"cheby1"` / `"cheby2"` on the current implemented subset, a broader public `bilinear` subset for transfer-function, zero-pole-gain, and state-space analog-to-digital conversion with optional prewarping, a first public `impinvar` subset for transfer-function analog-to-digital conversion with optional repeated-pole tolerance, a broader public `lp2lp` / `lp2hp` / `lp2bp` / `lp2bs` state-space and transfer-function subset for analog lowpass frequency transformations, plus a first analog lowpass prototype-helper slice through `buttap`, `cheb1ap`, and `cheb2ap`, a first `butter` subset covering normalized digital lowpass/highpass/bandpass/bandstop forms plus current analog `'s'` lowpass/highpass/bandpass/bandstop forms with `[b, a]` and `[z, p, k]` outputs, a broader `buttord` subset covering current scalar and two-edge digital plus analog `'s'` lowpass/highpass/bandpass/bandstop order estimation with `[n, Wn]` outputs for direct round-tripping into `butter`, a broader `cheb1ord` subset covering current scalar and two-edge digital plus analog `'s'` lowpass/highpass/bandpass/bandstop order estimation with `[n, Wp]` outputs for direct round-tripping into `cheby1`, and a broader `cheb2ord` subset covering current scalar and two-edge digital plus analog `'s'` lowpass/highpass/bandpass/bandstop order estimation with `[n, Ws]` outputs for direct round-tripping into `cheby2`.

- The same design surface now also includes a first `cheby1` subset for lowpass/highpass/bandpass/stop digital forms and analog `'s'` lowpass/highpass/bandpass/stop forms with current `[b, a]` and `[z, p, k]` outputs.
- That same design surface now also includes a first `cheby2` subset for lowpass/highpass/bandpass/stop digital forms and analog `'s'` lowpass/highpass/bandpass/stop forms with current `[b, a]` and `[z, p, k]` outputs.

Current usable CLI surface:
- `matc parse <file.m>`
- `matc check <file.m>`
- `matc lower <file.m>`
- `matc optimize <file.m>`
- `matc codegen <file.m>`
- `matc run <file.m> [scalar args...]`
- `matc run-workspace <file.m> [scalar args...]`
- `matc run-bytecode <file.m> [scalar args...]`
- `matc package-bytecode <file.m> <artifact.matbc>`
- `matc inspect-bytecode <artifact.matbc>`
- `matc bundle-bytecode <file.m> <bundle.matpkg>`
- `matc inspect-bundle <bundle.matpkg>`
- `matc run-artifact <artifact.matbc> [scalar args...]`
- `matc run-bundle <bundle.matpkg> [scalar args...]`
- `matc export-workspace <input> <snapshot.matws> [scalar args...]`
- `matc inspect-workspace <snapshot.matws>`

Quick start:
1. Run tests in the supported Windows/MSVC environment with `.\scripts\cargo-msvc.cmd test --workspace`.
2. Execute a fixture through the interpreter with `.\scripts\cargo-msvc.cmd run -p matc -- run tests\fixtures\execution\interpreter\builtin_complex_numbers.m`.
3. Execute the same file through the bytecode VM with `.\scripts\cargo-msvc.cmd run -p matc -- run-bytecode tests\fixtures\execution\interpreter\builtin_complex_numbers.m`.

Current public-facing project name:
- `Matlang`
- CLI binary currently remains `matc`

License:
- Apache License 2.0

Contributors must read:
1. `docs/handoff/CURRENT_STATE.md`
2. `docs/handoff/FUTURE_ONLY.md`
3. relevant `SPEC.md` and `DECISIONS.md` before editing a module
