//! Physical layout only: no rule/binding semantics are delegated to the solver.
use crate::{Result, program::Checked};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Gram,
    Paired,
    Rectangular,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub method: Method,
    pub conditions: Vec<usize>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Plan {
    pub blocks: Vec<Block>,
    pub report: Value,
}
impl Plan {
    pub fn uniform(c: &Checked, method: Method) -> Self {
        Self {
            blocks: c
                .groups
                .iter()
                .map(|g| Block {
                    method: method.clone(),
                    conditions: g.conditions.clone(),
                })
                .collect(),
            report: json!({"method":method,"solver":false}),
        }
    }
    pub fn validate(&self, c: &Checked) -> Result<()> {
        let required = c
            .groups
            .iter()
            .flat_map(|g| g.conditions.iter().copied())
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        for b in &self.blocks {
            let first = *b.conditions.first().ok_or("empty layout block")?;
            let shape = &c.features[c
                .program
                .conditions
                .get(first)
                .ok_or("bad layout condition")?
                .left
                .1]
                .shape;
            for id in &b.conditions {
                if !required.contains(id) || !seen.insert(*id) {
                    return Err("layout must cover each required condition exactly once".into());
                }
                if c.features[c.program.conditions[*id].left.1].shape != *shape {
                    return Err("layout mixed incompatible feature shapes".into());
                }
            }
        }
        if seen != required {
            return Err("layout omitted a required condition".into());
        }
        Ok(())
    }
}
pub struct Planner {
    pub mode: String,
    pub python: PathBuf,
    pub solver: String,
    pub costs: Option<PathBuf>,
    cache: Option<(String, Plan)>,
    pub solves: usize,
    pub cache_hits: usize,
}
impl Default for Planner {
    fn default() -> Self {
        Self {
            mode: "legacy".into(),
            python: "python3".into(),
            solver: "scip".into(),
            costs: None,
            cache: None,
            solves: 0,
            cache_hits: 0,
        }
    }
}
impl Planner {
    pub fn plan(&mut self, c: &Checked) -> Result<Plan> {
        match self.mode.as_str() {
            "legacy" => return Ok(Plan::uniform(c, Method::Gram)),
            "paired" => return Ok(Plan::uniform(c, Method::Paired)),
            "solver" => (),
            _ => return Err("matrix layout must be legacy, paired or solver".into()),
        }
        let request = json!({"version":1,"groups":c.groups.iter().map(|g|json!({
            "width":g.width,"edges":g.conditions.iter().map(|i| {
                let p=&c.program.conditions[*i];
                json!([i,g.wires.binary_search(&p.left).unwrap(),g.wires.binary_search(&p.right).unwrap()])
            }).collect::<Vec<_>>()
        })).collect::<Vec<_>>()});
        let request_json = serde_json::to_string(&request)?;
        let costs = self
            .costs
            .as_ref()
            .map(std::fs::read_to_string)
            .transpose()?;
        let key = serde_json::to_string(&(&request, &self.solver, &self.python, costs))?;
        if let Some((_, p)) = self.cache.as_ref().filter(|(k, _)| k == &key) {
            self.cache_hits += 1;
            return Ok(p.clone());
        }
        if c.groups.is_empty() {
            return Ok(Plan::uniform(c, Method::Paired));
        }
        let mut command = Command::new(&self.python);
        command
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("layout_solver.py"))
            .args(["--solver", &self.solver])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(path) = &self.costs {
            command.arg("--costs").arg(path);
        }
        let mut child = command.spawn()?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request_json.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err("layout solver failed; no fallback or partial plan installed".into());
        }
        let plan: Plan = serde_json::from_slice(&output.stdout)?;
        plan.validate(c)?;
        self.solves += 1;
        // Live semantic coordinates only grow. Retaining every obsolete whole
        // plan would make compiler memory grow with history for no reuse benefit.
        self.cache = Some((key, plan.clone()));
        Ok(plan)
    }
}
