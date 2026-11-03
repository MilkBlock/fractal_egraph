use crate::program::{Program, Result, Table};
use serde_json::{Value as J, json};
use std::collections::BTreeSet;
pub(crate) fn table(p: &Program, t: &Table) -> Result<J> {
    let e = &t.merge;
    let mut vars = e["args"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| a["var"].as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    vars.sort();
    let op = e["call"]["primitive"].as_str();
    let (kind, permission) = if t.constructor {
        ("congruence", "exact equality propagation")
    } else if e.is_null() {
        ("unique", "conflicting values remain an error")
    } else if vars == ["new", "old"]
        && matches!(op, Some("min" | "max" | "or" | "and" | "cell-key-min"))
    {
        (
            if op == Some("cell-key-min") {
                "key-min"
            } else {
                "lattice"
            },
            "associative commutative idempotent on declared value domain",
        )
    } else if op == Some("cell-key-select")
        && e["args"][0]["var"] == "old"
        && e["args"][1]["var"] == "new"
        && e["args"][2]["call"]["primitive"] == "cell-key-prefer-left"
    {
        (
            "ranked-selection",
            "ordered reads; reassociation needs frozen port versions and equivalent equal-rank candidates",
        )
    } else {
        ("ordered", "original submission order")
    };
    fn visit<'a>(
        p: &Program,
        j: &'a J,
        r: &mut BTreeSet<&'a str>,
        c: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if let Some(n) = j["call"]["table"].as_str() {
            if p.tables
                .iter()
                .find(|t| t.name == n)
                .ok_or("unknown merge port")?
                .constructor
            {
                c.insert(n);
            } else {
                r.insert(n);
            }
        }
        for a in j["args"].as_array().into_iter().flatten() {
            visit(p, a, r, c)?;
        }
        Ok(())
    }
    let (mut reads, mut constructs) = (BTreeSet::new(), BTreeSet::new());
    visit(p, e, &mut reads, &mut constructs)?;
    Ok(
        json!({"table":t.name,"kind":kind,"expression":e,"permission":permission,"reads":reads,"constructs":constructs,"equal_inputs":if e["call"]["table"].is_string(){"return_old"}else{"evaluate"}}),
    )
}
