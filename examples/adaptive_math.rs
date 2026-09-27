fn main() -> egg_layout::pipeline::Result {
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() < 3 || a.len() > 4 {
        return Err("adaptive_math native|adaptive|frozen SOURCE.egg OUT [LIBRARY.json]".into());
    }
    let r = egg_layout::reuse_graph::adaptive::run(
        &std::fs::read_to_string(&a[1])?,
        a.get(3).map(std::path::Path::new),
        std::path::Path::new(&a[2]),
        &a[0],
    )?;
    println!("{r}");
    Ok(())
}
