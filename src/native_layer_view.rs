//! Per-boundary DOT/data. The existing tools debugger owns SVG/Typst rendering.
use super::*;
use crate::{
    coarse_smooth::{CombKind, LayerStore, RelativeBinding},
    layer_patterns::{self, Analysis},
};
use std::fmt::Write as _;
#[derive(Default)]
pub(super) struct Exporter {
    frames: Vec<Json>,
    analyzer: layer_patterns::Analyzer,
}
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
        "digraph G {{\n  graph [rankdir=LR, label={}, labelloc=t];\n  node [shape=box, style=filled, fontname=Helvetica];\n",
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
        let kind = if matches!(s.combs[o.comb].kind, CombKind::CoarseComb) {
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
            edges.push(edge(
                format!("c{i}"),
                format!("a{w}"),
                "joint-use witness".into(),
            ));
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
fn fractal_graph(s: &LayerStore, a: &Analysis) -> Json {
    let mut nodes = vec![];
    let mut edges = vec![];
    for (f, fam) in a.fractals.iter().enumerate() {
        let t = &a.templates[fam.template];
        let rules = t
            .interface
            .members
            .iter()
            .map(|m| name(&m.rule))
            .collect::<Vec<_>>()
            .join(" → ");
        nodes.push(node(format!("f{f}"),format!("FractalComb F{f} · T{}\n{rules}\nobserved depth {} · {} return ports",fam.template,fam.max_observed_depth,t.interface.returns.len()),"fractal",json!({"family":fam,"interface":t.interface,"proof":"finite witnesses only; arbitrary n unknown"})));
        for entry in &fam.triggers {
            let parents = &s.occurrences[*entry].apply.parents;
            nodes.push(node(format!("f{f}t{entry}"),format!("trigger/context\nentry event {}",s.occurrences[*entry].apply.event),"trigger",json!({"entry_event":s.occurrences[*entry].apply.event,"context_events":parents.iter().map(|p|s.occurrences[*p].apply.event).collect::<Vec<_>>(),"external_values":s.occurrences[*entry].apply.external,"external_facts":s.occurrences[*entry].apply.external_facts})));
            edges.push(edge(
                format!("f{f}t{entry}"),
                format!("f{f}"),
                "inject/start".into(),
            ));
        }
        for u in &fam.units {
            let unit = &a.units[*u];
            nodes.push(node(format!("u{u}"),format!("unit {u}\nevent {} · {} applies",s.occurrences[unit.entry].apply.event,unit.members.len()),"smooth",json!({"unit":unit,"return_binding":t.interface.returns,"output_terms":t.interface.members.iter().map(|m|&m.output_terms).collect::<Vec<_>>(),"external_each_unit":fam.external_each_unit})));
            edges.push(edge(
                format!("f{f}"),
                format!("u{u}"),
                "observed instance".into(),
            ));
        }
        for (from, slot, to) in &fam.recursive_edges {
            edges.push(edge(
                format!("u{from}"),
                format!("u{to}"),
                format!("return {slot} / θ{slot}"),
            ));
        }
    }
    graph(nodes, edges)
}
fn coverage_graph(a: &Analysis) -> Json {
    let mut ids = BTreeSet::new();
    let mut edges = vec![];
    for c in &a.coverage {
        if c["status"] != "TemplateCoverage" {
            continue;
        }
        let small = c["small"].as_u64().unwrap() as usize;
        let big = c["big"].as_u64().unwrap() as usize;
        ids.extend([small, big]);
        edges.push(edge(
            format!("t{big}"),
            format!("t{small}"),
            "TemplateCoverage (not substitution)".into(),
        ));
    }
    let nodes=ids.into_iter().map(|i|node(format!("t{i}"),format!("T{i}\n{} apply members",a.templates[i].interface.members.len()),"template",json!({"template":a.templates[i],"relations":a.coverage.iter().filter(|c|c["small"]==i||c["big"]==i).collect::<Vec<_>>()}))).collect();
    graph(nodes, edges)
}
fn reuse_graph(s: &LayerStore) -> Json {
    use crate::comb_reuse::{Part, Reference};
    let r = &s.reuse;
    let mut nodes = vec![];
    let mut edges = vec![];
    let mut included = BTreeSet::new();
    let mut todo: Vec<Part> = r
        .active_uses
        .iter()
        .copied()
        .map(Part::Use)
        .chain(
            r.owner
                .iter()
                .enumerate()
                .filter_map(|(i, o)| o.is_none().then_some(Part::Residual(i))),
        )
        .collect();
    let source = |reference: &Reference| -> Option<(String, String)> {
        let (event, output) = match r.physical(reference)? {
            Reference::ResidualPort { event, output } => (event, Some(output)),
            Reference::ResidualContext(event) => (event, None),
            _ => return None,
        };
        let port = output
            .map(|o| format!("out{o}"))
            .unwrap_or_else(|| "context".into());
        Some(match r.owner[event] {
            Some((u, m)) => (format!("use{u}"), format!("m{m}.{port}")),
            None => (format!("res{event}"), port),
        })
    };
    while let Some(part) = todo.pop() {
        let (id, label, kind, detail, refs) = match part {
            Part::Use(i) => {
                let u = &r.uses[i];
                (
                    format!("use{i}"),
                    format!(
                        "Use(T{}) · U{i}\n{} applies · {} wiring units",
                        u.template,
                        u.members.len(),
                        u.wiring_cost
                    ),
                    "fractal",
                    json!({"instance":u,"definition":r.templates[u.template],"active":r.active_uses.contains(&i)}),
                    u.inputs
                        .iter()
                        .chain(&u.contexts)
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            }
            Part::Residual(i) => {
                let a = &s.occurrences[i].apply;
                let residual = &r.residuals[i];
                (
                    format!("res{i}"),
                    format!(
                        "{} · event {}\n{}",
                        name(&a.rule),
                        a.event,
                        if residual.coarse {
                            "CoarseComb residual"
                        } else {
                            "SmoothComb residual"
                        }
                    ),
                    if residual.coarse { "coarse" } else { "smooth" },
                    json!({"source":a.rule,"residual":residual,"covered":r.owner[i].is_some()}),
                    residual
                        .binding
                        .iter()
                        .chain(&residual.contexts)
                        .cloned()
                        .collect(),
                )
            }
        };
        if !included.insert(id.clone()) {
            continue;
        }
        nodes.push(node(id.clone(), label, kind, detail));
        for (slot, reference) in refs.iter().enumerate() {
            if let Some((from, port)) = source(reference) {
                edges.push(edge(
                    from,
                    id.clone(),
                    format!("{port} → input/context {slot}"),
                ));
            }
        }
        // The overview is the current cut. Historical parts remain in instance
        // details; rendering them here would resurrect removed cover regions.
    }

    graph(nodes, edges)
}

fn use_fractal_graph(s: &LayerStore, a: &Analysis) -> Json {
    let mut nodes = vec![];
    let mut edges = vec![];
    for (i, f) in a.use_fractals.families.iter().enumerate() {
        let id = format!("rf{i}");
        nodes.push(node(
            id.clone(),
            format!(
                "Use Fractal candidate F{i} / T{}\nobserved depth {} · {} transfers\n{}",
                f.template,
                f.observed_depth,
                f.transitions.len(),
                if f.external_demand_observed {
                    "external demand observed"
                } else {
                    "no fresh demand observed"
                }
            ),
            "fractal",
            json!(f),
        ));
        for u in &f.instances {
            let instance = &s.reuse.uses[*u];
            nodes.push(node(format!("{id}u{u}"), format!("Use(T{}) U{u}\n{} applies", instance.template, instance.members.len()),
                if f.triggers.contains(u) {"trigger"} else {"template"}, json!({"use":instance,"template":s.reuse.templates[instance.template],"observed_start":f.triggers.contains(u)})));
        }
        for u in &f.triggers {
            edges.push(edge(
                id.clone(),
                format!("{id}u{u}"),
                "observed start (not minimal trigger proof)".into(),
            ));
        }
        for [u, v, t] in &f.edges {
            edges.push(edge(
                format!("{id}u{u}"),
                format!("{id}u{v}"),
                format!("transfer {t}"),
            ));
        }
    }
    graph(nodes, edges)
}

/// Shared by native streaming, CLI online construction and history replay.
pub(super) fn snapshot(
    s: &LayerStore,
    b: &CaptureBoundary,
    index: usize,
    source: &str,
    analyzer: &mut layer_patterns::Analyzer,
) -> Result<Json> {
    s.reuse.verify(s)?;
    let a = analyzer.analyze(s);
    let label = b
        .round
        .map(|r| format!("Round {r}"))
        .unwrap_or_else(|| format!("{} {index}", b.kind));
    let graphs = json!({"layers":layer_graph(s),"fractals":fractal_graph(s,&a),"coverage":coverage_graph(&a),"reuse":reuse_graph(s),"use_fractals":use_fractal_graph(s,&a)});
    let mut dot = serde_json::Map::new();
    for kind in ["layers", "fractals", "coverage", "reuse", "use_fractals"] {
        dot.insert(
            kind.into(),
            json!(dots(&format!("{label} / {kind} (observed)"), &graphs[kind])),
        );
    }
    let parsed = debug::patterns(source)?;
    let sites: Vec<_> = parsed["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| json!({"source":r["source"],"rule":r["rule"],"source_line":r["source_line"]}))
        .collect();
    Ok(
        json!({"kind":"layer_snapshot","id":format!("layer-round:{index}"),"boundary":index,
        "label":label,"boundary_kind":b.kind,"round":b.round,"end":b.end,"stem":format!("round-{index:04}"),
        "counts":{"applications":s.occurrences.len(),"templates":a.templates.len(),"fractals":a.fractals.len(),"coverage_queries":a.coverage_queries,"coverage_cache_hits":a.coverage_cache_hits,"coverage_skipped":a.coverage_skipped,"truncated_candidates":a.truncated_candidates},
        "ripen":b.ripen,"reuse":s.reuse.report(),"graphs":graphs,"dots":dot,"analysis":a,"sites":sites,"preview_source":source}),
    )
}
impl Exporter {
    pub fn summary(&self) -> Json {
        self.frames.last().map(|f|json!({"latest":format!("rounds/{}.json",f["stem"].as_str().unwrap()),"counts":f["counts"],"snapshots":self.frames.len(),"proof":"observed finite recursion; arbitrary-depth induction unknown"})).unwrap_or(Json::Null)
    }
    fn save(&mut self, s: &LayerStore, b: &CaptureBoundary, out: &Path, source: &str) -> Result {
        let frame = snapshot(s, b, self.frames.len() + 1, source, &mut self.analyzer)?;
        let stem = frame["stem"].as_str().unwrap();
        let dir = out.join("rounds");
        std::fs::create_dir_all(&dir)?;
        for kind in ["layers", "fractals", "coverage", "reuse", "use_fractals"] {
            std::fs::write(
                dir.join(format!("{stem}.{kind}.dot")),
                frame["dots"][kind].as_str().unwrap(),
            )?;
        }
        std::fs::write(
            dir.join(format!("{stem}.json")),
            serde_json::to_vec(&frame)?,
        )?;
        self.frames.push(json!({"label":frame["label"],"kind":frame["boundary_kind"],"round":frame["round"],"end":frame["end"],"stem":frame["stem"],"counts":frame["counts"]}));
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&self.frames)?,
        )?;
        Ok(())
    }
    pub fn capture(&mut self, c: &mut Captured, out: &Path) -> Result {
        update_layers(c)?;
        if self.frames.len() < c.boundaries.len() {
            self.save(
                &c.layers,
                c.boundaries.last().unwrap(),
                out,
                &c.preview_source,
            )?;
        }
        Ok(())
    }
    pub fn replay(&mut self, c: &Captured, out: &Path) -> Result {
        let fallback = vec![CaptureBoundary {
            ripen: None,
            kind: "final-history-snapshot".into(),
            round: None,
            end: c.records.len(),
        }];
        let bs = if c.boundaries.is_empty() {
            &fallback
        } else {
            &c.boundaries
        };
        let mut s = LayerStore::default();
        for b in bs {
            for o in c
                .layers
                .occurrences
                .iter()
                .take(b.end)
                .skip(s.occurrences.len())
            {
                s.push(o.apply.clone())?;
            }
            s.ripen = b.ripen.clone();
            self.save(&s, b, out, &c.preview_source)?;
        }
        Ok(())
    }
}
