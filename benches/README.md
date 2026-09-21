# Headless benchmarks

Benchmarks live here so `examples/` contains runnable application examples. Each
benchmark is a separate stable-Rust executable with its own workload arguments.
`cargo bench --bench NAME -- ...` replaces the former
`cargo run --release --example NAME_bench -- ...` command.

```sh
cargo check --workspace --all-targets --all-features --locked
cargo bench --bench editing -- 10000 1000 1000
cargo bench --bench rich_text -- 1000 500
cargo bench --bench resize -- 1000 200
cargo bench --bench events -- 100000
```

Cargo supplies the release profile and appends `--bench`; the shared argument
adapter removes that marker before reading workload sizes. `editing` and
`rich_text` require the `editing` feature. Select a benchmark explicitly when
measuring one subsystem; running the full set measures several independent
workloads, not one end-to-end application.

The allocation benchmarks share one system-allocator wrapper. Counts include
successful allocations and reallocations, and live bytes track requested Rust
heap sizes. They do not measure allocator metadata, resident memory, GPU memory,
or native input delivery. Warm-loop assertions guard allocation, shaping,
layout, and cache behavior; timing output is diagnostic and has no universal
threshold. Compare runs on the same machine, compiler, profile, and workload.
