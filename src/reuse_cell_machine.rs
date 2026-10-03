//! Persistent parameterized cell states. A seed is admitted only after the whole
//! target recipe (including unions) is already true in a source symbolic cell.
//! The state tables and interface projections are native facts, not trace history.
use super::{
    Bake, Rule,
    transitions::{self, Binding, Compiled, FactIndex},
};
use crate::pipeline::Result;
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Parser},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Machine {
    pub dynamic: bool,
    pub definitions: String,
    pub actions: BTreeMap<usize, String>,
    pub completion: Vec<(String, String, Vec<String>)>,
    pub symmetries: BTreeMap<String, Vec<Vec<usize>>>,
    pub proof: Value,
}
fn match_at(cell: &super::RipSatCell, p: &Expr, slot: usize, env: &Binding) -> Vec<Binding> {
    match p {
        Expr::Var(_, v) => {
            if env.get(v).is_some_and(|x| *x != slot) {
                return vec![];
            }
            let mut b = env.clone();
            b.insert(v.clone(), slot);
            vec![b]
        }
        Expr::Lit(_, v) => {
            if cell.slots[slot].0 == v.to_string() {
                vec![env.clone()]
            } else {
                vec![]
            }
        }
        Expr::Call(_, op, args) => cell
            .template
            .iter()
            .filter(|(n, c)| n == op && c.len() == args.len() + 1 && c.last() == Some(&slot))
            .flat_map(|(_, cols)| {
                let mut rows = vec![env.clone()];
                for (p, c) in args.iter().zip(cols) {
                    rows = rows.iter().flat_map(|e| match_at(cell, p, *c, e)).collect();
                }
                rows
            })
            .collect(),
    }
}
pub(super) fn compile(
    eg: &mut EGraph,
    bake: &Bake,
    rules: &[Rule],
    plans: &[Compiled],
) -> Result<Machine> {
    let mut parser = Parser::default();
    let mut recipes = BTreeMap::new();
    let mut slot_exprs = BTreeMap::new();
    let mut definitions = String::new();
    let mut completion = vec![];
    for (&id, cell) in &bake.macros {
        let params = transitions::vars(&rules[id].lhs)
            .into_iter()
            .collect::<Vec<_>>();
        let name = format!("adaptive_cell_state_{id}");
        definitions += &format!(
            "(relation {name} ({}))\n",
            vec!["Math"; params.len()].join(" ")
        );
        completion.push((format!("adaptive_baked_{id}"), name, params));
        recipes.insert(id, parser.get_program_from_string(None, &cell.actions)?);
        slot_exprs.insert(
            id,
            cell.slots
                .iter()
                .map(|(term, _)| parser.get_expr_from_string(None, term))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        );
    }
    eg.parse_and_run_program(None, &definitions)?;
    let mut permutations = BTreeMap::<usize, BTreeSet<Vec<usize>>>::new();
    let mut actions = BTreeMap::new();
    let mut registrations = BTreeMap::new();
    let mut proofs = vec![];
    for (&source, cell) in &bake.macros {
        let mut facts = FactIndex::new();
        for (op, c) in &cell.template {
            let (out, args) = c.split_last().ok_or("empty RipSatCell fact")?;
            if facts
                .insert((op.clone(), args.to_vec()), *out)
                .is_some_and(|old| old != *out)
            {
                return Err("ambiguous cell constructor key".into());
            }
        }
        let literals = cell
            .slots
            .iter()
            .enumerate()
            .map(|(i, (s, _))| (s.clone(), i))
            .collect::<Binding>();
        let mut programs = BTreeSet::new();
        let mut seeds = BTreeSet::new();
        for (&target, t) in &bake.macros {
            let roots = cell
                .template
                .iter()
                .filter(|(op, _)| matches!(&rules[target].lhs,Expr::Call(_,n,_) if n==op))
                .filter_map(|(_, c)| c.last().copied())
                .collect::<BTreeSet<_>>();
            for root in roots {
                for mut binding in match_at(cell, &rules[target].lhs, root, &Binding::new()) {
                    let params = transitions::vars(&rules[target].lhs)
                        .iter()
                        .map(|n| binding[n])
                        .collect::<Vec<_>>();
                    binding.insert("adaptive_root".into(), root);
                    let valid = recipes[&target].iter().all(|cmd| match cmd {
                        Command::Action(Action::Let(_, n, e)) => {
                            if let Some(v) = transitions::evaluate(e, &binding, &literals, &facts) {
                                binding.insert(n.clone(), v);
                                true
                            } else {
                                false
                            }
                        }
                        Command::Action(Action::Union(_, a, b)) => match (
                            transitions::evaluate(a, &binding, &literals, &facts),
                            transitions::evaluate(b, &binding, &literals, &facts),
                        ) {
                            (Some(a), Some(b)) => a == b,
                            _ => false,
                        },
                        _ => false,
                    });
                    if !valid {
                        continue;
                    }
                    let Some(slots) = slot_exprs[&target]
                        .iter()
                        .map(|e| transitions::evaluate(e, &binding, &literals, &facts))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    if !t.template.iter().all(|(op, c)| {
                        facts.get(&(
                            op.clone(),
                            c[..c.len() - 1].iter().map(|i| slots[*i]).collect(),
                        )) == Some(&slots[*c.last().unwrap()])
                    }) {
                        continue;
                    }
                    if source == target {
                        let names = transitions::vars(&rules[source].lhs);
                        if let Some(original) = names
                            .iter()
                            .map(|n| cell.slots.iter().position(|(term, _)| term == n))
                            .collect::<Option<Vec<_>>>()
                        {
                            if let Some(perm) = params
                                .iter()
                                .map(|s| original.iter().position(|p| p == s))
                                .collect::<Option<Vec<_>>>()
                            {
                                if perm.iter().copied().collect::<BTreeSet<_>>().len()
                                    == original.len()
                                {
                                    permutations.entry(source).or_default().insert(perm);
                                }
                            }
                        }
                    }
                    if !seeds.insert((target, params.clone())) {
                        continue;
                    }
                    let terms = |ss: &[usize]| {
                        ss.iter()
                            .map(|i| cell.slots[*i].0.clone())
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    // Register after parameter symmetries have been proved for all cells.
                    // Completing an embedded cell must also provide its outgoing
                    // interfaces: otherwise skipping its RHS would break chaining.
                    for plan in plans.iter().filter(|p| p.spec.source_rule == target) {
                        for (view, cover) in &plan.rows {
                            let args = view[..view.len() - 1]
                                .iter()
                                .map(|i| slots[*i])
                                .collect::<Vec<_>>();
                            let keys = cover.iter().map(|i| slots[*i]).collect::<Vec<_>>();
                            programs.insert(format!(
                                "(set ({} {}) {})\n({} {})",
                                plan.relation,
                                terms(&args),
                                cell.slots[slots[*view.last().unwrap()]].0,
                                plan.cover,
                                terms(&keys)
                            ));
                        }
                    }
                    proofs.push(json!({"source_cell":source,"completed_cell":target,"parameter_slots":params,"target_slot_map":slots,"union_obligations_checked":true}));
                }
            }
        }
        if !seeds.iter().any(|(target, _)| *target == source) {
            return Err(format!("RipSatCell {source} failed self-completion validation").into());
        }
        registrations.insert(source, seeds);
        actions.insert(source, programs.into_iter().collect::<Vec<_>>().join("\n"));
    }
    let mut symmetries = BTreeMap::new();
    for (&id, _) in &bake.macros {
        let n = transitions::vars(&rules[id].lhs).len();
        let perms = permutations.entry(id).or_default();
        perms.insert((0..n).collect());
        let accepted = perms
            .iter()
            .filter(|p| {
                let mut inverse = vec![0; n];
                for (i, j) in p.iter().enumerate() {
                    inverse[*j] = i;
                }
                perms.contains(&inverse)
            })
            .cloned()
            .collect::<Vec<_>>();
        symmetries.insert(format!("adaptive_baked_{id}"), accepted);
    }
    // One native row can certify a whole proved parameter orbit. Use a stable
    // source-slot representative when emitting, and query every proved mapping
    // at runtime. Native union may reorder Values, so Value sorting is NOT used
    // as a persistent fact identity.
    let compact = !std::env::var("EGG_LAYOUT_CELL_COMPACT_STATES").is_ok_and(|v| v == "0");
    for (source, seeds) in registrations {
        let mut rows = BTreeSet::new();
        for (target, params) in seeds {
            let params = if compact {
                symmetries[&format!("adaptive_baked_{target}")]
                    .iter()
                    .map(|p| p.iter().map(|i| params[*i]).collect::<Vec<_>>())
                    .min()
                    .unwrap_or(params)
            } else {
                params
            };
            rows.insert((target, params));
        }
        for (target, params) in rows {
            let terms = params
                .iter()
                .map(|i| bake.macros[&source].slots[*i].0.clone())
                .collect::<Vec<_>>()
                .join(" ");
            actions
                .get_mut(&source)
                .unwrap()
                .push_str(&format!("\n(adaptive_cell_state_{target} {terms})"));
        }
    }
    let proof = json!({"schema":"cell-machine/v1","compact_state_orbits":compact,"proved_parameter_permutations":symmetries,"states":completion.iter().map(|(rule,name,params)|json!({"rule":rule,"table":name,"parameters":params})).collect::<Vec<_>>(),"completion_seeds":proofs,"transitions":plans.iter().map(|p|&p.proof).collect::<Vec<_>>(),"scope":"positive constructor cells, complete symbolic facts and union checks; state identity is cell type plus canonical original parameters; no concrete global-state enumeration"});
    Ok(Machine {
        dynamic: std::env::var("EGG_LAYOUT_DYNAMIC_INSTANCES").as_deref() != Ok("0"),
        definitions,
        actions,
        completion,
        symmetries,
        proof,
    })
}
impl Machine {
    pub fn counts(&self, eg: &EGraph) -> Value {
        let states = self
            .completion
            .iter()
            .map(|(_, table, _)| json!({"table":table,"instances":eg.get_size(table)}))
            .collect::<Vec<_>>();
        let total = states
            .iter()
            .map(|v| v["instances"].as_u64().unwrap())
            .sum::<u64>();
        json!({"stored_state_rows":total,"registered_instances":total,"count_scope":"native stored parameter representatives; proved permutations may represent additional bindings","states":states})
    }
}
