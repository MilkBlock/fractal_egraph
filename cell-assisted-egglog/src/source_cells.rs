//! Self-contained query-context declarations. These are complete-query plans,
//! not certificates that a subset of the LHS implies a saturated closure.
use crate::{
    Result,
    parameterized_cell::{Runtime, boundary},
};
use egglog::{EGraph, ast::Command};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

pub const HEADER: &str = "; cell-runtime/v1 ";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema: String,
    /// Exact native parsed command sequence; comments and filenames are not
    /// executable identity. Do not execute the copy stored in this field.
    pub commands: Vec<String>,
    pub queries: Vec<boundary::Spec>,
    pub native_rulesets: Vec<String>,
    pub marker_tables: Vec<String>,
}
impl Document {
    pub fn parse(text: &str, commands: &[Command]) -> Result<Option<Self>> {
        let headers = text
            .lines()
            .filter_map(|s| s.strip_prefix(HEADER))
            .collect::<Vec<_>>();
        if headers.len() > 1 {
            return Err("duplicate cell-runtime declaration".into());
        }
        let Some(header) = headers.first() else {
            return Ok(None);
        };
        fn self_contained(c: &Command) -> bool {
            match c {
                Command::Include(..)
                | Command::Input { .. }
                | Command::Push(..)
                | Command::Pop(..) => false,
                Command::Fail(_, c) => self_contained(c),
                _ => true,
            }
        }
        if !commands.iter().all(self_contained) {
            return Err("inline Cell source must be self-contained without stack scopes".into());
        }
        let d: Self = serde_json::from_str(header)?;
        if d.schema != "cell-runtime/v1"
            || d.commands != commands.iter().map(ToString::to_string).collect::<Vec<_>>()
        {
            return Err("Cell declaration does not match the executable source; regenerate source_sk_mk.egg".into());
        }
        if d.queries.iter().any(|q| q.plan.is_none()) {
            return Err("source Cell requires fully selected typed wiring".into());
        }
        for xs in [&d.native_rulesets, &d.marker_tables] {
            if xs.iter().collect::<BTreeSet<_>>().len() != xs.len() {
                return Err("duplicate Cell metadata item".into());
            }
        }
        Ok(Some(d))
    }
    pub fn attach(
        &self,
        eg: EGraph,
    ) -> Result<(EGraph, Option<Box<dyn crate::table_live::SaturationHook>>)> {
        for s in &self.native_rulesets {
            eg.cell_ruleset_members(s)?;
        }
        for t in &self.marker_tables {
            if eg.get_function(t).is_none() {
                return Err(format!("unknown declared marker table {t}").into());
            }
        }
        if self.queries.is_empty() {
            return Ok((eg, None));
        }
        // No query discovery, named-rule recognizer, imported library or env
        // sidecar. The generic compiler validates every slot/guard/access.
        let r = Runtime::from_declared_queries(eg, &self.queries)?;
        let (eg, hook) = r.detach_joint()?;
        Ok((eg, Some(Box::new(hook))))
    }
    pub fn report(&self, eg: &EGraph) -> Json {
        json!({"schema":"source-cell-observations/v1","authority":"complete native query; no implicit RipSat closure or sat-skip",
            "declared_queries":self.queries.iter().map(|q|json!({"ruleset":q.ruleset,"rule":q.rule,"root":q.root})).collect::<Vec<_>>(),
            "markers":self.marker_tables.iter().map(|t|json!({"table":t,"rows":eg.get_size(t)})).collect::<Vec<_>>(),
            "source_is_only_plan_input":true})
    }
}
