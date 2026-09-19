//! Minimal egglog adapters. Historical prototypes live in the research crate.
pub mod pipeline;
pub mod visual_rule;

pub mod embedded_rules;
pub mod native_analyze;

mod native_lower;

pub mod dag_embedding;

mod catalog_embedding;

pub mod binding_reduce;

pub mod coarse_smooth;
mod layer_bridge;
