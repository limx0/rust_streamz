# rust_streamz

rust_streamz provides a tiny set of synchronous streaming primitives for Rust: sources push items into a callback graph,
streams transform them in place, and sinks observe the results immediately. The crate also ships with simple async adapters (HTTP polling and WebSocket clients) plus a small engine that keeps those sources and timed buffers alive on a Tokio runtime.

## Highlights
- Immediate, callback-based pipelines (`Source` → `Stream` → `sink`)
- Core operators: `map`, `filter`, `filter_map`, `accumulate`, `tap`, `zip`, and `timed_buffer`
- Timed buffers that flush collected items on a fixed cadence

## Getting Started

Add rust_streamz to another crate with a path dependency while developing locally:

```toml
[dependencies]
rust_streamz = { path = "../rust_streamz" }
```

The synchronous APIs work in any Rust project. To drive the bundled async sources or the engine, ensure you run inside a Tokio runtime (the library expects the multi-threaded runtime features that are enabled in `Cargo.toml`).

### A Minimal Pipeline

```rust
use rust_streamz::Source;

fn main() {
    let source = Source::new();

    source
        .to_stream()
        .map(|x| x * 2)
        .filter(|x| *x > 5)
        .tap(|x| println!("{x:?}"));

    for value in [1, 2, 3, 4] {
        source.emit(value);
    }

    assert_eq!(*total.borrow(), 14);
}
```
