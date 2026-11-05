pub mod checksum;
#[cfg(feature = "slot-transfer-audit")]
pub mod transfer_audit;

#[path = "runtime.rs"]
pub mod executor;
pub mod extract;
pub mod graph;
pub mod known_results;
pub mod merge_programs;
pub mod multistage;
pub mod program;
pub mod query;

pub mod schedule;
pub mod value;

pub mod sieve;

#[cfg(feature = "spine-blocks")]
pub mod spine_blocks;
pub mod stage_spine;

mod entry_join;
pub mod entry_schedule;

pub mod compiler;
mod merge_contract;

mod physical_plan;
mod range_index;

#[cfg(feature = "iterative-bake")]
pub mod iterative;

pub mod slots;

mod neighbor;
mod row_kernel;

/// Common allocator for the standalone CLI and its Cargo benchmark workers.
pub use mimalloc::MiMalloc as RuntimeAllocator;

#[cfg(feature = "iterative-bake")]
mod result_handoff;

pub mod context_admission;
mod local_context;
