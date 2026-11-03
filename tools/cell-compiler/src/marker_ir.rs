use crate::{
    Result,
    surface::{a, s},
};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
fn resolve<'a>(mut x: &'a J, aliases: &BTreeMap<&str, &'a J>) -> &'a J {
    while let Some(next) = x["var"].as_str().and_then(|v| aliases.get(v)) {
        x = next
    }
    x
}
fn continuations(core: &J, report: &J) -> Result<Vec<J>> {
    let tables = a(&core["tables"])
        .iter()
        .map(|t| (s(&t["name"]), t))
        .collect::<BTreeMap<_, _>>();
    let markers = a(&report["markers"])
        .iter()
        .map(|m| s(&m["name"]))
        .collect::<BTreeSet<_>>();
    let rules = a(&core["rulesets"])
        .iter()
        .flat_map(|rs| a(&rs["rules"]).iter().map(move |r| (rs, r)))
        .collect::<Vec<_>>();
    let removed = rules
        .iter()
        .flat_map(|(_, r)| a(&r["head"]))
        .filter(|e| matches!(s(&e["op"]), "delete" | "subsume"))
        .map(|e| s(&e["call"]["table"]))
        .collect::<BTreeSet<_>>();
    let mut out = vec![];
    for name in a(&report["receipt_tables"]) {
        let mut sites = BTreeMap::new();
        for (rs, r) in &rules {
            for e in a(&r["head"]).iter().filter(|e| e["call"]["table"] == *name) {
                sites.insert(
                    (
                        r["source_set"].as_str().unwrap_or(s(&rs["name"])),
                        s(&r["name"]),
                    ),
                    (*r, e),
                );
            }
        }
        if sites.len() != 1 {
            return Err("application receipt has no unique source rule".into());
        }
        let (identity, (r, last)) = sites.into_iter().next().unwrap();
        let (head, body) = (a(&r["head"]), a(&r["body"]));
        if head.last() != Some(last) || last["op"] != "set" {
            return Err("application receipt must follow all rule effects".into());
        }
        let aliases = head
            .iter()
            .filter(|e| e["op"] == "copy")
            .map(|e| (s(&e["dst"]), &e["arg"]))
            .collect();
        let keys = a(&last["args"])
            .iter()
            .map(|x| resolve(x, &aliases))
            .collect::<Vec<_>>();
        let effects = &head[..head.len() - 1];
        let bound = body
            .iter()
            .flat_map(|a1| a(&a1["args"]))
            .filter_map(|x| x["var"].as_str())
            .collect::<BTreeSet<_>>();
        let mut available = keys
            .iter()
            .filter_map(|x| x["var"].as_str())
            .collect::<BTreeSet<_>>();
        let required = effects
            .iter()
            .filter(|e| e["op"] != "copy")
            .flat_map(|e| {
                a(&e["args"])
                    .iter()
                    .chain(e.get("arg"))
                    .chain(e.get("value"))
            })
            .filter_map(|x| resolve(x, &aliases)["var"].as_str())
            .filter(|x| bound.contains(x))
            .collect::<BTreeSet<_>>();
        loop {
            let old = available.len();
            for atom in body {
                if matches!(
                    s(&atom["call"]["primitive"]),
                    "+" | "-"
                        | "*"
                        | "/"
                        | "%"
                        | "min"
                        | "max"
                        | "<"
                        | ">"
                        | "<="
                        | ">="
                        | "!="
                        | "not"
                        | "and"
                        | "or"
                        | "xor"
                        | "=>"
                        | "guard"
                ) && let Some((out, args)) = a(&atom["args"]).split_last()
                    && args
                        .iter()
                        .all(|x| x.get("literal").is_some() || available.contains(s(&x["var"])))
                    && let Some(v) = out["var"].as_str()
                {
                    available.insert(v);
                }
            }
            if available.len() == old {
                break;
            }
        }
        let safe = required.is_subset(&available)
            && !effects.is_empty()
            && effects.iter().all(|e| {
                let t = s(&e["call"]["table"]);
                e["op"] == "copy"
                    || e["op"] == "let"
                        && tables.get(t).is_some_and(|t| {
                            t["kind"] == "constructor" && t["merge_expr"].is_null()
                        })
                        && !removed.contains(t)
            });
        let (structural, ports): (Vec<_>, Vec<_>) = body
            .iter()
            .partition(|f| markers.contains(s(&f["call"]["table"])));
        out.push(json!({"receipt_table":name,"identity":identity,"keys":keys,"structural_inputs":structural,"ports":ports,"effects":effects,"completion":if safe{"immutable-constructor-effects"}else{"historical-observation-only"},"activation":"original Run membership, snapshot and per-rule semi-naive frontier; not a permanent done bit","readiness":"structural prefix rows plus current correlated port rows; missing reads remain exact delta queries","storage":"ordinary typed egraph tables; full Spine correlation, effect-input receipt projection; native canonicalization, visibility and timestamps"}));
    }
    Ok(out)
}
pub fn build(core: &J, report: &J, marked: &str, analysis: &[u8]) -> Result<J> {
    let mut ms = vec![];
    for v in a(&report["markers"]) {
        let name = &v["name"];
        let t = a(&core["tables"])
            .iter()
            .find(|t| t["name"] == *name)
            .ok_or("marker table missing")?;
        let schema = a(&t["schema"]);
        if t["kind"] != "function"
            || t["merge"] != "old"
            || schema.len() != a(&v["sorts"]).len() + 1
            || schema.last().unwrap()["name"] != "Unit"
            || schema
                .iter()
                .zip(a(&v["sorts"]))
                .any(|(a, b)| a["name"] != *b)
        {
            return Err("invalid correlated marker schema".into());
        }
        let (mut reads, mut writes, mut repair) = (vec![], vec![], vec![]);
        for rs in a(&core["rulesets"]) {
            for r in a(&rs["rules"]) {
                for (key, field) in [("body", "reads"), ("head", "writes")] {
                    let ops = a(&r[key])
                        .iter()
                        .filter(|op| op["call"]["table"] == *name)
                        .collect::<Vec<_>>();
                    if ops.is_empty() {
                        continue;
                    }
                    let item = json!({"ruleset":rs["name"],"rule":r["name"],field:ops});
                    if key == "body" {
                        reads.push(item)
                    } else if rs["name"] == "cell-spine-complete" {
                        repair.push(item)
                    } else {
                        writes.push(item)
                    }
                }
            }
        }
        if reads.is_empty() || repair.len() != 1 {
            return Err("missing marker consumer or unique completion".into());
        }
        ms.push(json!({"name":name,"fields":a(&v["sorts"]).iter().enumerate().map(|(i,sort)|json!({"slot":i,"sort":sort,"variable":format!("cell_spine_slot{i}")})).collect::<Vec<_>>(),"guarantee":v["body"],"witness_cells":v["cells"],"layout":"full correlated tuple","consumers":reads,"publishers":writes,"completion":repair,"maintenance":{"initial":"complete before first consumer","insert":"committed RHS publication plus completion","union":"canonicalize complete tuple and complete newly enabled bindings","removal":"source view tables are statically excluded if subsumable/deletable","ports":"mutable functions and numeric guards remain in consumer LHS"},"effects":"original consumer RHS; no closure endpoint substitution"}))
    }
    let mut j = json!({"schema":"eggfuse-marker-program/v2","source_sha256":crate::sha(marked.as_bytes()),"analysis_sha256":crate::sha(analysis),"typed_program":{"tables":core["tables"],"rulesets":core["rulesets"],"schedule":core["schedule"]},"markers":ms,"continuations":continuations(core,report)?,"state_representation":"prefix presence + live original ports + successful application receipts; original control owns stage and repeat","schedule":core["schedule"],"cost_model":report["cost_model"],"selection_scope":report["selection_scope"],"closure_authority":"none; complete positive source views only","obligations":["SK versus marked native projection","marked native versus standalone Cell CPU","original visible stage boundaries"]});
    j["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&j));
    Ok(j)
}
