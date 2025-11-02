use rust_streamz::Source;

fn main() {
    let source = Source::new();

    source
        .to_stream()
        .map(|x| x * 2)
        .filter(|x| *x > 5)
        .tap(|x| println!("total = {x:?}"));

    for value in [1, 2, 3, 4] {
        source.emit(value);
    }
}
