# rust_streamz

A small, callback-based streaming library, based on python-streamz and written in rust. 


## Highlights
- Full typing support via generics.
- Core operators: `map`, `filter`, `filter_map`, `accumulate`, `tap`, `zip`, and `timed_buffer`

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

See example [deribit_trade_classifier.rs](examples/deribit_trade_classifier.rs) for a more in-depth example.