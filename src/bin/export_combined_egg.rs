//! Export native-compatible visualization bundles and a generated integration handoff.
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact, Rule},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error, path::Path};
const PREFIX: &str = "; @egg-viz-json ";
fn expressions(f: &Fact) -> Vec<&Expr> {
    match f {
        Fact::Eq(_, a, b) => vec![a, b],
        Fact::Fact(e) => vec![e],
    }
}
fn calls(e: &Expr, table: &str, out: &mut Vec<Expr>) {
    if let Expr::Call(_, op, args) = e {
        if op == table {
            out.push(e.clone());
        }
        for a in args {
            calls(a, table, out);
        }
    }
}
fn read_calls(r: &Rule, table: &str) -> Vec<Expr> {
    let mut out = vec![];
    for f in &r.body {
        for e in expressions(f) {
            calls(e, table, &mut out);
        }
    }
    out
}
fn write_calls(r: &Rule, table: &str) -> Vec<Expr> {
    let mut out = vec![];
    for a in &r.head.0 {
        match a {
            Action::Expr(_, e) | Action::Let(_, _, e) => calls(e, table, &mut out),
            Action::Union(_, a, b) => {
                calls(a, table, &mut out);
                calls(b, table, &mut out);
            }
            _ => {}
        }
    }
    out
}
fn annotate(meta: Value, command: &str) -> String {
    format!(
        "{PREFIX}{}\n{command}\n\n",
        serde_json::to_string(&meta).unwrap()
    )
}
fn renamed(r: &Rule, prefix: &str) -> Rule {
    r.clone()
        .map_symbols(&mut |x| x, &mut |v| format!("{prefix}_{v}"))
}
fn labels_for(all: &Value, id: &str) -> Value {
    all.get(id)
        .cloned()
        .unwrap_or(json!({"bindings":{},"positions":{}}))
}
fn validate_labels(rule: &Rule, labels: &Value) -> Result<(), Box<dyn Error>> {
    if !labels.is_object() {
        return Err("labels must be an object".into());
    }
    let mut vars = std::collections::BTreeSet::new();
    rule.clone().map_symbols(&mut |x| x, &mut |v| {
        vars.insert(v.clone());
        v
    });
    for field in ["bindings", "positions"] {
        if let Some(map) = labels.get(field) {
            let map = map.as_object().ok_or("labels fields must be objects")?;
            for (key, value) in map {
                if !value.is_string() {
                    return Err("display labels must be strings".into());
                }
                if field == "bindings" {
                    if !vars.contains(key) {
                        return Err(format!("unknown binding {key}").into());
                    }
                    continue;
                }
                let parts: Vec<_> = key.split('/').collect();
                if parts.len() < 4 || parts.len() % 2 != 0 || parts[2] != "expr" {
                    return Err(format!("invalid selector {key}").into());
                }
                let n: usize = parts[1].parse()?;
                let j: usize = parts[3].parse()?;
                let roots = match parts[0] {
                    "body" => expressions(rule.body.get(n).ok_or("fact index out of range")?),
                    "head" => match rule.head.0.get(n).ok_or("action index out of range")? {
                        Action::Expr(_, e) | Action::Let(_, _, e) => vec![e],
                        Action::Union(_, a, b) => vec![a, b],
                        _ => return Err("unsupported action position selector".into()),
                    },
                    _ => return Err("unknown selector side".into()),
                };
                let mut expr = *roots.get(j).ok_or("expression index out of range")?;
                for pair in parts[4..].chunks_exact(2) {
                    if pair[0] != "args" {
                        return Err("expected args in selector".into());
                    }
                    let Expr::Call(_, _, args) = expr else {
                        return Err("selector descends into leaf".into());
                    };
                    expr = args
                        .get(pair[1].parse::<usize>()?)
                        .ok_or("argument index out of range")?;
                }
            }
        }
    }
    Ok(())
}
fn lift_labels(
    dest: &mut Value,
    src: &Value,
    prefix: &str,
    bo: usize,
    ho: usize,
) -> Result<(), Box<dyn Error>> {
    for (v, label) in src["bindings"].as_object().into_iter().flatten() {
        if !label.is_string() {
            return Err("binding labels must be strings".into());
        }
        dest["bindings"][format!("{prefix}_{v}")] = label.clone();
    }
    for (selector, label) in src["positions"].as_object().into_iter().flatten() {
        let parts: Vec<_> = selector.split('/').collect();
        if parts.len() < 3 {
            return Err(format!("invalid position selector: {selector}").into());
        }
        let offset = match parts[0] {
            "body" => bo,
            "head" => ho,
            _ => return Err("position must start with body/ or head/".into()),
        };
        let n = parts[1].parse::<usize>()?;
        dest["positions"][format!(
            "{}/{}{}",
            parts[0],
            n + offset,
            format!("/{}", parts[2..].join("/"))
        )] = label.clone();
    }
    Ok(())
}
fn generate(source: &str, report: &Value, overlay: &Value) -> Result<String, Box<dyn Error>> {
    let mut parser = EGraph::default();
    let commands = parser.parse_program(None, source)?;
    let mut all = json!({});
    let mut dsl_types = BTreeMap::new();
    for line in source.lines() {
        if let Some(raw) = line.trim_start().strip_prefix(PREFIX) {
            let meta: Value = serde_json::from_str(raw)?;
            if meta["schema"] != "egg-viz/v1" {
                return Err("unknown egg-viz schema".into());
            }
            let id = meta["id"].as_str().ok_or("annotation lacks id")?;
            if meta["kind"] == "dsl_type" {
                if !meta["variants"].is_object() {
                    return Err("dsl_type variants must be an object".into());
                }
                if dsl_types.insert(id.to_string(), meta.clone()).is_some() {
                    return Err("duplicate dsl_type id".into());
                }
                continue;
            }
            if all.get(id).is_some() {
                return Err("duplicate annotation id".into());
            }
            all[id] = meta.get("labels").cloned().unwrap_or(json!({}));
        }
    }
    for (id, labels) in overlay
        .as_object()
        .ok_or("label overlay must be an object")?
    {
        all[id] = labels.clone();
    }
    let mut original = BTreeMap::new();
    let mut out = String::from(
        "; Original program plus visualization-only combined witness bundles.\n; Metadata is JSON in line comments; combined ruleset is never scheduled.\n\n",
    );
    let mut ordinal = 0;
    for command in commands {
        let command = egg_layout::visual_rule::normalize(command, ordinal);
        if let Command::Rule { rule } = &command {
            let id = if rule.name.is_empty() {
                format!("R{ordinal}")
            } else {
                rule.name.clone()
            };
            ordinal += 1;
            if report["rule_labels"][&id]["definition"].as_str() != Some(rule.to_string().as_str())
            {
                return Err(format!(
                    "profile/source rule mismatch for {id}; regenerate combine_profile"
                )
                .into());
            }
            let labels = labels_for(&all, &id);
            validate_labels(rule, &labels)?;
            out += &annotate(
                json!({"schema":"egg-viz/v1","id":id,"kind":"original_rule","labels":labels,"profile_round_limit":report["profile_round_limit"],"statistics":report["rule_labels"][&id]}),
                &command.to_string(),
            );
            original.insert(id, rule.clone());
        } else {
            let id = match &command {
                Command::Datatype { name, .. }
                | Command::Constructor { name, .. }
                | Command::Function { name, .. }
                | Command::Relation { name, .. }
                | Command::Sort { name, .. } => Some(name),
                _ => None,
            };
            if let Some(meta) = id.and_then(|name| dsl_types.remove(name)) {
                out += &annotate(meta, &command.to_string());
            } else {
                out += &format!("{command}\n");
            }
        }
    }
    if !dsl_types.is_empty() {
        return Err(format!("unattached dsl_type annotations: {:?}", dsl_types.keys()).into());
    }
    for id in all.as_object().unwrap().keys() {
        if !original.contains_key(id) {
            return Err(format!("unknown label rule id: {id}").into());
        }
    }
    if source.contains("__viz_combined") {
        return Err("reserved visualization ruleset name already present".into());
    }
    out += "\n(ruleset __viz_combined)\n\n";
    for ranking in report["motif_rankings"]
        .as_array()
        .ok_or("missing motif rankings")?
    {
        let example = ranking["example_matches"][0]
            .as_u64()
            .ok_or("missing example")?;
        let witness = report["witnesses"]
            .as_array()
            .ok_or("missing witnesses")?
            .iter()
            .find(|w| w["consumer"].as_u64() == Some(example))
            .ok_or("missing example witness")?;
        let shape = ranking["shape"].as_str().ok_or("missing shape")?;
        let target = shape.rsplit(" -> ").next().unwrap();
        let consumer = original.get(target).ok_or("unknown consumer rule")?;
        let mut producers = BTreeMap::<u64, (String, String)>::new();
        // Shape preserves first-appearance producer alias IDs p0/p1.
        let entries = shape
            .strip_prefix('[')
            .and_then(|x| x.split_once("] -> "))
            .ok_or("bad motif shape")?
            .0
            .split(" + ")
            .collect::<Vec<_>>();
        let support = witness["support"].as_array().ok_or("missing support")?;
        if entries.len() != support.len() {
            return Err("motif/support mismatch".into());
        }
        for (entry, s) in entries.iter().zip(support) {
            let (name, tail) = entry.split_once('@').ok_or("bad producer signature")?;
            let role = tail.split(':').next().ok_or("missing role")?;
            producers.insert(
                s["producer"].as_u64().ok_or("producer id")?,
                (role.into(), name.into()),
            );
        }
        let mut stages: Vec<_> = producers.values().cloned().collect();
        stages.sort_by_key(|(role, _)| role[1..].parse::<usize>().unwrap());
        stages.push(("c".into(), target.into()));
        let mut body = Vec::new();
        let mut head = Vec::new();
        let mut stage_meta = Vec::new();
        let mut labels = json!({"bindings":{},"positions":{}});
        let mut transformed = BTreeMap::new();
        for (role, name) in &stages {
            let r = original.get(name).ok_or("unknown producer rule")?;
            let renamed = renamed(r, role);
            let (bo, ho) = (body.len(), head.len());
            lift_labels(&mut labels, &labels_for(&all, name), role, bo, ho)?;
            body.extend(renamed.body.iter().map(ToString::to_string));
            head.extend(renamed.head.0.iter().map(ToString::to_string));
            stage_meta.push(json!({"role":role,"source_rule":name,"body_start":bo,"body_count":renamed.body.len(),"head_start":ho,"head_count":renamed.head.0.len()}));
            transformed.insert(role.clone(), renamed);
        }
        let renamed_consumer = renamed(consumer, "c");
        let mut connections = Vec::new();
        for s in support {
            let (role, _name) = &producers[&s["producer"].as_u64().unwrap()];
            let table = s["table"].as_str().ok_or("table")?;
            let slot = s["slot"].as_u64().ok_or("slot")? as usize;
            let producer_sites = s["producer_sites"].as_array();
            let consumer_sites = s["consumer_sites"].as_array();
            let located = producer_sites
                .zip(consumer_sites)
                .filter(|(p, c)| p.len() == 1 && c.len() == 1)
                .and_then(|(p, c)| {
                    let pp = p[0].as_str()?;
                    let cp = c[0].as_str()?;
                    Some((
                        egg_layout::visual_rule::expression_at(&transformed[role], pp)?,
                        egg_layout::visual_rule::expression_at(&renamed_consumer, cp)?,
                        pp,
                        cp,
                    ))
                });
            let Some((output, input, producer_path, consumer_path)) = located else {
                let outputs = write_calls(&transformed[role], table);
                let inputs = read_calls(&renamed_consumer, table);
                connections.push(json!({"producer_role":role,"consumer_role":"c","table":table,"read_slot":slot,"mapping_status":"unresolved_ast_occurrence","producer_sites":s["producer_sites"],"consumer_sites":s["consumer_sites"],"candidate_input_expressions":inputs.iter().map(ToString::to_string).collect::<Vec<_>>(),"candidate_output_expressions":outputs.iter().map(ToString::to_string).collect::<Vec<_>>(),"evidence":s,"note":"Missing or multiple compiler source positions; all supplied positions are retained, no guessed equality constraint was added."}));
                continue;
            };
            match (output, input) {
                (Expr::Call(_, a, _), Expr::Call(_, b, _)) if a == table && b == table => {}
                _ => return Err("source site operation differs from witnessed table".into()),
            }
            let (Expr::Call(_, _, pa), Expr::Call(_, _, ca)) = (output, input) else {
                return Err("expected table calls".into());
            };
            if pa.len() != ca.len() {
                return Err("port arity mismatch".into());
            }
            let start = body.len();
            for (a, b) in pa.iter().zip(ca) {
                body.push(format!("(= {a} {b})"));
            }
            let combined_path = |role: &str, path: &str| -> Result<String, Box<dyn Error>> {
                let stage = stage_meta
                    .iter()
                    .find(|s| s["role"].as_str() == Some(role))
                    .ok_or("missing stage")?;
                let parts: Vec<_> = path.split('/').collect();
                let offset = match parts[0] {
                    "body" => stage["body_start"].as_u64().unwrap(),
                    "head" => stage["head_start"].as_u64().unwrap(),
                    _ => return Err("invalid source side".into()),
                };
                Ok(format!(
                    "{}/{}/{}",
                    parts[0],
                    parts[1].parse::<u64>()? + offset,
                    parts[2..].join("/")
                ))
            };
            let producer_combined_position = combined_path(role, producer_path)?;
            let consumer_combined_position = combined_path("c", consumer_path)?;
            connections.push(json!({"producer_combined_position":producer_combined_position,"consumer_combined_position":consumer_combined_position,"producer_role":role,"consumer_role":"c","table":table,"read_slot":slot,"producer_position":producer_path,"consumer_position":consumer_path,"mapping_method":"compiler_source_span","constraint_start":start,"constraint_count":pa.len(),"evidence":s}));
        }
        let id = format!("combined_{:03}", ranking["rank"].as_u64().unwrap());
        let command = format!(
            "(rule ({}) ({}) :ruleset __viz_combined :name {:?})",
            body.join(" "),
            head.join(" "),
            id
        );
        out += &annotate(
            json!({"schema":"egg-viz/v1","id":id,"kind":"combined_witness_bundle","status":"visualization_only_not_a_shortcut","labels":labels,"profile_round_limit":report["profile_round_limit"],"ast_connections_complete":connections.iter().all(|c|c.get("mapping_status").is_none()),"statistics":ranking,"compiled_macro_executions":0,"stages":stage_meta,"connections":connections,"boundary_policy":"all source and consumer LHS retained, including intermediate reads; no causal prerequisite is eliminated"}),
            &command,
        );
    }
    Ok(format!("{}\n", out.trim_end()))
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: export_combined_egg SOURCE.egg PROFILE.json OUTPUT_DIR [LABELS.json]".into(),
        );
    }
    let source = std::fs::read_to_string(&args[1])?;
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&args[2])?)?;
    let labels = if let Some(p) = args.get(4) {
        serde_json::from_str(&std::fs::read_to_string(p)?)?
    } else {
        json!({})
    };
    let output = generate(&source, &report, &labels)?;
    let mut baseline = EGraph::default();
    let baseline_outputs = baseline.parse_and_run_program(Some(args[1].clone()), &source)?;
    let mut check = EGraph::default();
    let exported_outputs = check.parse_and_run_program(None, &output)?;
    let sizes = |outputs: Vec<egglog::CommandOutput>| {
        outputs
            .into_iter()
            .filter(|o| {
                matches!(
                    o,
                    egglog::CommandOutput::PrintFunctionSize(_)
                        | egglog::CommandOutput::PrintAllFunctionsSize(_)
                )
            })
            .map(|o| o.to_string())
            .collect::<Vec<_>>()
    };
    if sizes(baseline_outputs) != sizes(exported_outputs) {
        return Err("export changed printed table sizes".into());
    }
    let dir = Path::new(&args[3]);
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("combined.egg"), &output)?;
    let prompt = include_str!("../../experiments/annotated_export/deepseek-template.md")
        .replace(
            "{{ARTIFACT}}",
            &dir.canonicalize()?
                .join("combined.egg")
                .display()
                .to_string(),
        )
        .replace(
            "{{ORIGINAL_COUNT}}",
            &report["rule_labels"]
                .as_object()
                .map_or(0, |x| x.len())
                .to_string(),
        )
        .replace(
            "{{COMBINED_COUNT}}",
            &report["motif_rankings"]
                .as_array()
                .map_or(0, |x| x.len())
                .to_string(),
        )
        .replace(
            "{{ROUND_LIMIT}}",
            &report["profile_round_limit"].to_string(),
        );
    std::fs::write(dir.join("DEEPSEEK.md"), prompt)?;
    println!(
        "validated original program and annotated export; wrote {}",
        dir.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_annotation_is_not_silently_ignored() {
        let x = generate("; @egg-viz-json nope", &json!({}), &json!({}));
        assert!(x.is_err());
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    fn fixture() -> (String, Value) {
        (
            std::fs::read_to_string("egglog/tests/web-demo/cyk.egg").unwrap(),
            serde_json::from_str(
                &std::fs::read_to_string("experiments/combine_profile/cyk.json").unwrap(),
            )
            .unwrap(),
        )
    }
    #[test]
    fn native_compatibility_and_default_labels() {
        let (s, p) = fixture();
        let out = generate(&s, &p, &json!({})).unwrap();
        let metas: Vec<Value> = out
            .lines()
            .filter_map(|l| l.strip_prefix(PREFIX))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(metas.len(), 11);
        assert!(
            metas
                .iter()
                .all(|m| m["labels"]["bindings"] == json!({})
                    && m["labels"]["positions"] == json!({}))
        );
        EGraph::default().parse_and_run_program(None, &out).unwrap();
        assert!(!out.contains("(run __viz_combined"));
    }
    #[test]
    fn inline_labels_propagate_without_changing_variables() {
        let (s, p) = fixture();
        let s = format!(
            "{PREFIX}{}\n{s}",
            json!({"schema":"egg-viz/v1","id":"R1","labels":{"bindings":{"p1":"左侧长度\n显示"},"positions":{"body/1/expr/0/args/0":"长度端口"}}})
        );
        let out = generate(&s, &p, &json!({})).unwrap();
        let metas: Vec<Value> = out
            .lines()
            .filter_map(|l| l.strip_prefix(PREFIX))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(
            metas
                .iter()
                .any(|m| m["labels"]["bindings"]["c_p1"] == "左侧长度\n显示")
        );
        assert!(out.contains("c_p1"));
        EGraph::default().parse_and_run_program(None, &out).unwrap();
    }
    #[test]
    fn stale_profile_and_bad_selectors_are_errors() {
        let (s, mut p) = fixture();
        assert!(
            generate(
                &s,
                &p,
                &json!({"R1":{"positions":{"body/999/expr/0":"bad"}}})
            )
            .is_err()
        );
        p["rule_labels"]["R0"]["definition"] = json!("stale");
        assert!(generate(&s, &p, &json!({})).is_err());
    }
}

#[cfg(test)]
mod dsl_metadata_tests {
    use super::*;
    #[test]
    fn preserves_dsl_templates_and_lifts_updated_labels() {
        let source =
            std::fs::read_to_string("experiments/annotated_export/cyk/source.egg").unwrap();
        let report: Value = serde_json::from_str(
            &std::fs::read_to_string("experiments/combine_profile/cyk.json").unwrap(),
        )
        .unwrap();
        let out = generate(&source, &report, &json!({})).unwrap();
        let metas: Vec<Value> = out
            .lines()
            .filter_map(|l| l.strip_prefix(PREFIX))
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(metas.iter().filter(|m| m["kind"] == "dsl_type").count(), 8);
        assert!(
            !out.lines()
                .filter(|l| !l.trim_start().starts_with(';'))
                .any(|l| l.contains(":args_name"))
        );
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, &out).unwrap();
        for meta in metas.iter().filter(|m| m["kind"] == "dsl_type") {
            for (variant, config) in meta["variants"].as_object().unwrap() {
                let fields = config["fields"].as_array().unwrap();
                assert_eq!(
                    fields.len(),
                    eg.get_function(variant).unwrap().schema().input.len()
                );
            }
        }
        for meta in metas
            .iter()
            .filter(|m| m["kind"] == "combined_witness_bundle")
        {
            for stage in meta["stages"].as_array().unwrap() {
                if stage["source_rule"] == "R2" {
                    let role = stage["role"].as_str().unwrap();
                    assert_eq!(
                        meta["labels"]["bindings"][format!("{role}_a")],
                        "parent nonterminal"
                    );
                }
            }
        }
    }
}
