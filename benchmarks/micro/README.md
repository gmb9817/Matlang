Use this folder for focused microbenchmarks.

Good candidates:

- parser tokenization and command-form ambiguity cases
- HIR lowering or binder hot paths
- indexing, reshaping, and stdlib kernels

Each benchmark should isolate one operation family and keep setup overhead small.
