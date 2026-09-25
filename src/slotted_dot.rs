//! Graphviz DOT export for the original `slotted-egraphs` data model.
//!
//! The exporter mirrors the native egglog presentation but adds the information
//! that matters for slots: e-class slot sets, AppliedId slot maps, and edges from
//! an e-node to its child e-classes. It is deliberately read-only and does not
//! enumerate every group permutation; group generators are shown by the caller
//! when available through the original debug dump.
use slotted_egraphs::{Analysis, EGraph, Language};
use std::fmt::Debug;

fn esc(s: impl AsRef<str>) -> String {
    s.as_ref()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Render all live e-classes in a slotted egraph as Graphviz DOT.
///
/// E-class clusters are blue, e-nodes are gray boxes, and child edges carry the
/// original `AppliedId` mapping. The mapping is essential: an edge to the same
/// e-class can still instantiate different slots.
pub fn to_dot<L, N>(eg: &EGraph<L, N>) -> String
where
    L: Language + Debug,
    N: Analysis<L>,
{
    let mut out = String::from(
        "digraph SlottedEGraph {\n  rankdir=TB;\n  graph [pad=0.2, nodesep=0.25, ranksep=0.55];\n  node [fontname=Helvetica, fontsize=10];\n",
    );
    for id in eg.ids() {
        let slots = eg
            .slots(id)
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        out.push_str(&format!("  subgraph cluster_eclass_{} {{\n    color=\"#4c78a8\"; style=rounded; label=\"EClass {:?} slots {{{}}}\";\n", id.0, id, esc(slots.join(", "))));
        let mut nodes = eg.enodes(id).into_iter().collect::<Vec<_>>();
        nodes.sort_by_key(|n| format!("{n:?}"));
        for (index, node) in nodes.iter().enumerate() {
            let node_id = format!("e{}_{}", id.0, index);
            out.push_str(&format!(
                "    {node_id} [shape=box, style=filled, fillcolor=\"#eeeeee\", label=\"{}\"];\n",
                esc(format!("{node:?}"))
            ));
            for child in node.applied_id_occurrences() {
                let found = eg.find_applied_id(child);
                let child_id = format!("eclass_{}", found.id.0);
                out.push_str(&format!(
                    "    {node_id} -> {child_id} [label=\"{}\", color=\"#555555\"];\n",
                    esc(format!("{:?}", found.m))
                ));
            }
        }
        out.push_str("  }\n");
    }
    out.push_str("  legend [shape=note, label=\"blue cluster = e-class\\ngray box = slotted e-node\\nedge label = AppliedId SlotMap\\nslot permutations are represented by e-class equality, not duplicated nodes\"];\n}\n");
    out
}
