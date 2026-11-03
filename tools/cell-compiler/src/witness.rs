//! Positive observations guide diagnostics. A missing training row never excludes a source fragment.
use crate::{Result, read, surface::*};
use serde_json::Value as J;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
pub struct Cell {
    name: String,
    values: Vec<J>,
    rows: BTreeMap<String, Vec<Vec<usize>>>,
}
pub fn load(root: &Path) -> Result<Vec<Cell>> {
    fn paths(dir: &Path) -> Result<Vec<PathBuf>> {
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut files = fs::read_dir(dir)?
            .map(|v| v.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        files.sort();
        Ok(files)
    }
    let states = paths(&root.join("catalog/states"))?
        .into_iter()
        .filter(|p| p.extension().is_some_and(|s| s == "json"))
        .map(|p| (p, false));
    let observed = paths(&root.join("saturated-rule-composition/cells"))?
        .into_iter()
        .map(|d| d.join("observed-saturated-state.json"))
        .filter(|p| p.is_file())
        .map(|p| (p, true));
    let mut out = vec![];
    for (p, observed) in states.chain(observed) {
        let state = read(&p)?;
        let scope = &state["scope"];
        if !observed
            && (!matches!(
                s(&scope["semantics"]),
                "positive-constructor-equality/v1" | "positive-constructor-table-equality/v1"
            ) || !a(&scope["symbolic_size_conditions"]).is_empty())
        {
            continue;
        }
        let hidden = a(&state["subsumed_rows"])
            .iter()
            .filter_map(J::as_u64)
            .collect::<BTreeSet<_>>();
        let mut rows = BTreeMap::<String, Vec<Vec<usize>>>::new();
        for (i, r) in a(&state["rows"]).iter().enumerate() {
            if !hidden.contains(&(i as u64)) {
                let row = a(&r["args"])
                    .iter()
                    .chain([&r["result"]])
                    .map(|v| v.as_u64().map(|n| n as usize).ok_or("invalid Bake row"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                rows.entry(s(&r["op"]).into()).or_default().push(row);
            }
        }
        out.push(Cell {
            name: p.strip_prefix(root)?.to_string_lossy().into(),
            values: a(&state["values"]).to_vec(),
            rows,
        });
    }
    Ok(out)
}
pub fn matches(cells: &[Cell], body: &[J], sorts: &[String]) -> Vec<String> {
    fn search(
        cell: &Cell,
        atoms: &[J],
        slots: &BTreeMap<String, usize>,
        sorts: &[String],
        env: &[Option<usize>],
    ) -> bool {
        let Some((f, rest)) = atoms.split_first() else {
            return true;
        };
        let expr = &f["args"][1];
        let Some(rows) = cell.rows.get(s(&expr["head"])) else {
            return false;
        };
        rows.iter().any(|row| {
            let mut env = env.to_owned();
            let pats = a(&expr["args"]).iter().chain([&f["args"][0]]);
            row.len() == a(&expr["args"]).len() + 1
                && pats.zip(row).all(|(v, &id)| {
                    let Some(data) = cell.values.get(id) else {
                        return false;
                    };
                    if v["kind"] == "var" {
                        let slot = slots[s(&v["name"])];
                        data["sort"] == sorts[slot] && *env[slot].get_or_insert(id) == id
                    } else {
                        data["sort"] == v["sort"] && data["literal"] == v["text"]
                    }
                })
                && search(cell, rest, slots, sorts, &env)
        })
    }
    let slots = (0..sorts.len())
        .map(|i| (format!("cell_spine_slot{i}"), i))
        .collect();
    cells
        .iter()
        .filter(|c| {
            body.iter()
                .all(|f| c.rows.contains_key(s(&f["args"][1]["head"])))
                && search(c, body, &slots, sorts, &vec![None; sorts.len()])
        })
        .map(|c| c.name.clone())
        .collect()
}
