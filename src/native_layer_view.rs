//! Rust CS frames; no template mining or secondary egraph interpreter.
use super::*;
use crate::coarse_smooth::{CombKind, LayerStore, RelativeBinding};
use std::fmt::Write as _;
fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn name(s: &str) -> String {
    s.rsplit_once(":name ")
        .map(|(_, n)| n.trim_end_matches(')').trim().trim_matches('"').to_string())
        .unwrap_or_else(|| s.chars().take(70).collect())
}
fn node(id: String, label: String, kind: &str, detail: Json) -> Json {
    json!({"id":id,"label":label,"kind":kind,"detail":detail})
}
fn edge(from: String, to: String, label: String) -> Json {
    json!({"from":from,"to":to,"label":label})
}
fn graph(nodes: Vec<Json>, edges: Vec<Json>) -> Json {
    json!({"nodes":nodes,"edges":edges})
}
fn dots(title: &str, g: &Json) -> String {
    let mut s = format!(
        "digraph G {{\n  graph [rankdir=TB, label={}, labelloc=t];\n  node [shape=box, style=filled, fontname=Helvetica];\n",
        quoted(title)
    );
    for n in g["nodes"].as_array().unwrap() {
        let color = match n["kind"].as_str().unwrap_or("") {
            "coarse" => "#fce6c9",
            "smooth" => "#dff2ec",
            "fractal" => "#e9e1fa",
            "trigger" => "#fbd5d0",
            _ => "#edf0f5",
        };
        writeln!(
            s,
            "  {} [label={}, fillcolor={}, tooltip={}];",
            quoted(n["id"].as_str().unwrap()),
            quoted(n["label"].as_str().unwrap()),
            quoted(color),
            quoted(n["id"].as_str().unwrap())
        )
        .unwrap();
    }
    if let Some(groups) = g["groups"].as_array() {
        for group in groups
            .iter()
            .filter(|g| g["id"].as_str().unwrap().starts_with('s'))
        {
            writeln!(
                s,
                "  subgraph {} {{ label={}; color=\"#b6dacf\";",
                quoted(&format!("layer_{}", group["id"].as_str().unwrap())),
                quoted(group["label"].as_str().unwrap())
            )
            .unwrap();
            for member in group["members"].as_array().unwrap() {
                writeln!(s, "    {};", quoted(member.as_str().unwrap())).unwrap();
            }
            s.push_str("  }\n");
        }
    }
    for e in g["edges"].as_array().unwrap() {
        writeln!(
            s,
            "  {} -> {} [label={}];",
            quoted(e["from"].as_str().unwrap()),
            quoted(e["to"].as_str().unwrap()),
            quoted(e["label"].as_str().unwrap())
        )
        .unwrap();
    }
    s.push_str("}\n");
    s
}
fn layer_graph(s: &LayerStore) -> Json {
    let mut nodes = vec![];
    let mut edges = vec![];
    for (i, o) in s.occurrences.iter().enumerate() {
        let kind = if matches!(s.combs[o.comb].kind, CombKind::CoarseRuleComposition) {
            "coarse"
        } else {
            "smooth"
        };
        nodes.push(node(format!("a{i}"),format!("{} · event {}\nC{} / S{}",name(&o.apply.rule),o.apply.event,o.coarse_layer,o.smooth_layer.map(|x|x.to_string()).unwrap_or_else(||"—".into())),kind,
            json!({"occurrence":i,"event":o.apply.event,"source":o.apply.rule,"coarse_layer":s.coarse_layers[o.coarse_layer],"smooth_layer":o.smooth_layer,"boundary_restart":o.boundary_restart,"binding":o.apply.binding,"inputs":o.apply.input_roles,"outputs":o.apply.output_roles,"value_tokens":o.apply.wanted,"requires":o.apply.required,"effects":o.apply.produced})));
        let mut used = BTreeSet::new();
        for (input, p) in o.apply.binding.iter().enumerate() {
            if let RelativeBinding::ParentPort { parent, output } = p {
                used.insert(*parent);
                edges.push(edge(
                    format!("a{}", o.apply.parents[*parent]),
                    format!("a{i}"),
                    format!("out{output} → in{input}"),
                ));
            }
        }
        for (slot, p) in o.apply.parents.iter().enumerate() {
            if !used.contains(&slot) {
                edges.push(edge(
                    format!("a{p}"),
                    format!("a{i}"),
                    "effect/context".into(),
                ));
            }
        }
    }
    if let Some(ripen) = &s.ripen {
        nodes.push(node(
            "ripen-cell".into(),
            format!(
                "Ripen {} · round {}\nwhole local cell",
                ripen.state, ripen.round
            ),
            "template",
            json!(ripen),
        ));
        for (i, _o) in s
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, o)| o.apply.parents.is_empty())
        {
            edges.push(edge(
                "ripen-cell".into(),
                format!("a{i}"),
                "observed entry".into(),
            ));
        }
    }
    // Joint coarse interfaces are metadata, not extra executed applications.
    for (i, c) in s
        .coarse_layers
        .iter()
        .enumerate()
        .filter(|(_, c)| c.members.len() > 1)
    {
        nodes.push(node(
            format!("c{i}"),
            format!(
                "CoarseLayer C{i}\n{} members · shared interface",
                c.members.len()
            ),
            "coarse",
            json!(c),
        ));
        for m in &c.members {
            edges.push(edge(format!("a{m}"), format!("c{i}"), "member".into()));
        }
        if let Some(w) = c.witness {
            edges.push(if c.members.contains(&w) {
                edge(format!("a{w}"), format!("c{i}"), "coarse promotion".into())
            } else {
                edge(format!("c{i}"), format!("a{w}"), "joint-use witness".into())
            });
        }
    }
    let mut g = graph(nodes, edges);
    let mut groups = vec![];
    for (i, c) in s.coarse_layers.iter().enumerate() {
        let mut members: Vec<_> = c.members.iter().map(|m| format!("a{m}")).collect();
        if c.members.len() > 1 {
            members.push(format!("c{i}"));
        }
        groups.push(
            json!({"id":format!("c{i}"),"label":format!("CoarseLayer C{i}"),"members":members}),
        );
    }
    for (i, l) in s.smooth_layers.iter().enumerate() {
        groups.push(json!({"id":format!("s{i}"),"label":format!("SmoothLayer S{i}"),"members":l.members.iter().map(|m|format!("a{m}")).collect::<Vec<_>>() }));
    }
    g["groups"] = json!(groups);
    g
}

