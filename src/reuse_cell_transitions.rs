//! Exact residual queries for positive constructor patterns with a functional
//! anchor. RipSatCell instances emit views directly; uncovered keys use native
//! matching. No observation-frequency heuristic is used as a proof.
use super::{Bake, Rule};
use crate::pipeline::Result;
use egglog::{EGraph, ast::Expr};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spec {
    pub source_rule: usize,
    pub consumer_rule: usize,
    /// Child path from consumer LHS to the reusable subtree.
    pub cut_path: Vec<usize>,
    /// Child path within that subtree to a unique constructor whose direct keys
    /// contain every variable of the subtree (other keys may be literals).
    pub anchor_path: Vec<usize>,
}
pub(super) struct Compiled {
    pub spec: Spec,
    pub relation: String,
    pub cover: String,
    pub builder: String,
    pub anchor: String,
    pub residual: String,
    pub definitions: String,
    pub projections: String,
    pub proof: Value,
}
fn at<'a>(mut e: &'a Expr, path: &[usize]) -> Result<&'a Expr> {
    for i in path {
        let Expr::Call(_, _, args) = e else {
            return Err("cell path does not name a constructor".into());
        };
        e = args.get(*i).ok_or("cell path out of range")?;
    }
    Ok(e)
}
fn vars(e: &Expr) -> BTreeSet<String> {
    match e {
        Expr::Var(_, n) => BTreeSet::from([n.clone()]),
        Expr::Lit(..) => BTreeSet::new(),
        Expr::Call(_, _, a) => a.iter().flat_map(vars).collect(),
    }
}
fn atoms(e: &Expr) -> usize {
    if let Expr::Call(_, _, a) = e {
        1 + a.iter().map(atoms).sum::<usize>()
    } else {
        0
    }
}
fn occurrences(e: &Expr, name: &str) -> usize {
    if let Expr::Call(_, op, a) = e {
        usize::from(op == name) + a.iter().map(|a| occurrences(a, name)).sum::<usize>()
    } else {
        0
    }
}
fn replace(e: &Expr, path: &[usize], hole: &str) -> Result<String> {
    if path.is_empty() {
        return Ok(hole.into());
    }
    let Expr::Call(_, op, args) = e else {
        return Err("invalid cut path".into());
    };
    let mut parts = Vec::new();
    for (i, a) in args.iter().enumerate() {
        parts.push(if i == path[0] {
            replace(a, &path[1..], hole)?
        } else {
            a.to_string()
        });
    }
    Ok(format!("({op} {})", parts.join(" ")))
}
type Binding = BTreeMap<String, usize>;
type FactIndex = BTreeMap<(String, Vec<usize>), usize>;
fn evaluate(e: &Expr, vars: &Binding, literals: &Binding, facts: &FactIndex) -> Option<usize> {
    match e {
        Expr::Var(_, name) => vars.get(name).copied(),
        Expr::Lit(_, v) => literals.get(&v.to_string()).copied(),
        Expr::Call(_, op, args) => {
            let inputs = args
                .iter()
                .map(|e| evaluate(e, vars, literals, facts))
                .collect::<Option<Vec<_>>>()?;
            facts.get(&(op.clone(), inputs)).copied()
        }
    }
}
pub(super) fn compile(
    eg: &mut EGraph,
    rules: &[Rule],
    bake: &Bake,
    specs: &[Spec],
) -> Result<Vec<Compiled>> {
    let mut consumers = BTreeSet::new();
    let mut compiled = vec![];
    for (id, spec) in specs.iter().enumerate() {
        if !consumers.insert(spec.consumer_rule) {
            return Err("one proved cut per consumer in the first residual compiler".into());
        }
        let cell = bake
            .macros
            .get(&spec.source_rule)
            .ok_or("source RipSatCell is not in the supplied Bake library")?;
        let rule = rules
            .get(spec.consumer_rule)
            .ok_or("unknown consumer rule")?;
        let cut = at(&rule.lhs, &spec.cut_path)?;
        if !matches!(cut, Expr::Call(..)) {
            return Err("cut must be a constructor subtree".into());
        }
        let anchor_expr = at(cut, &spec.anchor_path)?;
        let Expr::Call(_, anchor, keys) = anchor_expr else {
            return Err("anchor must be a constructor".into());
        };
        if keys.is_empty()
            || keys
                .iter()
                .any(|a| !matches!(a, Expr::Var(..) | Expr::Lit(..)))
            || vars(anchor_expr) != vars(cut)
            || occurrences(cut, anchor) != 1
        {
            return Err("coverage requires one unique flat anchor whose keys determine all subtree variables; no heuristic fallback".into());
        }
        let names = vars(cut).into_iter().collect::<Vec<_>>();
        // A functional anchor avoids subgraph search: bind its keys, then
        // evaluate the pattern forward through the cell's constructor index.
        let mut facts = FactIndex::new();
        for (op, cols) in &cell.template {
            let (output, inputs) = cols.split_last().ok_or("empty cell fact")?;
            if facts
                .insert((op.clone(), inputs.to_vec()), *output)
                .is_some_and(|old| old != *output)
            {
                return Err("cell has ambiguous constructor keys".into());
            }
        }
        let literals = cell
            .slots
            .iter()
            .enumerate()
            .map(|(i, (term, _))| (term.clone(), i))
            .collect::<Binding>();
        let mut rows = BTreeSet::new();
        for (_, cols) in cell
            .template
            .iter()
            .filter(|(op, c)| op == anchor && c.len() == keys.len() + 1)
        {
            let mut binding = Binding::new();
            let valid = keys.iter().zip(cols).all(|(expr, slot)| match expr {
                Expr::Var(_, name) => binding
                    .insert(name.clone(), *slot)
                    .is_none_or(|old| old == *slot),
                Expr::Lit(_, v) => cell.slots[*slot].0 == v.to_string(),
                _ => false,
            });
            if !valid {
                continue;
            }
            if let Some(root) = evaluate(cut, &binding, &literals, &facts) {
                let mut projection = names.iter().map(|n| binding[n]).collect::<Vec<_>>();
                projection.push(root);
                rows.insert((projection, cols[..keys.len()].to_vec()));
            }
        }
        if rows.is_empty() {
            return Err(
                "the selected RipSatCell contains no proof of the requested subtree".into(),
            );
        }
        let relation = format!("adaptive_cell_view_{id}");
        let cover = format!("adaptive_cell_cover_{id}");
        let builder = format!("adaptive_cell_build_{id}");
        let schema = eg
            .get_function(anchor)
            .ok_or("missing anchor constructor")?
            .schema();
        let cover_sorts = schema
            .input
            .iter()
            .map(|s| s.name())
            .collect::<Vec<_>>()
            .join(" ");
        let fields = names.join(" ");
        let anchor_fields = keys
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        let definitions = format!(
            "(function {relation} ({}) Math :merge old)\n(relation {cover} ({cover_sorts}))\n(ruleset {builder})\n(rule ((= adaptive_cell_subroot {cut})) ((set ({relation} {fields}) adaptive_cell_subroot) ({cover} {anchor_fields})) :ruleset {builder} :name \"{builder}\")\n",
            vec!["Math"; names.len()].join(" ")
        );
        eg.parse_and_run_program(None, &definitions)?;
        let hole = format!("adaptive_cell_cut_{id}");
        let residual = format!(
            "(= adaptive_root {}) (= {hole} ({relation} {}))",
            replace(&rule.lhs, &spec.cut_path, &hole)?,
            names.join(" ")
        );
        let projections = rows
            .iter()
            .map(|(view, keys)| {
                let terms = |cols: &Vec<usize>| {
                    cols.iter()
                        .map(|i| cell.slots[*i].0.clone())
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                format!(
                    "(set ({relation} {}) {})\n({cover} {})",
                    terms(&view[..view.len() - 1].to_vec()),
                    cell.slots[*view.last().unwrap()].0,
                    terms(keys)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let proof = json!({"spec":spec,"source_cell":spec.source_rule,"target_cell":bake.macros.contains_key(&spec.consumer_rule).then_some(spec.consumer_rule),"internal_atoms":atoms(cut),"original_lhs_atoms":atoms(&rule.lhs),"residual_lhs_atoms":atoms(&rule.lhs)-atoms(cut)+1,"symbolic_binding_rows":rows,"anchor":anchor,"internal_pattern":cut.to_string(),"view":relation,"coverage":cover,"residual":residual,"scope":"positive constructor functions; all pattern variables determined by unique anchor keys; metadata is maintained by native union/rebuild"});
        compiled.push(Compiled {
            spec: spec.clone(),
            relation,
            cover,
            builder,
            anchor: anchor.clone(),
            residual,
            definitions,
            projections,
            proof,
        });
    }
    Ok(compiled)
}
/// Return counters without retaining logical match/trace history.
pub(super) fn prepare(eg: &mut EGraph, plans: &[Compiled]) -> Result<Value> {
    let mut matches = 0;
    let mut per_plan = vec![];
    for p in plans {
        let report = eg.step_rules_with_key_coverage(
            &p.builder,
            vec![(p.builder.clone(), p.anchor.clone(), p.cover.clone())],
        )?;
        let n = report.num_matches_per_rule.values().sum::<usize>();
        matches += n;
        per_plan.push(json!({"view":p.relation,"fallback_matches":n,"view_rows":eg.get_size(&p.relation),"covered_keys":eg.get_size(&p.cover)}));
    }
    Ok(json!({"fallback_matches":matches,"plans":per_plan}))
}

pub(super) fn read_profile(eg: &EGraph) -> Value {
    #[cfg(feature = "native-read-profile")]
    {
        let tables=eg.join_read_profile().into_iter().map(|(name,p)|match p {
            Some(p)=>json!({"table":name,"scan_calls":p.scan_calls,"scan_rows":p.scan_rows,"refine_rows":p.refine_rows,"key_lookups":p.key_lookups}),
            None=>json!({"table":name,"unavailable":true}),
        }).collect::<Vec<_>>();
        json!({"tables":tables,"scope":"native search/apply row work, includes coverage probes and summary relations; not an abstract Join operator count"})
    }
    #[cfg(not(feature = "native-read-profile"))]
    {
        let _ = eg;
        Value::Null
    }
}
