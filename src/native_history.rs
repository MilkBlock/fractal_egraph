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
    sort: String,
    label: String,
    external: bool,
}
#[derive(Serialize, Deserialize)]
struct History {
    version: u32,
    complete: bool,
    source: String,
    datatype: String,
    rules: Vec<RuleData>,
    tokens: Vec<TokenData>,
    records: Vec<Record>,
    events: usize,
    rounds: Option<usize>,
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
        version: u32,
        complete: bool,
        source: String,
        datatype: &'a str,
        rules: Vec<RuleData>,
        tokens: Vec<TokenData>,
        records: &'a [Record],
        events: usize,
        rounds: Option<usize>,
    }
    let history = Borrowed {
        version: 1,
        complete: true,
        source: source.display().to_string(),
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
            .map(|t| TokenData {
                sort: t.sort.to_string(),
                label: t.label(),
                external: matches!(t.key, Key::External(..) | Key::Replay(_, true)),
            })
            .collect(),
        records: &c.records,
        events: c.events,
        rounds: c.rounds,
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
            let expr = crate::visual_rule::expression_at(rule, &path)
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
    Ok(Captured {
        datatype: h.datatype,
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
