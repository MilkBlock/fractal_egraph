//! Shared native frontend with explicit source-declared extension registration.
#[path = "../experiments/device_backend/frontend.rs"]
mod frontend;
fn main() -> egg_layout::pipeline::Result<()> {
    frontend::execute(|eg, text| {
        egg_layout::structural_key::source::install(eg, text)?;
        Ok(())
    })
}
