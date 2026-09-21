//! Portable resolved application history, not a raw kernel trace.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter, Write};

#[derive(Serialize, Deserialize)]
struct RuleData {
    code: String,
    // Preserve the source-span keys used by Input/Output without relying on
    // parser allocation or the source file still existing during replay.
    calls: Vec<(String, String)>,
}
#[derive(Serialize, Deserialize)]
struct TokenData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    literal: Option<String>,
    sort: String,
    label: String,
    external: bool,
}
#[derive(Serialize, Deserialize)]
struct History {
    #[serde(default)]
    changes: Vec<crate::closure_contract::Change>,
    version: u32,
    complete: bool,
    source: String,
    #[serde(default)]
    source_text: Option<String>,
    datatype: String,
    rules: Vec<RuleData>,
    tokens: Vec<TokenData>,
    records: Vec<Record>,
    events: usize,
    rounds: Option<usize>,
    #[serde(default)]
    boundaries: Vec<CaptureBoundary>,
}

pub(super) fn save(path: &Path, source: &Path, c: &Captured) -> Result {
    // Serialize directly to a buffered file: never materialize the history as
    // a serde_json::Value or one giant JSON string.
    let partial = path.with_extension("json.partial");
    if path.exists() {
        return Err("history output already exists".into());
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let mut out = BufWriter::new(file);
    #[derive(Serialize)]
    struct Borrowed<'a> {
        changes: &'a [crate::closure_contract::Change],
        version: u32,
        complete: bool,
        source: String,
        source_text: &'a str,
        datatype: &'a str,
        rules: Vec<RuleData>,
        tokens: Vec<TokenData>,
        records: &'a [Record],
        events: usize,
        rounds: Option<usize>,
        boundaries: &'a [CaptureBoundary],
    }
    let history = Borrowed {
        changes: &c.changes,
        version: 1,
        complete: true,
        source: source.display().to_string(),
        source_text: &c.preview_source,
        datatype: &c.datatype,
        rules: c
            .rules
            .iter()
            .map(|r| RuleData {
                code: Command::Rule {
                    rule: r.rule.clone(),
                }
                .to_string(),
                calls: r
                    .calls
                    .iter()
                    .map(|(s, (p, _))| (s.to_string(), p.clone()))
                    .collect(),
            })
            .collect(),
        tokens: c
            .pool
            .values
            .iter()
            .enumerate()
            .map(|(i, t)| TokenData {
                literal: c.pool.literals.get(&i).cloned(),
                sort: t.sort.to_string(),
                label: t.label(),
                external: matches!(t.key, Key::External(..) | Key::Replay(_, true)),
            })
            .collect(),
        records: &c.records,
        events: c.events,
        rounds: c.rounds,
        boundaries: &c.boundaries,
    };
    serde_json::to_writer(&mut out, &history)?;
    out.write_all(b"\n")?;
    out.flush()?;
    out.get_ref().sync_all()?;
    std::fs::rename(partial, path)?;
    Ok(())
}

