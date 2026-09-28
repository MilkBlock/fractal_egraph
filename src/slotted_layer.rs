//! Read-only mainline adapter to the pinned upstream slotted-egraphs engine.
//! Encodes observed Apply DAGs, never hypothetical attachment enumeration.
use crate::coarse_smooth::{CombKind, Effect, LayerStore};
use crate::pipeline::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use slotted_egraphs::*;
use std::collections::BTreeMap;

/// One coarse-grained node per Apply, with actual parent AppliedIds. This is
/// an upstream Language adapter, not a second implementation of an egraph.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerDsl {
    pub metadata: String,
    pub parents: Vec<AppliedId>,
    pub tokens: Vec<Slot>,
}
impl Language for LayerDsl {
    fn all_slot_occurrences_mut(&mut self) -> Vec<&mut Slot> {
        self.parents
            .iter_mut()
            .flat_map(|p| p.m.values_mut())
            .chain(self.tokens.iter_mut())
            .collect()
    }
    fn public_slot_occurrences_mut(&mut self) -> Vec<&mut Slot> {
        self.all_slot_occurrences_mut()
    }
    fn applied_id_occurrences_mut(&mut self) -> Vec<&mut AppliedId> {
        self.parents.iter_mut().collect()
    }
    fn all_slot_occurrences(&self) -> Vec<Slot> {
        self.parents
            .iter()
            .flat_map(|p| p.m.values_immut().copied())
            .chain(self.tokens.iter().copied())
            .collect()
    }
    fn public_slot_occurrences(&self) -> Vec<Slot> {
        self.all_slot_occurrences()
    }
    fn applied_id_occurrences(&self) -> Vec<&AppliedId> {
        self.parents.iter().collect()
    }
    fn to_syntax(&self) -> Vec<SyntaxElem> {
        std::iter::once(SyntaxElem::String(self.metadata.clone()))
            .chain(self.parents.iter().cloned().map(SyntaxElem::AppliedId))
            .chain(self.tokens.iter().copied().map(SyntaxElem::Slot))
            .collect()
    }
    fn from_syntax(s: &[SyntaxElem]) -> Option<Self> {
        let SyntaxElem::String(metadata) = s.first()? else {
            return None;
        };
        let mut parents = vec![];
        let mut tokens = vec![];
        for field in &s[1..] {
            match field {
                SyntaxElem::AppliedId(p) if tokens.is_empty() => parents.push(p.clone()),
                SyntaxElem::Slot(t) => tokens.push(*t),
                _ => return None,
            }
        }
        Some(Self {
            metadata: metadata.clone(),
            parents,
            tokens,
        })
    }
    fn slots(&self) -> SmallHashSet<Slot> {
        self.all_slot_occurrences().into_iter().collect()
    }
    fn weak_shape_inplace(&mut self) -> SlotMap {
        let mut names = SlotMap::new();
        for slot in self.all_slot_occurrences_mut() {
            let renamed = names.get(*slot).unwrap_or_else(|| {
                let n = Slot::numeric(names.len() as u32);
                names.insert(*slot, n);
                n
            });
            *slot = renamed;
        }
        names.inverse()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenSpec {
    pub sort: String,
    pub literal: Option<String>,
}

pub struct LayerCanon {
    pub egraph: EGraph<LayerDsl>,
    pub roots: Vec<AppliedId>,
    report: Value,
}
fn encode_token(specs: &[TokenSpec], id: usize, slots: &mut Vec<Slot>) -> Result<Value> {
    let spec = specs.get(id).ok_or("unknown native token in slotted DSL")?;
    if let Some(literal) = &spec.literal {
        Ok(json!({"sort":spec.sort,"literal":literal}))
    } else {
        let position = slots.len();
        slots.push(Slot::named(&format!("t{id}")));
        Ok(json!({"sort":spec.sort,"slot_occurrence":position}))
    }
}
fn encode_values(specs: &[TokenSpec], ids: &[usize], slots: &mut Vec<Slot>) -> Result<Value> {
    Ok(json!(
        ids.iter()
            .map(|i| encode_token(specs, *i, slots))
            .collect::<Result<Vec<_>>>()?
    ))
}
fn encode_effects(specs: &[TokenSpec], es: &[Effect], slots: &mut Vec<Slot>) -> Result<Value> {
    let mut result = vec![];
    for e in es {
        result.push(match *e {
            Effect::RowFact(i) => json!({"RowFact":encode_token(specs,i,slots)?}),
            Effect::Equal(a, b) => {
                json!({"Equal":[encode_token(specs,a,slots)?,encode_token(specs,b,slots)?]})
            }
        });
    }
    Ok(json!(result))
}
fn mapping(id: &AppliedId) -> Vec<Value> {
    id.m.iter()
        .map(|(formal, actual)| json!({"formal":formal.to_string(),"actual":actual.to_string()}))
        .collect()
}
impl LayerCanon {
    pub fn build(layer: &LayerStore, specs: &[TokenSpec]) -> Result<Self> {
        let mut eg = EGraph::<LayerDsl>::default();
        let mut roots: Vec<AppliedId> = vec![];
        let mut groups: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
        for (i, o) in layer.occurrences.iter().enumerate() {
            let a = &o.apply;
            let comb = layer.combs.get(o.comb).ok_or("unknown Comb")?;
            if a.parents.iter().any(|p| *p >= i) {
                return Err("non-topological Apply DAG".into());
            }
            let mut slots = vec![];
            let mut fields = serde_json::Map::new();
            for (name, ids) in [
                ("wanted", &a.wanted),
                ("outputs", &a.outputs),
                ("external", &a.external),
            ] {
                fields.insert(name.into(), encode_values(specs, ids, &mut slots)?);
            }
            for (name, es) in [
                ("required", &a.required),
                ("external_facts", &a.external_facts),
                ("produced", &a.produced),
            ] {
                fields.insert(name.into(), encode_effects(specs, es, &mut slots)?);
            }
            let coarse = comb.kind == CombKind::CoarseRuleComposition;
            let metadata = serde_json::to_string(
                &json!({"rule":a.rule,"input_roles":a.input_roles,
                "output_roles":a.output_roles,"binding":a.binding,"boundary_restart":o.boundary_restart,
                "kind":comb.kind,"fields":fields}),
            )?;
            let root = eg.add(LayerDsl {
                metadata,
                parents: a.parents.iter().map(|p| roots[*p].clone()).collect(),
                tokens: slots,
            });
            let graft = coarse
                && a.parents
                    .iter()
                    .any(|p| layer.occurrences[*p].coarse_layer != o.coarse_layer);
            let parent_evidence: Vec<_> = a
                .parents
                .iter()
                .map(|p| {
                    json!({"apply":p,"coarse_layer":layer.occurrences[*p].coarse_layer,
                "smooth_layer":layer.occurrences[*p].smooth_layer,"slotted_class":roots[*p].id.0})
                })
                .collect();
            groups.entry(root.id.0).or_default().push(json!({"apply":i,"event":a.event,"comb":o.comb,
                "coarse_layer":o.coarse_layer,"smooth_layer":o.smooth_layer,"grafting_coarse_rule":graft,
                "rule":a.rule,"parents":parent_evidence,"binding":a.binding,"input_roles":a.input_roles,
                "wanted":a.wanted,"slot_map":mapping(&root)}));
            roots.push(root);
        }
        let apply_classes = groups.len();
        let groups: Vec<_> = groups
            .into_iter()
            .map(|(class, members)| {
                let grafts = members
                    .iter()
                    .filter(|m| m["grafting_coarse_rule"] == true)
                    .count();
                json!({"class":class,"instances":members.len(),"grafts":grafts,"members":members})
            })
            .collect();
        let grafts: usize = groups
            .iter()
            .map(|g| g["grafts"].as_u64().unwrap() as usize)
            .sum();
        let graft_groups = groups
            .iter()
            .filter(|g| g["grafts"].as_u64().unwrap() > 0)
            .count();
        let report = json!({"schema":"slotted-layer/v2","engine":"slotted-egraphs=0.0.36",
            "scope":"alpha-equivalent observed Apply DAGs including complete parent slot maps; no assumed operator symmetry, no tier0 union; not a RipSatEgraph saturation certificate",
            "applications":roots.len(),"apply_classes":apply_classes,"grafting_coarse_applications":grafts,
            "graft_classes":graft_groups,"shared_graft_classes":groups.iter().filter(|g|g["grafts"].as_u64().unwrap()>1).count(),
            "slotted_enodes":eg.total_number_of_nodes(),"slotted_eclasses":eg.ids().len(),
            "instance_slot_map_entries":roots.iter().map(|r|r.m.len()).sum::<usize>(),
            "coarse_layers":layer.coarse_layers.len(),"smooth_layers":layer.smooth_layers.len(),
            "groups":groups,"tokens":specs});
        Ok(Self {
            egraph: eg,
            roots,
            report,
        })
    }
    pub fn report(&self) -> &Value {
        &self.report
    }

    /// Return the canonical slotted class for each observed Apply occurrence.
    ///
    /// The vector is indexed by the Apply occurrence ID used by `LayerStore`.
    /// It is derived from the egraph's actual roots, so callers do not need to
    /// scrape the debug JSON or infer class membership from rendered output.
    pub fn apply_classes(&self) -> Vec<usize> {
        self.roots.iter().map(|root| root.id.0).collect()
    }

    /// Return the canonical slotted class for one observed Apply occurrence.
    pub fn class_for_apply(&self, apply: usize) -> Option<usize> {
        self.roots.get(apply).map(|root| root.id.0)
    }

    /// Return the concrete Apply occurrence IDs currently represented by a
    /// slotted class. The result is stable because report groups use the
    /// original topological occurrence order.
    pub fn applies_for_class(&self, class: usize) -> Vec<usize> {
        self.report["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|group| group["class"].as_u64() == Some(class as u64))
            .and_then(|group| group["members"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|member| member["apply"].as_u64().map(|id| id as usize))
            .collect()
    }

    /// Compact semantic-free view. Each Apply template is a slotted e-class;
    /// the full list/effect encoding is available in slotted-egraph.dot.
    pub fn to_dot(&self) -> String {
        let mut out = String::from(
            "digraph SlottedLayers {\nrankdir=TB; graph [compound=true]; node [fontname=Helvetica];\n",
        );
        let groups = self.report["groups"].as_array().unwrap();
        for g in groups {
            let c = g["class"].as_u64().unwrap();
            let members = g["members"].as_array().unwrap();
            let first = &members[0];
            let slots = self
                .egraph
                .slots(Id(c as usize))
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let title = format!(
                "Slotted class {c} ({slots})\n{} Apply / {} Graft",
                g["instances"], g["grafts"]
            );
            let color = if g["grafts"].as_u64().unwrap() > 1 {
                "#b7791f"
            } else if first["smooth_layer"].is_null() {
                "#3976a8"
            } else {
                "#43864b"
            };
            out.push_str(&format!("subgraph cluster_c{c} {{ style=\"rounded,dashed\"; color=\"{color}\"; label={};\nc{c} [shape=box,label={}];\n",quote(&title),quote(first["rule"].as_str().unwrap())));
            for m in members {
                let i = m["apply"].as_u64().unwrap();
                let label = format!(
                    "Apply {i} / CoarseLayer {} / SmoothLayer {}",
                    m["coarse_layer"], m["smooth_layer"]
                );
                out.push_str(&format!("i{i} [shape=ellipse,label={}]; i{i} -> c{c} [style=dotted,label=\"instance SlotMap (see JSON)\"];\n",quote(&label)));
            }
            out.push_str("}\n");
        }
        // Declare edges after clusters, so Graphviz cannot invent phantom child nodes.
        for g in groups {
            for m in g["members"].as_array().unwrap() {
                let i = m["apply"].as_u64().unwrap();
                for (position, p) in m["parents"].as_array().unwrap().iter().enumerate() {
                    let j = p["apply"].as_u64().unwrap();
                    out.push_str(&format!(
                        "i{i} -> i{j} [label=\"Parent {position}\",color=\"#555555\"];\n"
                    ));
                }
            }
        }
        out.push_str("legend [shape=note,label=\"gold = shared Grafting coarse rule class\\nblue = CoarseRule; green = SmoothRule\\nsame class = shared recipe, NOT equal concrete values\\nonly observed dependencies, no hypothetical attachment enumeration\"];\n}\n");
        out
    }
    /// Debug DSL, not executable rules. Full metadata and concrete mappings live
    /// in JSON; parent AppliedId references avoid recursively printing ten layers.
    pub fn to_dsl(&self) -> String {
        let mut out =
            String::from("; slotted-layer/v2 -- observed recipe DSL; not executable .egg\n");
        for g in self.report["groups"].as_array().unwrap() {
            let class = g["class"].as_u64().unwrap();
            let formal = self
                .egraph
                .slots(Id(class as usize))
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let m = &g["members"][0];
            out += &format!(
                "(class C{class} ({formal})\n  ({} {})\n  (binding {})\n)\n",
                if m["smooth_layer"].is_null() {
                    "CoarseRule"
                } else {
                    "SmoothRule"
                },
                quote(m["rule"].as_str().unwrap()),
                m["binding"]
            );
            for member in g["members"].as_array().unwrap() {
                out += &format!(
                    "(instance A{} C{class} (CoarseLayer {}) (SmoothLayer {})\n  (SlotMap {})\n  (parents {}) (graft {}))\n",
                    member["apply"],
                    member["coarse_layer"],
                    member["smooth_layer"],
                    member["slot_map"],
                    member["parents"],
                    member["grafting_coarse_rule"]
                );
            }
        }
        out
    }
}
fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}
