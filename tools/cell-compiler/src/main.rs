#[global_allocator]
static ALLOCATOR: eggfuse_cell_runtime::RuntimeAllocator = eggfuse_cell_runtime::RuntimeAllocator;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    eggfuse_cell_compiler::cli(false)
}
