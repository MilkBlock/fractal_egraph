//! Streaming debugger for the actual patched egglog runtime, not a match simulator.
use super::*;

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn math(e: &Expr) -> String {
    match e {
        Expr::Var(_, n) => format!("op({})", quote(n)),
        Expr::Lit(_, Literal::Int(n)) => n.to_string(),
        Expr::Lit(_, l) => format!("op({})", quote(&l.to_string())),
        Expr::Call(_, op, args) => {
            let a: Vec<_> = args.iter().map(math).collect();
            match (op.as_str(), a.as_slice()) {
                ("Add" | "+", [a, b]) => format!("({a} + {b})"),
                ("Mul" | "*", [a, b]) => format!("({a} dot {b})"),
                ("Sub" | "-", [a, b]) => format!("({a} - {b})"),
                ("Div" | "/", [a, b]) => format!("frac({a}, {b})"),
                ("Pow", [a, b]) => format!("({a})^({b})"),
                _ => format!("op({})({})", quote(op), a.join(", ")),
            }
        }
    }
}
fn render(rule: &Rule) -> Json {
    fn node(e: &Expr, lines: &mut Vec<String>, next: &mut usize) -> usize {
        let id = *next;
        *next += 1;
        let label = match e {
            Expr::Call(_, op, _) => op.clone(),
            _ => e.to_string(),
        };
        lines.push(format!("n{id} [label={}];", quote(&label)));
        if let Expr::Call(_, _, args) = e {
            for (slot, a) in args.iter().enumerate() {
                let child = node(a, lines, next);
                lines.push(format!("n{id} -> n{child} [label=\"{slot}\"] ;"));
            }
        }
        id
    }
    let mut lines = vec!["digraph pattern { rankdir=LR;".into()];
    let mut next = 0;
    lines.push("subgraph cluster_match { label=\"match / pattern\";".into());
    let mut body = vec![];
    for f in &rule.body {
        match f {
            Fact::Eq(_, a, b) => {
                body.push(format!("{} = {}", math(a), math(b)));
                let x = node(a, &mut lines, &mut next);
                let y = node(b, &mut lines, &mut next);
                lines.push(format!("n{x} -> n{y} [label=\"=\",dir=none,style=dashed];"));
            }
            Fact::Fact(e) => {
                body.push(math(e));
                node(e, &mut lines, &mut next);
            }
        }
    }
    lines.push("} subgraph cluster_effects { label=\"effects\";".into());
    let mut head = vec![];
    for a in &rule.head.0 {
        match a {
            Action::Union(_, a, b) => {
                head.push(format!("{} equiv {}", math(a), math(b)));
                let x = node(a, &mut lines, &mut next);
                let y = node(b, &mut lines, &mut next);
                lines.push(format!("n{x} -> n{y} [label=\"union\",color=blue];"));
            }
            Action::Let(_, n, e) => {
                head.push(format!("op({}) := {}", quote(n), math(e)));
                node(e, &mut lines, &mut next);
            }
            Action::Expr(_, e) => {
                head.push(math(e));
                node(e, &mut lines, &mut next);
            }
            _ => {
                head.push(format!("op({})", quote(&a.to_string())));
                lines.push(format!(
                    "n{next} [shape=box,label={}];",
                    quote(&a.to_string())
                ));
                next += 1;
            }
        }
    }
    lines.push("} }".into());
    json!({"typst":format!("#set page(width: auto, height: auto, margin: 12pt)\n$ {} ==> {} $",body.join(" and "),head.join(" quad ")),"dot":lines.join("\n"),"source":rule.to_string()})
}
fn location(rule: &Rule) -> (usize, usize) {
    if let Span::Egglog(s) = &rule.span {
        (
            s.file.get_location(s.i).0,
            s.file.get_location(s.j.saturating_sub(1)).0,
        )
    } else {
        (1, 1)
    }
}
// A staged composition preserves every dependency and intermediate effect even when
// native_lower cannot legally flatten it into a single endpoint rewrite.
fn composition(c: &Captured, index: usize) -> Json {
    let mut steps = BTreeSet::new();
    let mut todo = vec![index];
    while let Some(i) = todo.pop() {
        if steps.insert(i) {
            todo.extend(&c.records[i].parents);
        }
    }
    let mut dot = vec!["digraph compose { rankdir=LR;".to_owned()];
    let mut typst = "#set page(width: auto, height: auto, margin: 12pt)\n".to_owned();
    let mut source = vec![];
    let mut details = vec![];
    for i in &steps {
        let r = &c.records[*i];
        let rule = &c.rules[r.rule].rule;
        let formula = render(rule)["typst"]
            .as_str()
            .unwrap()
            .split_once('\n')
            .unwrap()
            .1
            .to_owned();
        typst += &format!(
            "$ op({}) $\n\n{}\n\n",
            quote(&format!("event {} · {}", r.id, rule.name)),
            formula
        );
        let bindings: Vec<_> = r
            .inputs
            .iter()
            .zip(&r.wanted)
            .map(|(input, v)| format!("{input:?} = {}", c.pool.values[*v].label()))
            .collect();
        for binding in &bindings {
            typst += &format!("$ op({}) $\n\n", quote(binding));
        }
        let (source_line, end_line) = location(rule);
        // A step is coarse when it takes an input from outside the chain (or has no
        // parent at all); only smooth steps can repeat on their own. A dissipative
        // fractal rule is one whose repetitions are coarse: it keeps firing only
        // because an external effect keeps arriving, so say which routes those are.
        details.push(json!({
            "event": r.id,
            "rule": rule.name,
            "source_line": source_line,
            "end_line": end_line,
            "binding": bindings,
            "ports": r.ports,
            "kind": if c.is_coarse(r) { "coarse" } else { "smooth" },
            "external_routes": r.ports.iter().filter_map(|p| match p { Port::External(k) => Some(*k), _ => None }).collect::<Vec<_>>(),
            // The same coarse ports by the input they consume: a `Var` name or the
            // read's source span, which is what a reader can act on.
            "external_inputs": r.wanted.iter().enumerate().filter_map(|(slot, _)| match r.ports.get(slot) {
                Some(Port::External(_)) => r.inputs.get(slot).map(|input| match input {
                    Input::Var(name) => name.clone(),
                    // A read is identified by where it was written; the span is the
                    // part of the trace a reader can find in the source.
                    Input::Read(text) => match text.rfind("\"):") {
                        Some(at) => format!("L{}", &text[at + 3..]),
                        None => text.to_string(),
                    },
                }),
                _ => None,
            }).collect::<Vec<_>>(),
            "produced": r.produced.iter().map(|v| c.pool.values[*v].label()).collect::<Vec<_>>(),
            "unions": r.unions
        }));
        dot.push(format!(
            "e{} [label={}];",
            r.id,
            quote(&format!("{} · {}\n{}", r.id, rule.name, rule))
        ));
        for (slot, p) in r.parents.iter().enumerate() {
            dot.push(format!(
                "e{} -> e{} [label={}];",
                c.records[*p].id,
                r.id,
                quote(&format!("parent {slot}: {:?}", r.ports))
            ));
        }
        source.push(format!("; event {}\n{}", r.id, rule));
    }
    dot.push("}".into());
    json!({"typst":typst,"dot":dot.join("\n"),"source":source.join("\n"),"steps":steps.iter().map(|i|c.records[*i].id).collect::<Vec<_>>(),"step_details":details})
}
/// Answers only "can the pinned kernel parse this?", with no preview or analysis work.
///
/// `patterns` is not a usable substitute: it also builds a preview and therefore rejects
/// subsuming rewrites, which parse and run fine. Callers that need to tell "this program uses
/// syntax we do not have" apart from "we cannot preview this" must use this.
pub fn parse_check(source: &str) -> Result<Json> {
    let mut eg = EGraph::default();
    let commands = eg.parse_program(None, source)?;
    Ok(json!({"commands": commands.len()}))
}
/// Source ranges come from egglog's parser (including multi-line rules and Unicode).
pub fn patterns(source: &str) -> Result<Json> {
    let mut eg = EGraph::default();
    let mut rows = vec![];
    for command in crate::visual_rule::surface_program(eg.parse_program(None, source)?) {
        // `birewrite` desugars to both directions and the transpiler emits no
        // `add_rule` scope for it; preview its forward rewrite like a plain rewrite.
        let command = match command {
            Command::BiRewrite(ruleset, rewrite) => Command::Rewrite(ruleset, rewrite, false),
            c => c,
        };
        // Keep subsume explicit instead of normalizing it into a union.
        let command = match command {
            Command::Rewrite(_, ref r, true) => {
                return Err(format!("subsuming rewrite preview is unsupported: {}", r.lhs).into());
            }
            c => crate::visual_rule::normalize(c, rows.len()),
        };
        if let Command::Rule { mut rule } = command {
            if rule.name.is_empty() {
                rule.name = format!("R{}", rows.len());
            }
            let (start, end) = location(&rule);
            let mut row = render(&rule);
            row["source_line"] = json!(start);
            row["end_line"] = json!(end);
            row["rule"] = json!(rule.name);
            rows.push(row);
        }
    }
    Ok(json!({"patterns":rows}))
}
/// Retain tier-1/tier-2 state across completed execution boundaries and emit only new evidence.
pub fn stream(root: &Path, source: &Path, emit: &mut dyn FnMut(Json) -> Result) -> Result {
    let text = std::fs::read_to_string(source)?;
    stream_source(root, &source.to_string_lossy(), &text, emit)
}