pub(super) fn snapshot(
    s: &LayerStore,
    b: &CaptureBoundary,
    index: usize,
    source: &str,
) -> Result<Json> {
    if !crate::graph_render::graphs_enabled() {
        return Ok(
            json!({"kind":"layer_snapshot","id":format!("layer-{index}"),"stem":format!("round-{index:04}"),"label":format!("{} {}", b.kind,b.round.unwrap_or(index)),"boundary":index,"boundary_kind":b.kind,"round":b.round,"end":b.end,"preview_source":source,"graphs_enabled":false,"counts":{"applications":s.occurrences.len(),"templates":0,"fractals":0},"graphs":{},"dots":{},"analysis":{"scope":"observed CS interfaces only","templates":[],"fractals":[]},"sites":[]}),
        );
    }
    let g = layer_graph(s);
    let label = format!("{} {}", b.kind, b.round.unwrap_or(index));
    Ok(
        json!({"kind":"layer_snapshot","id":format!("layer-{index}"),"stem":format!("round-{index:04}"),"label":label,"boundary":index,"boundary_kind":b.kind,"round":b.round,"end":b.end,"preview_source":source,"counts":{"applications":s.occurrences.len(),"templates":0,"fractals":0},"graphs":{"layers":g},"dots":{"layers":dots(&label,&g)},"analysis":{"scope":"observed CS interfaces only","templates":[],"fractals":[]},"sites":debug::patterns(source)?["patterns"]}),
    )
}
pub(super) fn persist(out: &Path, frame: &Json, manifest: &mut Vec<Json>) -> Result {
    let dir = out.join("rounds");
    std::fs::create_dir_all(&dir)?;
    let stem = frame["stem"].as_str().unwrap();
    std::fs::write(dir.join(format!("{stem}.json")), serde_json::to_vec(frame)?)?;
    for (kind, dot) in frame["dots"].as_object().unwrap() {
        std::fs::write(
            dir.join(format!("{stem}.{kind}.dot")),
            dot.as_str().unwrap_or(""),
        )?;
    }
    manifest.push(json!({"stem":stem,"label":frame["label"]}));
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(manifest)?)?;
    Ok(())
}