pub(super) fn read(path: &Path) -> Result<Captured> {
    let mut h: History = serde_json::from_reader(BufReader::new(std::fs::File::open(path)?))?;
    if h.version != 1 || !h.complete {
        return Err("unsupported or incomplete history".into());
    }
    let mut previous = 0;
    for b in &h.boundaries {
        if b.end < previous || b.end > h.records.len() {
            return Err("invalid history round boundary".into());
        }
        previous = b.end;
    }
    if !h.boundaries.is_empty() && previous != h.records.len() {
        return Err("incomplete history round boundaries".into());
    }
    let mut eg = EGraph::default();
    let mut rules = vec![];
    let mut span_maps = vec![];
    for r in h.rules {
        let commands = eg.parse_program(None, &r.code)?;
        let [Command::Rule { rule }] = commands.as_slice() else {
            return Err("invalid history rule".into());
        };
        let positions: BTreeMap<_, _> = crate::visual_rule::positions(rule)
            .into_iter()
            .map(|(span, path, _)| (path, span))
            .collect();
        let mut span_map = BTreeMap::new();
        let mut calls = BTreeMap::new();
        for (span, path) in r.calls {
            let expr = crate::visual_rule::owned_expression_at(rule, &path)
                .ok_or("invalid history call path")?
                .clone();
            let new_span: Arc<str> = Arc::from(
                positions
                    .get(&path)
                    .ok_or("missing replay position")?
                    .as_str(),
            );
            span_map.insert(Arc::<str>::from(span), new_span.clone());
            calls.insert(new_span, (path, expr));
        }
        span_maps.push(span_map);
        rules.push(RuleInfo {
            rule: rule.clone(),
            calls,
        });
    }
    let pool = Pool {
        literals: h
            .tokens
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.literal.clone().map(|v| (i, v)))
            .collect(),
        ids: BTreeMap::new(),
        values: h
            .tokens
            .into_iter()
            .map(|t| Token {
                sort: Arc::from(t.sort),
                key: Key::Replay(t.label, t.external),
            })
            .collect(),
    };
    for change in &h.changes {
        if change.boundary == 0
            || (!h.boundaries.is_empty() && change.boundary > h.boundaries.len())
            || change.tokens.iter().any(|i| *i >= pool.values.len())
            || change.unions.iter().any(|(a, b)| {
                *a >= pool.values.len()
                    || *b >= pool.values.len()
                    || pool.values[*a].sort != pool.values[*b].sort
            })
        {
            return Err("invalid history mutation evidence".into());
        }
    }
    if h.changes.windows(2).any(|p| p[0].boundary > p[1].boundary) {
        return Err("out-of-order history mutation evidence".into());
    }
    if h.boundaries.is_empty() {
        for change in &mut h.changes {
            change.boundary = 1;
        }
    }
    let mut ids = BTreeSet::new();
    for (i, r) in h.records.iter().enumerate() {
        if !ids.insert(r.id)
            || r.rule >= rules.len()
            || r.parents.iter().any(|p| *p >= i)
            || r.ports.len() != r.wanted.len()
            || r.inputs.len() != r.wanted.len()
            || r.outputs.len() != r.values.len()
            || r.values
                .iter()
                .chain(&r.wanted)
                .chain(&r.external)
                .chain(&r.required)
                .chain(&r.produced)
                .chain(r.unions.iter().flat_map(|(a, b)| [a, b]))
                .any(|v| *v >= pool.values.len())
        {
            return Err(format!("invalid history record {i}").into());
        }
        for p in &r.ports {
            match p {
                Port::Parent(k, j)
                    if *k < r.parents.len() && *j < h.records[r.parents[*k]].values.len() => {}
                Port::External(k) if *k < r.external.len() => {}
                _ => return Err(format!("invalid history port at record {i}").into()),
            }
        }
    }
    for r in &mut h.records {
        let remap = |span: &mut Arc<str>| -> Result {
            *span = span_maps[r.rule]
                .get(span)
                .ok_or("missing replay source span")?
                .clone();
            Ok(())
        };
        for input in &mut r.inputs {
            if let Input::Read(span) = input {
                remap(span)?;
            }
        }
        for output in &mut r.outputs {
            match output {
                Output::Column(span, _) | Output::Row(span) => remap(span)?,
                _ => {}
            }
        }
    }
    let rejected = h
        .events
        .checked_sub(h.records.len())
        .ok_or("invalid history event count")?;
    // Replayed histories carry the datatype text; take its name as the sort name
    // the analysis uses, the same way a fresh capture does.
    let datatype_commands = eg.parse_program(None, &h.datatype)?;
    let datatype_name = declaration_sort(&datatype_commands)?;
    let preview_source = h.source_text.unwrap_or_else(|| {
        format!(
            "{}\n{}",
            h.datatype,
            rules
                .iter()
                .map(|r| r.rule.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    Ok(Captured {
        changes: h.changes,
        last_scope: 0,
        preview_source,
        boundaries: h.boundaries,
        layers: Default::default(),
        datatype: h.datatype,
        datatype_name,
        rules,
        pool,
        records: h.records,
        events: h.events,
        rounds: h.rounds,
        rejected,
        trace_seconds: 0.0,
        trace_peak: 0,
        trace_batches: 0,
    })
}
