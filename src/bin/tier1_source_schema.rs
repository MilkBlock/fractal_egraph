//! Compatibility entry point; implementation lives in pipeline.
fn main() -> egg_layout::pipeline::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("expected INPUT OUTPUT".into());
    }
    egg_layout::pipeline::schema(&args[0], &args[1])
}