/// Same as `stream`, but for source text already in memory; the wasm build has no
/// filesystem and calls this directly.
pub fn stream_source(
    root: &Path,
    name: &str,
    text: &str,
    emit: &mut dyn FnMut(Json) -> Result,
) -> Result {
    let mut eg = EGraph::default();
    let mut inserted = (0, 0);
    let mut tier2 = Tier2State::default();
    let mut seen = BTreeSet::new();
    let mut snapshot_count = 0;
    #[cfg(not(target_arch = "wasm32"))]
    let ripen_output = std::env::var_os("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("out/debug-stream").join(format!("{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    #[cfg(not(target_arch = "wasm32"))]
    let mut saturated_rule_composition = Some(saturated_rule_composition_pipeline::Pipeline::new(
        &ripen_output,ripen_output.join("source.egg").display().to_string(),
    )?);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("[ripen] default local saturation output: {}",ripen_output.display());
    let mut layer_analyzer = crate::layer_patterns::Analyzer::default();
    // rayon has no threads on wasm32-unknown-unknown; the pool only sizes the stack
    // natively, so the browser build runs the analysis inline.
    #[cfg(not(target_arch = "wasm32"))]
    let worker = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
    capture_text_with_sink(
        name,
        text,
        None,
        None,
        Some(&mut |c| {
            let boundary = c.boundaries.len();
            let start = inserted.1;
            let mut analyse = || -> std::result::Result<_, String> {
                build_tier1(c, &mut eg, root, &mut inserted).map_err(|e| e.to_string())?;
                update_tier2(c, &mut eg, root, &mut tier2).map_err(|e| e.to_string())
            };
            #[cfg(not(target_arch = "wasm32"))]
            let outcome = worker.install(analyse);
            #[cfg(target_arch = "wasm32")]
            let outcome = analyse();
            let (ext, higher) = outcome.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            for index in start..c.records.len() {
                let r = &c.records[index];
                let rule = &c.rules[r.rule].rule;
                let (line, end) = location(rule);
                let mut row = render(rule);
                row["kind"] = json!("application");
                row["id"] = json!(format!("apply:{}", r.id));
                row["event"] = json!(r.id);
                if let Some(o) = c.layers.occurrence(r.id) {
                    row["layer"] = json!({"comb":o.comb,"coarse":o.coarse_layer,
                        "smooth":o.smooth_layer,"boundary_restart":o.boundary_restart});
                }
                row["boundary"] = json!(boundary);
                row["rule"] = json!(rule.name);
                row["source_line"] = json!(line);
                row["end_line"] = json!(end);
                row["binding"] = json!(
                    r.inputs
                        .iter()
                        .zip(&r.wanted)
                        .map(|(i, v)| format!("{i:?} = {}", c.pool.values[*v].label()))
                        .collect::<Vec<_>>()
                );
                row["parents"] = json!(
                    r.parents
                        .iter()
                        .map(|p| c.records[*p].id)
                        .collect::<Vec<_>>()
                );
                row["effects"] = json!({"produced":r.produced.iter().map(|v|c.pool.values[*v].label()).collect::<Vec<_>>(),"unions":r.unions});
                let bindings = row["binding"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| format!("$ op({}) $", quote(v.as_str().unwrap())))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                row["typst"] = json!(format!(
                    "{}\n\n$ op({}) $\n\n{}",
                    row["typst"].as_str().unwrap(),
                    quote(&format!("event {}", r.id)),
                    bindings
                ));
                emit(row.clone())?;
                if !r.parents.is_empty() {
                    row["kind"] = json!("compose");
                    row["id"] = json!(format!("compose:{}", r.id));
                    let staged = composition(c, index);
                    for key in ["typst", "dot", "source", "steps", "step_details"] {
                        row[key] = staged[key].clone();
                    }
                    row["composition_mode"] = json!("staged dependency DAG");
                    match crate::native_lower::lower(&c.records, &c.rules, index).and_then(
                        |lowered| {
                            let mut check = EGraph::default();
                            check
                                .parse_and_run_program(
                                    None,
                                    &format!("{}\n{}", c.datatype, lowered.code),
                                )
                                .map_err(|e| e.to_string())?;
                            Ok(lowered)
                        },
                    ) {
                        Ok(lowered) => {
                            let parsed = patterns(&lowered.code)?;
                            if let Some(p) = parsed["patterns"].as_array().and_then(|a| a.first()) {
                                for key in ["typst", "dot", "source"] {
                                    row[key] = p[key].clone();
                                }
                                row["composition_mode"] = json!("flattened rule");
                            }
                            row["steps"] = json!(
                                lowered
                                    .steps
                                    .iter()
                                    .map(|i| c.records[*i].id)
                                    .collect::<Vec<_>>()
                            );
                        }
                        Err(reason) => {
                            row["reason"] = json!(format!("保留分阶段组合：{reason}"));
                        }
                    }
                    emit(row)?;
                }
            }
            let v = view(c, &eg, &ext, &higher)?;
            for lane in v["lanes"].as_array().unwrap() {
                let key = format!("{}:{}", lane["trigger"], lane["events"]);
                if !seen.insert(key.clone()) {
                    continue;
                }
                let endpoint = lane["events"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()
                    .as_u64()
                    .unwrap();
                let ep = c.records.iter().position(|r| r.id == endpoint).unwrap();
                let r = &c.records[ep];
                let rule = &c.rules[r.rule].rule;
                let (line, end) = location(rule);
                let mut row =
                    composition(c, c.records.iter().position(|r| r.id == endpoint).unwrap());
                row["kind"] = json!("fractal");
                row["id"] = json!(format!("fractal:{key}"));
                row["boundary"] = json!(boundary);
                row["rule"] = json!(rule.name);
                row["source_line"] = json!(line);
                row["end_line"] = json!(end);
                // The coarse lane rule: one step from the trigger state to the last
                // state, with every intermediate read fused away. `view` builds the
                // same node; carry it here so a lane can be shown both ways, and
                // report why it is missing when native_lower refuses the shape.
                let mut lane = lane.clone();
                lane["coarse"] = match crate::native_lower::lower(&c.records, &c.rules, ep) {
                    Ok(lowered) => {
                        let mut check = EGraph::default();
                        match check.parse_and_run_program(
                            None,
                            &format!("{}\n{}", c.datatype, lowered.code),
                        ) {
                            Ok(_) => json!({
                                "code": lowered.code,
                                "steps": lowered.steps.iter().map(|i| c.rules[c.records[*i].rule].rule.name.clone()).collect::<Vec<_>>(),
                            }),
                            Err(error) => json!({"reason": format!("合法性检查失败：{error}")}),
                        }
                    }
                    Err(reason) => json!({"reason": reason}),
                };
                row["evidence"] = lane.clone();
                // The lane itself is the visualization input: the debugger appends
                // the repetition (depth, operator, context, update map, witness) to
                // the plugin's formula. Keep `typst`/`dot` as the staged
                // composition they already are instead of hand-building a second
                // formula here.
                let graph = row["dot"].as_str().unwrap().trim_end_matches('}');
                row["dot"] = json!(format!(
                    "{graph}\nf [shape=box,color=blue,label={}]; f -> e{endpoint} [style=dashed,label=\"Represents\"];\n}}",
                    quote(lane["higher"].as_str().unwrap())
                ));
                emit(row)?;
            }
            if snapshot_count < c.boundaries.len() {
                snapshot_count = c.boundaries.len();
                let mut frame = layer_view::snapshot(
                    &c.layers,
                    c.boundaries.last().unwrap(),
                    snapshot_count,
                    &c.preview_source,
                    &mut layer_analyzer,
                )?;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(pipeline) = &mut saturated_rule_composition {
                    frame["ripen_output_directory"] = json!(ripen_output);
                    frame["saturated_rule_composition"] = pipeline.step(c, &c.layers, snapshot_count)?;
                    if let Some(dot) = frame["saturated_rule_composition"]["catalog"]["dot"].as_str() {
                        frame["dots"]["saturated_rule_composition"] = json!(dot);
                    }
                }
                emit(frame)?;
            }
            emit(
                json!({"kind":"boundary","boundary":boundary,"applications":c.records.len(),"logical_matches":c.events,"excluded":c.rejected}),
            )?;
            Ok(())
        }),
    )?;
    emit(json!({"kind":"complete"}))
}
