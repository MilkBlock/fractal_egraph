//! Graphviz DOT export for the original `slotted-egraphs` data model.
use slotted_egraphs::{Analysis, AppliedId, EGraph, Language, Slot, SlotMap};
use std::fmt::Debug;

fn esc(s: impl AsRef<str>) -> String {
    s.as_ref()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
fn next_permutation(values: &mut [Slot]) -> bool {
    if values.len() < 2 {
        return false;
    }
    let mut i = values.len() - 2;
    while values[i] >= values[i + 1] {
        if i == 0 {
            return false;
        }
        i -= 1;
    }
    let mut j = values.len() - 1;
    while values[j] <= values[i] {
        j -= 1;
    }
    values.swap(i, j);
    values[i + 1..].reverse();
    true
}
fn symmetry_permutations<L, N>(
    eg: &EGraph<L, N>,
    id: slotted_egraphs::Id,
    slots: &[Slot],
) -> Option<Vec<SlotMap>>
where
    L: Language + Debug,
    N: Analysis<L>,
{
    if slots.len() > 8 {
        return None;
    }
    let base = AppliedId::new(id, SlotMap::identity(&eg.slots(id)));
    let mut current = slots.to_vec();
    let mut out = vec![];
    loop {
        if current != slots {
            let pairs = slots
                .iter()
                .copied()
                .zip(current.iter().copied())
                .collect::<Vec<_>>();
            let map = SlotMap::from_pairs(&pairs);
            if eg.eq(&base, &AppliedId::new(id, map.clone())) {
                out.push(map);
            }
        }
        if !next_permutation(&mut current) {
            break;
        }
    }
    Some(out)
}
/// Render all live e-classes. Blue clusters are e-classes, gray boxes are
/// e-nodes, gray edges are AppliedId child mappings, and yellow diamonds are
/// non-identity slot permutations proven equivalent by the original egraph.
pub fn to_dot<L, N>(eg: &EGraph<L, N>) -> String
where
    L: Language + Debug,
    N: Analysis<L>,
{
    let mut out = String::from(
        "digraph SlottedEGraph {\n  rankdir=TB;\n  graph [pad=0.2, nodesep=0.25, ranksep=0.55];\n  node [fontname=Helvetica, fontsize=10];\n",
    );
    for id in eg.ids() {
        let mut slots = eg.slots(id).into_iter().collect::<Vec<_>>();
        slots.sort();
        out.push_str(&format!("  subgraph cluster_eclass_{} {{\n    color=\"#4c78a8\"; style=rounded; label=\"EClass {:?} slots {{{}}}\";\n",id.0,id,esc(slots.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))));
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
        match symmetry_permutations(eg,id,&slots) {
            Some(perms)=>{
                let anchor=format!("class_anchor_{}",id.0);out.push_str(&format!("    {anchor} [shape=point, width=0.05, label=\"\"];\n"));
                for (i,perm) in perms.iter().enumerate(){let p=format!("perm_{}_{}",id.0,i);out.push_str(&format!("    {p} [shape=diamond, style=filled, fillcolor=\"#f6d55c\", label=\"{}\"];\n",esc(format!("perm {:?}",perm))));out.push_str(&format!("    {anchor} -> {p} [dir=none, style=dashed, color=\"#c28e00\"];\n"));}
            }
            None=>out.push_str(&format!("    group_note_{} [shape=note, label=\"symmetry enumeration skipped: {} slots\"];\n",id.0,slots.len())),
        }
        out.push_str("  }\n");
    }
    out.push_str("  legend [shape=note, label=\"blue cluster = e-class\\ngray box = slotted e-node\\ngray edge = AppliedId SlotMap\\nyellow diamond = proven non-identity slot permutation\"];\n}\n");
    out
}
