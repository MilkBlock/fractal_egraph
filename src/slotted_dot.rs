//! Read-only DOT export of the original slotted-egraphs engine.
use slotted_egraphs::{Analysis, AppliedId, EGraph, Language, Slot, SlotMap, SyntaxElem};

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
#[derive(Clone, Copy)]
pub struct DotOptions {
    /// Debug only. None does not query symmetries. A global budget also applies;
    /// this never changes canonicalization or bounds native rule execution.
    pub symmetry_slot_limit: Option<usize>,
    pub permutation_budget: usize,
}
impl Default for DotOptions {
    fn default() -> Self {
        Self {
            symmetry_slot_limit: None,
            permutation_budget: 0,
        }
    }
}
fn next_permutation(v: &mut [Slot]) -> bool {
    let Some(i) = (0..v.len().saturating_sub(1))
        .rev()
        .find(|i| v[*i] < v[*i + 1])
    else {
        return false;
    };
    let j = (i + 1..v.len()).rev().find(|j| v[*j] > v[i]).unwrap();
    v.swap(i, j);
    v[i + 1..].reverse();
    true
}
pub fn to_dot<L: Language, N: Analysis<L>>(eg: &EGraph<L, N>) -> String {
    to_dot_with_options(eg, DotOptions::default())
}
/// Every referenced e-class has one explicit anchor inside its cluster. Child
/// ports carry full formal->actual SlotMaps, including repeated child-class use.
/// No dependency on recursion depth, so cyclic egraphs also export finitely.
pub fn to_dot_with_options<L: Language, N: Analysis<L>>(
    eg: &EGraph<L, N>,
    options: DotOptions,
) -> String {
    let mut out = String::from(
        "digraph SlottedEGraph {\nrankdir=TB; graph [compound=true]; node [fontname=Helvetica,fontsize=10];\n",
    );
    let mut edges = String::new();
    let mut budget = options.permutation_budget;
    let mut ids = eg.ids();
    ids.sort();
    for id in ids {
        let mut slots = eg.slots(id).into_iter().collect::<Vec<_>>();
        slots.sort();
        let label = format!(
            "EClass {} ({})",
            id.0,
            slots
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        out += &format!(
            "subgraph cluster_eclass_{} {{ style=\"rounded,dashed\"; color=\"#4c78a8\"; label={};\n",
            id.0,
            quote(&label)
        );
        out += &format!("eclass_{} [shape=point,width=0.04,label=\"\"];\n", id.0);
        let mut nodes = eg.enodes(id).into_iter().collect::<Vec<_>>();
        nodes.sort();
        for (i, node) in nodes.iter().enumerate() {
            let nid = format!("e{}_{}", id.0, i);
            let syntax = node.to_syntax();
            let labels: Vec<_> = syntax
                .iter()
                .filter_map(|s| match s {
                    SyntaxElem::String(s) => {
                        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(s) {
                            if let Some(rule) = meta["rule"].as_str() {
                                return Some(rule.lines().next().unwrap_or(rule).to_string());
                            }
                        }
                        Some(s.clone())
                    }
                    SyntaxElem::Slot(s) => Some(s.to_string()),
                    _ => None,
                })
                .collect();
            out += &format!(
                "{nid} [shape=plain,tooltip={},label=<<TABLE BORDER=\"1\" CELLBORDER=\"0\" CELLSPACING=\"0\"><TR><TD BGCOLOR=\"#eeeeee\">{}</TD></TR>",
                quote(&labels.join(" ")),
                html(&labels.join(" "))
            );
            for (p, child) in node.applied_id_occurrences().iter().enumerate() {
                let child = eg.find_applied_id(child);
                let label = child
                    .m
                    .iter()
                    .map(|(a, b)| format!("{a} → {b}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                out += &format!(
                    "<TR><TD PORT=\"p{p}\" BGCOLOR=\"#f5f5f5\">arg {p}: ({})</TD></TR>",
                    html(&label)
                );
                edges += &format!("{nid}:p{p} -> eclass_{} [label=\"{p}\"];\n", child.id.0);
            }
            out += "</TABLE>>];\n";
        }
        if let Some(limit) = options.symmetry_slot_limit {
            let mut tested = 0;
            let mut accepted = 0;
            let mut complete = false;
            if slots.len() <= limit {
                let base = AppliedId::new(id, SlotMap::identity(&eg.slots(id)));
                let mut permutation = slots.clone();
                complete = true;
                while next_permutation(&mut permutation) {
                    if budget == 0 {
                        complete = false;
                        break;
                    }
                    budget -= 1;
                    tested += 1;
                    let map: SlotMap = slots
                        .iter()
                        .copied()
                        .zip(permutation.iter().copied())
                        .collect();
                    if eg.eq(&base, &AppliedId::new(id, map.clone())) {
                        out += &format!(
                            "perm_{}_{accepted} [shape=diamond,color=\"#c28e00\",label={}];\n",
                            id.0,
                            quote(&format!("{map:?}"))
                        );
                        edges += &format!(
                            "eclass_{} -> perm_{}_{accepted} [dir=none,style=dashed];\n",
                            id.0, id.0
                        );
                        accepted += 1;
                    }
                }
            }
            out += &format!(
                "symmetry_{} [shape=note,label={}];\n",
                id.0,
                quote(&format!(
                    "symmetry: {accepted} nonidentity / {tested} tested; {}",
                    if complete {
                        "complete"
                    } else {
                        "incomplete/skipped"
                    }
                ))
            );
        }
        out += "}\n";
    }
    out += &edges;
    out += "legend [shape=note,label=\"dashed cluster: e-class with formal slots\\nargument box: AppliedId formal -> actual SlotMap\\nsame child class may have different argument maps\\nno symmetry enumeration by default\"];\n}\n";
    out
}
