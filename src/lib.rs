//! Native history -> CS interfaces -> ripen -> exact sharing -> weak dominate.
pub mod bake_limits;
pub mod closure_contract;
pub mod coarse_smooth;
pub mod native_analyze;
pub mod pipeline;
pub mod saturated_rule_composition;
pub mod state_difference;
pub mod visual_rule;

pub mod cc_merge;

pub mod reentry_bucket;

pub mod ripen_observation;

mod graph_render;

pub mod reuse_graph;
pub mod slotted_canon;
pub mod slotted_dot;
pub mod slotted_layer;

pub mod cell_recipe;
pub mod structural_key;
pub mod cell_schedule;

pub mod source_ast;
#[path="../examples/support/egg_source.rs"]
pub mod egg_source;
pub mod device_frontend;
