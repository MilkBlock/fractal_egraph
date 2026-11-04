#[cfg(feature = "slot-transfer-audit")]
pub mod transfer_audit;
pub mod checksum;
pub mod compositions;
pub mod congruence;
#[path = "runtime.rs"]
pub mod executor;
pub mod extract;
pub mod graph;
pub mod multistage;
pub mod merge_programs;
pub mod known_results;
pub mod program;
pub mod query;
pub mod refutation;
pub mod schedule;
pub mod value;
pub mod union_graft;
pub mod union_impact;

pub mod sieve;

pub mod views;

pub mod search_context;

pub mod local_sat;
pub mod retained;

#[cfg(feature = "spine-blocks")]
pub mod spine_blocks;
pub mod stage_spine;

pub mod entry_schedule;
mod entry_join;

mod merge_contract;
pub mod compiler;

mod physical_plan;
mod range_index;

#[cfg(feature = "iterative-bake")]
pub mod iterative;

pub mod slots;

mod row_kernel;

/// Common allocator for the standalone CLI and its Cargo benchmark workers.
pub use mimalloc::MiMalloc as RuntimeAllocator;

#[cfg(feature = "iterative-bake")]
mod result_handoff;
