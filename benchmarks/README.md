Benchmark assets live here.

- `micro/`: tiny kernel-level measurements for parser, lowering, runtime, or stdlib hot paths.
- `macro/`: multi-stage end-to-end scenarios that exercise a subsystem, not just one helper.
- `real-world/`: representative workloads that look like actual user code.

When adding benchmark content:

- Keep inputs deterministic.
- Record the command used to run the case.
- Prefer fixtures that can be checked into the repo and rerun without network access.
