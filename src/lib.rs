//! Minimal egglog adapters. Historical prototypes live in the research crate.
pub mod pipeline;
pub mod visual_rule;

pub mod embedded_rules;
pub mod native_analyze;

mod native_lower;

pub mod dag_embedding;

mod catalog_embedding;

pub mod binding_reduce;

pub mod closed_state;
pub mod coarse_smooth;
mod layer_bridge;

pub mod layer_patterns;

pub mod comb_reuse;
pub mod use_fractals;

pub mod closure_contract;
