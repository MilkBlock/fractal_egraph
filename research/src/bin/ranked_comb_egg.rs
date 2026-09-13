//! Restore ranked trace-connected chains using the existing symbolic composer.
#[allow(dead_code)]
#[path = "rule_combine/compose.rs"]
mod compose;
use compose::{Pat, Rule as Symbolic};
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn path(site: &str) -> Option<Vec<usize>> {
    let suffix = site.strip_prefix("head/0/expr/1")?;
    if suffix.is_empty() {
        return Some(vec![]);
    }
    let xs: Vec<_> = suffix.trim_start_matches('/').split('/').collect();
    if xs.len() % 2 != 0 {
        return None;
    }
    xs.chunks(2)
        .map(|p| {
            if p[0] == "args" {
                p[1].parse().ok()
            } else {
                None
            }
        })
        .collect()
}
fn safe_at<'a>(p: &'a Pat, path: &[usize]) -> Option<&'a Pat> {
    let mut p = p;
    for &i in path {
        if let Pat::App(_, a) = p {
            p = a.get(i)?;
        } else {
            return None;
        }
    }
    Some(p)
}
fn enodes(p: &Pat) -> usize {
    match p {
        Pat::App(_, a) => 1 + a.iter().map(enodes).sum::<usize>(),
        _ => 0,
    }
}
fn make_rule(definition: &str) -> Result<Symbolic> {
    let c = EGraph::default().parse_program(None, definition)?.remove(0);
    let Command::Rule { rule } = c else {
        return Err("not a normalized rule".into());
    };
    if rule.body.len() != 1 || rule.head.0.len() != 1 {
        return Err("composer only supports single equation rewrites".into());
    }
    let Fact::Eq(_, Expr::Var(_, root), lhs) = &rule.body[0] else {
        return Err("unsupported LHS".into());
    };
    let Action::Union(_, Expr::Var(_, target), rhs) = &rule.head.0[0] else {
        return Err("unsupported RHS".into());
    };
    if root != target {
        return Err("union root mismatch".into());
    }
    Ok(Symbolic::new(
        &rule.name,
        &lhs.to_string(),
        &rhs.to_string(),
    ))
}
fn fused_command(rule: &Symbolic, paths: &[Vec<usize>]) -> Result<(String, Vec<(Pat, Pat)>)> {
    let ordinary = rule.command("__ranked_combs");
    if paths.is_empty() {
        return Ok((ordinary, vec![(rule.lhs.clone(), rule.rhs.clone())]));
    }
    let mut parser = EGraph::default();
    let Command::Rule { rule: mut ast_rule } = parser.parse_program(None, &ordinary)?.remove(0)
    else {
        return Err("expected staged composer rule".into());
    };
    let Fact::Eq(_, Expr::Var(_, root), _) = &ast_rule.body[0] else {
        unreachable!()
    };
    let mut actions = vec![format!("(union {root} {})", rule.stages[0])];
    let mut effects = vec![(rule.lhs.clone(), rule.stages[0].clone())];
    for (i, p) in paths.iter().enumerate() {
        let before = safe_at(&rule.stages[i], p)
            .ok_or("lost intermediate apply position")?
            .clone();
        let after = safe_at(
            if i + 1 < rule.stages.len() {
                &rule.stages[i + 1]
            } else {
                &rule.rhs
            },
            p,
        )
        .ok_or("lost final apply position")?
        .clone();
        actions.push(format!("(union {before} {after})"));
        effects.push((before, after));
    }
    let Command::Rule { rule: head } = parser
        .parse_program(None, &format!("(rule () ({}))", actions.join(" ")))?
        .remove(0)
    else {
        unreachable!()
    };
    ast_rule.head = head.head;
    Ok((Command::Rule { rule: ast_rule }.to_string(), effects))
}
fn compose_candidate(
    c: &Value,
    events: &BTreeMap<u64, Value>,
    rules: &BTreeMap<String, Symbolic>,
) -> Result<(Symbolic, Vec<Vec<usize>>)> {
    let ids: Vec<_> = c["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let first = *ids.first().ok_or("Empty has no rule")?;
    let mut combined = rules[events[&first]["rule"].as_str().unwrap()].clone();
    let mut positions = BTreeMap::from([(first, vec![])]);
    let mut paths = vec![];
    for pair in ids.windows(2) {
        let (previous, id) = (pair[0], pair[1]);
        let e = &events[&id];
        let mut choices = vec![];
        for read in e["reads"].as_array().unwrap() {
            if read["producer"].as_u64() != Some(previous) {
                continue;
            }
            if !read["consumer_sites"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v == "body/0/expr/1")
            {
                continue;
            }
            for site in read["producer_sites"].as_array().into_iter().flatten() {
                if let Some(relative) = site.as_str().and_then(path) {
                    let mut p = positions[&previous].clone();
                    p.extend(relative);
                    choices.push(p);
                }
            }
        }
        choices.sort();
        choices.dedup();
        if choices.len() != 1 {
            return Err(format!(
                "requires a unique previous-producer RHS to consumer-root connection, found {}",
                choices.len()
            )
            .into());
        }
        let wanted = choices.pop().unwrap();
        let next = &rules[e["rule"].as_str().unwrap()];
        let same = combined.lhs == next.lhs
            && combined.rhs == next.rhs
            && combined.conditions == next.conditions
            && combined.stages == next.stages;
        let inputs = if same {
            vec![combined.clone()]
        } else {
            vec![combined.clone(), next.clone()]
        };
        let found=compose::effect_candidates_with_limit(&inputs,512).into_iter().find(|x|x.first==0&&x.second==usize::from(!same)&&x.path==wanted).ok_or("old composer cannot express this witnessed connection within its supported theory/size limit")?;
        combined = found.rule;
        positions.insert(id, wanted.clone());
        paths.push(wanted);
    }
    combined.name = format!("ranked_{:06}", c["rank"].as_u64().unwrap());
    Ok((combined, paths))
}
fn source_steps_match(
    effects: &[(Pat, Pat)],
    ids: &[u64],
    events: &BTreeMap<u64, Value>,
    rules: &BTreeMap<String, Symbolic>,
) -> Result<()> {
    fn bind(pattern: &Pat, term: &Pat, env: &mut BTreeMap<String, Pat>) -> bool {
        match (pattern, term) {
            (Pat::Var(v), t) => match env.get(v) {
                Some(old) => old == t,
                None => {
                    env.insert(v.clone(), t.clone());
                    true
                }
            },
            (Pat::Lit(a), Pat::Lit(b)) => a == b,
            (Pat::App(a, x), Pat::App(b, y)) => {
                a == b && x.len() == y.len() && x.iter().zip(y).all(|(a, b)| bind(a, b, env))
            }
            _ => false,
        }
    }
    fn subst(p: &Pat, env: &BTreeMap<String, Pat>) -> Pat {
        match p {
            Pat::Var(v) => env[v].clone(),
            Pat::Lit(_) => p.clone(),
            Pat::App(op, a) => Pat::App(op.clone(), a.iter().map(|p| subst(p, env)).collect()),
        }
    }
    if effects.len() != ids.len() {
        return Err("source stage count mismatch".into());
    }
    for ((before, after), id) in effects.iter().zip(ids) {
        let r = &rules[events[id]["rule"].as_str().unwrap()];
        let mut env = BTreeMap::new();
        if !bind(&r.lhs, before, &mut env) || subst(&r.rhs, &env) != *after {
            return Err("composed step is not an exact source-rule substitution".into());
        }
    }
    Ok(())
}
fn contextual_pair(
    c: &Value,
    events: &BTreeMap<u64, Value>,
    rules: &BTreeMap<String, Symbolic>,
) -> Result<(Symbolic, String, Vec<(Pat, Pat)>)> {
    let ids: Vec<_> = c["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    if ids.len() != 2 {
        return Err("contextual fallback requires exactly two stages".into());
    }
    let mut positions = vec![];
    for read in events[&ids[1]]["reads"].as_array().unwrap() {
        if read["producer"].as_u64() != Some(ids[0]) {
            continue;
        }
        if !read["producer_sites"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|s| s == "head/0/expr/1")
        {
            continue;
        }
        for site in read["consumer_sites"].as_array().into_iter().flatten() {
            if let Some(p) = site
                .as_str()
                .and_then(|s| path(&s.replacen("body/0/expr/1", "head/0/expr/1", 1)))
            {
                if !p.is_empty() {
                    positions.push(p);
                }
            }
        }
    }
    positions.sort();
    positions.dedup();
    if positions.len() != 1 {
        return Err("no unique consumer-context position".into());
    }
    let p = &positions[0];
    let first = rules[events[&ids[0]]["rule"].as_str().unwrap()].clone();
    let second = rules[events[&ids[1]]["rule"].as_str().unwrap()].clone();
    for candidate in compose::contextual_candidates(&[first, second])
        .into_iter()
        .filter(|c| c.first == 0 && c.second == 1)
    {
        let mut r = candidate.rule;
        if r.stages.len() != 1 {
            continue;
        }
        let (Some(before), Some(after)) = (safe_at(&r.lhs, p), safe_at(&r.stages[0], p)) else {
            continue;
        };
        let effects = vec![
            (before.clone(), after.clone()),
            (r.stages[0].clone(), r.rhs.clone()),
        ];
        if source_steps_match(&effects, &ids, events, rules).is_err() {
            continue;
        }
        r.name = format!("ranked_{:06}", c["rank"].as_u64().unwrap());
        let mut parser = EGraph::default();
        let Command::Rule { rule: mut ast } = parser
            .parse_program(None, &r.command("__ranked_combs"))?
            .remove(0)
        else {
            continue;
        };
        let actions = effects
            .iter()
            .map(|(a, b)| format!("(union {a} {b})"))
            .collect::<Vec<_>>()
            .join(" ");
        let Command::Rule { rule: head } = parser
            .parse_program(None, &format!("(rule () ({actions}))"))?
            .remove(0)
        else {
            unreachable!()
        };
        ast.head = head.head;
        return Ok((r, Command::Rule { rule: ast }.to_string(), effects));
    }
    Err("old contextual API found no source-verified composition".into())
}
fn concrete(p: &Pat) -> String {
    match p {
        Pat::Var(v) => format!("(Var {:?})", format!("test_{v}")),
        Pat::Lit(l) => l.to_string(),
        Pat::App(op, a) => format!(
            "({op} {})",
            a.iter().map(concrete).collect::<Vec<_>>().join(" ")
        ),
    }
}
fn validate(datatype: &str, command: &str, r: &Symbolic, effects: &[(Pat, Pat)]) -> Result<()> {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None,&format!("{datatype}\n(ruleset __ranked_combs)\n{command}\n(let $seed {})\n(run __ranked_combs 1)",concrete(&r.lhs)))?;
    eg.parse_and_run_program(None, &format!("(check (= $seed {}))", concrete(&r.rhs)))?;
    for (a, b) in effects {
        eg.parse_and_run_program(
            None,
            &format!("(check (= {} {}))", concrete(a), concrete(b)),
        )?;
    }
    Ok(())
}
fn main() -> Result<()> {
    let input: Value = serde_json::from_str(&std::fs::read_to_string(
        "experiments/comb_order/input.json",
    )?)?;
    let ranking: Value = serde_json::from_str(&std::fs::read_to_string(
        "experiments/comb_order/ranking.json",
    )?)?;
    let rules: BTreeMap<_, _> = input["definitions"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, r)| Ok((name.clone(), make_rule(r["definition"].as_str().unwrap())?)))
        .collect::<Result<_>>()?;
    let events: BTreeMap<_, _> = input["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["id"].as_u64().unwrap(), r.clone()))
        .collect();
    let source = std::fs::read_to_string(input["source"].as_str().unwrap())?;
    let datatype = EGraph::default()
        .parse_program(None, &source)?
        .into_iter()
        .find(|c| matches!(c, Command::Datatype { .. }))
        .ok_or("missing Math datatype")?
        .to_string();
    let mut out = format!(
        "; Restored using existing compose API; rules are NOT scheduled here.\n{datatype}\n(ruleset __ranked_combs)\n"
    );
    let mut report = vec![];
    let mut checked = 0;
    let mut validated = std::collections::BTreeSet::new();
    for c in ranking["rankings"].as_array().unwrap() {
        if c["events"].as_array().unwrap().is_empty() {
            report.push(
                json!({"rank":c["rank"],"status":"empty_unit","reason":"Empty is not a rewrite"}),
            );
            continue;
        }
        match compose_candidate(c,&events,&rules).and_then(|(r,paths)|{let (command,effects)=fused_command(&r,&paths)?;let ids:Vec<_>=c["events"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap()).collect();source_steps_match(&effects,&ids,&events,&rules)?;Ok((r,command,effects))}).or_else(|direct| contextual_pair(c,&events,&rules).map_err(|context| -> Box<dyn std::error::Error> {format!("direct: {direct}; contextual: {context}").into()})){
            Ok((r,command,effects))=>{
                let native_checked=effects.len()>1;
                if native_checked {let key=format!("{}|{}|{:?}",r.lhs,r.rhs,effects);if validated.insert(key){validate(&datatype,&command,&r,&effects)?;checked+=1;}}
                let meta=json!({"schema":"egg-viz/v1","kind":"ranked_combined_rule","id":r.name,"rank":c["rank"],"selected":c["selected"],"events":c["events"],"historical_score":c["score"],"historical_lhs_enodes":c["lhs_num"],"historical_rhs_enodes":c["rhs_num"],"export_lhs_ast_enodes":enodes(&r.lhs),"export_final_rhs_ast_enodes":enodes(&r.rhs),"local_union_steps":effects.len(),"source_substitutions_verified":true,"native_ground_instance_checked":native_checked});
                out+=&format!("; @egg-viz-json {meta}\n{command}\n\n");report.push(json!({"rank":c["rank"],"status":"exported","metadata":meta,"command":command}));
            },Err(e)=>report.push(json!({"rank":c["rank"],"events":c["events"],"status":"not_expressible_by_old_api","reason":e.to_string()})),
        }
    }
    let out = format!("{}\n", out.trim_end());
    EGraph::default().parse_and_run_program(None, &out)?;
    std::fs::write("experiments/comb_order/legacy_composed.egg", out)?;
    std::fs::write(
        "experiments/comb_order/legacy_egg_export.json",
        serde_json::to_string_pretty(
            &json!({"typechecked":true,"native_ground_multi_step_checks":checked,"rules":report,"scope":"all ranked candidates; native multi-step checks cached by identical action shape; unmatched DAG/position cases explicit; union steps preserved, no speedup claim"}),
        )?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const DT: &str = "(datatype Math (Var String) (Mul Math Math) (Sub Math Math) (Integral Math Math) (Diff Math Math))";
    fn r23() -> Symbolic {
        Symbolic::new(
            "R23",
            "(Integral (Mul a b) x)",
            "(Sub (Mul a (Integral b x)) (Integral (Mul (Diff x a) (Integral b x)) x))",
        )
    }
    #[test]
    fn nested_union_needs_more_than_outer_context_equality() {
        let r = r23();
        let c = compose::candidates(&[r])
            .into_iter()
            .find(|c| c.path == vec![1])
            .unwrap()
            .rule;
        let (command, effects) = fused_command(&c, &[vec![1]]).unwrap();
        validate(DT, &command, &c, &effects).unwrap();
        let mut old = EGraph::default();
        old.parse_and_run_program(
            None,
            &format!(
                "{DT} (ruleset __ranked_combs) {} (let $seed {}) (run __ranked_combs 1)",
                c.command("__ranked_combs"),
                concrete(&c.lhs)
            ),
        )
        .unwrap();
        let (a, b) = &effects[1];
        assert!(matches!(
            old.parse_and_run_program(
                None,
                &format!("(check (= {} {}))", concrete(a), concrete(b))
            ),
            Err(egglog::Error::CheckError(..))
        ));
    }
    #[test]
    fn effectful_identity_is_optional_and_preserves_the_middle() {
        let r = Symbolic::new("swap", "(Mul a b)", "(Mul b a)");
        assert!(compose::candidates(&[r.clone()]).is_empty());
        let c = compose::effect_candidates_with_limit(&[r], 32)
            .remove(0)
            .rule;
        assert_eq!(c.lhs, c.rhs);
        assert!(!c.stages.is_empty());
        let (command, effects) = fused_command(&c, &[vec![]]).unwrap();
        validate(DT, &command, &c, &effects).unwrap();
    }
    #[test]
    fn old_default_limit_stays_bounded_but_export_can_build_five_steps() {
        let r = r23();
        let mut combined = compose::candidates(&[r.clone()])
            .into_iter()
            .find(|c| c.path == vec![1])
            .unwrap()
            .rule;
        assert!(
            !compose::candidates(&[combined.clone(), r.clone()])
                .iter()
                .any(|c| c.first == 0 && c.second == 1 && c.path == vec![1, 1])
        );
        let mut paths = vec![vec![1]];
        for n in 2..=4 {
            let wanted = vec![1; n];
            combined = compose::effect_candidates_with_limit(&[combined, r.clone()], 512)
                .into_iter()
                .find(|c| c.first == 0 && c.second == 1 && c.path == wanted)
                .unwrap()
                .rule;
            paths.push(wanted);
        }
        let (command, effects) = fused_command(&combined, &paths).unwrap();
        assert_eq!(effects.len(), 5);
        validate(DT, &command, &combined, &effects).unwrap();
    }
}

#[cfg(test)]
mod contextual_test {
    use super::*;
    #[test]
    fn old_context_api_fuses_factoring_then_product_derivative() {
        let rules = BTreeMap::from([
            (
                "R10".into(),
                Symbolic::new("R10", "(Add (Mul a b) (Mul a c))", "(Mul a (Add b c))"),
            ),
            (
                "R15".into(),
                Symbolic::new(
                    "R15",
                    "(Diff x (Mul a b))",
                    "(Add (Mul a (Diff x b)) (Mul b (Diff x a)))",
                ),
            ),
        ]);
        let events = BTreeMap::from([
            (1, json!({"rule":"R10"})),
            (
                2,
                json!({"rule":"R15","reads":[{"producer":1,"producer_sites":["head/0/expr/1"],"consumer_sites":["body/0/expr/1/args/1"]}]}),
            ),
        ]);
        let c = json!({"rank":1,"events":[1,2]});
        let (r, command, effects) = contextual_pair(&c, &events, &rules).unwrap();
        validate(
            "(datatype Math (Var String) (Add Math Math) (Mul Math Math) (Diff Math Math))",
            &command,
            &r,
            &effects,
        )
        .unwrap();
        assert!(matches!(r.lhs,Pat::App(ref op,_) if op=="Diff"));
    }
}
