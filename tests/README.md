The main test surface is split by intent.

- `fixtures/`: golden-backed source programs already used by the current crates
- `unit/`: focused helper-level assets and future small-scope test data
- `integration/`: cross-crate and CLI-level scenarios
- `regression/`: minimized repros for fixed bugs
- `compatibility/`: MATLAB-behavior comparison cases
- `conformance/`: supported-subset gates
- `perf/`: performance regression cases
- `fuzz/`: randomized or reducer-driven corpus inputs

New work should land in the narrowest folder that matches its purpose.
