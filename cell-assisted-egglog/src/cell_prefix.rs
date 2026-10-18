//! Analyze-backed, finite Cell projections. These are positive witnesses, NOT
//! closed-world assumptions: the GPU must also certify that each fully keyed local
//! read has a current visible witness before replacing that join prefix.
use crate::{Part, Result, Template, json, project_mode};
use egglog::{
    EGraph,
    ast::{Expr, Literal},
    cell_assist::CellActionSink,
};
use serde_json::{Value as Json, json as value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct Library {
    templates: Vec<Template>,
    pub evidence: Json,
}
impl Library {
    /// Values are interned in the target engine, never copied as foreign IDs.
    /// Only complete states referenced by the analyze catalog are accepted.
    pub fn load(eg: &mut EGraph, dir: &Path) -> Result<Self> {
        let catalog = json(&dir.join("catalog/catalog.json"))?;
        if catalog["schema"] != "saturated-rule-composition-catalog/v1" {
            return Err("unsupported analyzed Cell catalog".into());
        }
        let mut states = BTreeMap::<usize, BTreeSet<String>>::new();
        for trigger in catalog["triggers"]
            .as_array()
            .ok_or("missing Cell triggers")?
        {
            let Some(id) = trigger["saturated_rule_composition"].as_u64() else {
                continue;
            };
            let origin = &trigger["binding_origin"];
            let markers = states.entry(id as usize).or_default();
            markers.extend(
                origin["parameter_markers"]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.values())
                    .chain([&origin["parameter_marker"]])
                    .filter_map(Json::as_str)
                    .map(str::to_owned),
            );
        }
        let mut templates = vec![];
        let mut rejected = vec![];
        for (id, markers) in states {
            let state = json(&dir.join(format!("catalog/states/state-{id:04}.json")))?;
            let semantics = state["scope"]["semantics"].as_str().unwrap_or("");
            if !matches!(
                semantics,
                "positive-constructor-equality/v1" | "positive-constructor-table-equality/v1"
            ) || state["scope"]["symbolic_size_conditions"] == true
            {
                rejected.push(
                    value!({"state":id,"reason":"not a certified positive structural closure"}),
                );
                continue;
            }
            let mut literals = BTreeMap::new();
            for (i, v) in state["values"]
                .as_array()
                .ok_or("missing Cell values")?
                .iter()
                .enumerate()
            {
                if let Some(lit) = v["literal"].as_str() {
                    let e = egglog::ast::Parser::default().get_expr_from_string(None, lit)?;
                    // Unit can be encoded by the printer as (), but eval_expr on
                    // an arbitrary expression would create facts in the runtime.
                    if !matches!(
                        e,
                        Expr::Lit(
                            _,
                            Literal::Int(_)
                                | Literal::Float(_)
                                | Literal::String(_)
                                | Literal::Bool(_)
                                | Literal::Unit
                        )
                    ) {
                        return Err("Cell literal is not a literal".into());
                    }
                    let (sort, val) = eg.eval_expr(&e)?;
                    if Some(sort.name()) != v["sort"].as_str() {
                        return Err("Cell literal sort mismatch".into());
                    }
                    literals.insert(i, val);
                }
            }
            // Shape matching itself checks native column types. Rows not read by
            // this program (including RipenInput) never become native facts.
            templates.push(Template {
                id,
                state,
                parameters: vec![],
                producer: String::new(),
                markers,
                literals,
            });
        }
        let evidence = value!({"analysis_directory":dir,"loaded_states":templates.iter().map(|t|t.id).collect::<Vec<_>>(),"rejected":rejected,
            "contract":"positive projection plus fully keyed versioned witnesses; no saturation or completeness inferred from a historical match"});
        Ok(Self {
            templates,
            evidence,
        })
    }

    /// All witnessed aliases/literals for this exact prefix; root and non-root
    /// positions are treated alike. New aliases use fallback until re-certified.
    pub(crate) fn alternatives(
        &self,
        q: &CellActionSink,
        atoms: &[usize],
    ) -> Vec<(usize, Vec<[u32; 2]>)> {
        use egglog_numeric_id::NumericId;
        let mut bound = BTreeSet::new();
        for (i, part) in atoms.iter().enumerate() {
            // Cache only reads functionally determined by the already bound
            // input key; arbitrary reverse fibers remain ordinary queries.
            let a = &q.atoms[*part];
            if i > 0
                && !a.columns[..a.columns.len() - 1].iter().all(|o| match o {
                    egglog::cell_assist::CellOperand::Constant { .. } => true,
                    egglog::cell_assist::CellOperand::Slot(v) => bound.contains(v),
                })
            {
                return vec![];
            }
            let slots = q.atoms[*part]
                .columns
                .iter()
                .filter_map(|x| match x {
                    egglog::cell_assist::CellOperand::Slot(v) => Some(*v),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            if i > 0 && bound.is_disjoint(&slots) {
                return vec![];
            }
            bound.extend(slots);
        }
        let mut out = BTreeSet::new();
        for t in &self.templates {
            if atoms
                .iter()
                .any(|i| t.markers.contains(&q.atoms[*i].function))
            {
                continue;
            }
            for lens in project_mode(
                t,
                q,
                &Part {
                    atoms: atoms.to_vec(),
                    cell: true,
                },
                false,
            ) {
                // An alias-dependent *new* specialization was not in this closed
                // state. It remains fully supported by the normal query.
                if !lens.aliases.is_empty() {
                    continue;
                }
                let mut seen = BTreeMap::new();
                let conditions = lens
                    .binding
                    .iter()
                    .enumerate()
                    .map(|(slot, v)| {
                        let Some(v) = v else { return [0, 0] };
                        if let Some(lit) = t.literals.get(v) {
                            return [2, lit.rep()];
                        }
                        [1, *seen.entry(*v).or_insert(slot) as u32]
                    })
                    .collect();
                out.insert((t.id, conditions));
            }
        }
        out.into_iter().collect()
    }
}
