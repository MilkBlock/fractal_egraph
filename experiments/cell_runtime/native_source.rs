//! Benchmark adapter: unchanged native frontend plus source-declared SK primitives.
//! No Cell observer, source rewrite, recipe execution, or GPU.
//! Can link the existing release egglog artifacts for a bounded benchmark.
mod pipeline {
    pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
}
#[path = "../../src/structural_key/mod.rs"]
mod structural_key;
#[path = "../device_backend/frontend.rs"]
mod frontend;

fn main() -> pipeline::Result {
    frontend::execute(|eg, text| {
        structural_key::source::install(eg, text)?;
        Ok(())
    })
}
