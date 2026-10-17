//! CPU owns native commits; the device owns instances, proofs, reverse watches,
//! fragment indexes and delta matching. Only numeric native events cross this ABI.
use super::*;
use egglog::cell_events::{CellCommitFeed, CellProducerEvent, CellProducerTap};
use serde_json::json;
use std::io::{BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const NONE: u32 = u32::MAX;
#[derive(Default, Serialize)]
pub struct ResidentStats {
    pub batches: usize,
    pub quiet_rounds: usize,
    pub row_deltas: usize,
    pub union_deltas: usize,
    pub producer_events: usize,
    pub uploaded_bytes: usize,
    pub downloaded_bytes: usize,
    pub cpu_cell_validations: usize,
    pub cpu_cell_point_reads: usize,
    pub diagnostic_trace_events: usize,
    pub dispatched_bindings: usize,
    pub native_matches: usize,
    pub setup_seconds: f64,
    pub native_seconds: f64,
    pub event_seconds: f64,
    pub device_roundtrip_seconds: f64,
    pub rhs_seconds: f64,
    pub audit_queries: usize,
    pub audit_uncovered_per_round: Vec<usize>,
    pub gpu: Json,
}
struct Device {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    device: String,
}
impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn write_json(w: &mut impl Write, v: &Json) -> Result<()> {
    let bytes = serde_json::to_vec(v)?;
    w.write_all(&(u32::try_from(bytes.len())?).to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()?;
    Ok(())
}
fn read_json(r: &mut impl Read) -> Result<Json> {
    let mut n = [0; 4];
    r.read_exact(&mut n)?;
    let n = u32::from_le_bytes(n) as usize;
    let mut bytes = vec![0; n];
    r.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
impl Device {
    fn new(worker: &Path, config: &Json) -> Result<Self> {
        let mut child = Command::new(worker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        write_json(&mut input, config)?;
        let ready = read_json(&mut output)?;
        if ready["ready"] != true {
            return Err(format!("resident GPU initialization failed: {ready}").into());
        }
        Ok(Self {
            child,
            input,
            output,
            device: ready["device"].as_str().unwrap().into(),
        })
    }
}

pub struct ResidentSession {
    eg: EGraph,
    sink: CellActionSink,
    ruleset: String,
    tap: CellProducerTap,
    feed: CellCommitFeed,
    events: Vec<CellProducerEvent>,
    device: Device,
    max_cols: usize,
    max_fields: usize,
    consumer_id: usize,
    epoch: u32,
    audit: bool,
    seen: BTreeSet<Vec<Value>>,
    description: Json,
    pub stats: ResidentStats,
}
impl ResidentSession {
    pub fn new(mut source: Session, worker: &Path) -> Result<Self> {
        if source.stats.epochs != 0 {
            return Err("attach resident executor before any input".into());
        }
        let t0 = Instant::now();
        let mut names: BTreeSet<String> = source
            .sink
            .atoms
            .iter()
            .map(|a| a.function.clone())
            .collect();
        for t in &source.templates {
            for r in t.state["rows"].as_array().unwrap() {
                let op = r["op"].as_str().unwrap();
                if !t.markers.contains(op) {
                    names.insert(op.into());
                }
            }
        }
        let names: Vec<_> = names.into_iter().collect();
        let (feed, pending) = source.eg.subscribe_cell_commits(&names)?;
        if !pending.rows.is_empty() || !pending.unions.is_empty() {
            return Err("attach resident executor to a fresh environment before input".into());
        }
        let table_id = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u32))
            .collect::<BTreeMap<_, _>>();
        let mut rule_names = source.native.clone();
        let consumer_id = rule_names.len();
        rule_names.push(source.sink.name.clone());
        let mut fields: Vec<Vec<String>> = rule_names
            .iter()
            .map(|r| {
                source
                    .templates
                    .iter()
                    .filter(|t| t.producer == *r)
                    .flat_map(|t| t.parameters.iter().map(|(_, n)| n.clone()))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .collect();
        fields[consumer_id] = source
            .sink
            .slots()
            .iter()
            .map(|(n, _)| n.to_string())
            .collect();
        let specs = source
            .native
            .iter()
            .cloned()
            .zip(fields.iter().cloned())
            .collect::<Vec<_>>();
        let tap = source.eg.prepare_cell_producers(&source.ruleset, &specs)?;
        let k = source.sink.slots().len().max(1);
        let max_cols = feed
            .tables
            .iter()
            .map(|t| t.eq_columns.len())
            .max()
            .unwrap()
            .max(1);
        let max_vals = source
            .templates
            .iter()
            .map(|t| t.state["values"].as_array().unwrap().len())
            .max()
            .unwrap()
            .max(1);
        let max_params = source
            .templates
            .iter()
            .map(|t| t.parameters.len())
            .max()
            .unwrap()
            .max(1);
        let reads = source
            .templates
            .iter()
            .map(|t| {
                t.state["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| !t.markers.contains(r["op"].as_str().unwrap()))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let max_reads = reads.iter().map(Vec::len).max().unwrap().max(1);
        let max_lenses = source
            .lenses
            .iter()
            .map(|ps| ps.iter().map(Vec::len).sum::<usize>())
            .max()
            .unwrap()
            .max(1);
        let max_alias = source
            .lenses
            .iter()
            .flatten()
            .flatten()
            .map(|l| l.aliases.len())
            .max()
            .unwrap_or(0)
            .max(1);
        let max_fields = fields.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let mut program = vec![];
        let mut offsets = BTreeMap::new();
        offsets.insert("TABLE_OFF", program.len());
        for (i, t) in feed.tables.iter().enumerate() {
            let ext: Vec<_> = source
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.cell && source.sink.atoms[p.atoms[0]].function == names[i])
                .map(|(p, _)| p as u32)
                .collect();
            program.extend([t.keys as u32, t.eq_columns.len() as u32, ext.len() as u32]);
            program.extend(
                (0..max_cols).map(|c| t.eq_columns.get(c).copied().unwrap_or(false) as u32),
            );
            program.extend((0..source.parts.len()).map(|p| ext.get(p).copied().unwrap_or(NONE)));
        }
        offsets.insert("TEMPLATE_OFF", program.len());
        let mut lens_start = 0;
        for (ti, t) in source.templates.iter().enumerate() {
            let mut order = vec![];
            let mut todo = reads[ti].clone();
            let mut bound: BTreeSet<_> = t
                .parameters
                .iter()
                .map(|(i, _)| *i)
                .chain(t.literals.keys().copied())
                .collect();
            while !todo.is_empty() {
                let Some(i) = todo.iter().position(|r| {
                    r["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|x| bound.contains(&(x.as_u64().unwrap() as usize)))
                }) else {
                    return Err(
                        "Cell has no forward keyed realization; keep it on the existing path"
                            .into(),
                    );
                };
                let r = todo.remove(i);
                bound.insert(r["result"].as_u64().unwrap() as usize);
                order.push(r);
            }
            let rid = rule_names
                .iter()
                .position(|r| *r == t.producer)
                .ok_or("unknown producer")?;
            let n_lenses: usize = source.lenses[ti].iter().map(Vec::len).sum();
            program.extend([
                t.state["values"].as_array().unwrap().len() as u32,
                t.parameters.len() as u32,
                order.len() as u32,
                lens_start,
                n_lenses as u32,
                rid as u32,
            ]);
            lens_start += n_lenses as u32;
            program.extend(
                (0..max_params).map(|p| t.parameters.get(p).map_or(NONE, |(i, _)| *i as u32)),
            );
            program.extend((0..max_params).map(|p| {
                t.parameters.get(p).map_or(NONE, |(_, n)| {
                    fields[rid].iter().position(|x| x == n).unwrap() as u32
                })
            }));
            for v in 0..max_vals {
                program.push(
                    if let Some(v) = t.state["values"].as_array().unwrap().get(v) {
                        source
                            .eg
                            .cell_sort_is_equality(v["sort"].as_str().unwrap())?
                            as u32
                    } else {
                        0
                    },
                );
            }
            program.extend((0..max_vals).map(|i| t.literals.get(&i).map_or(NONE, |v| v.rep())));
            for i in 0..max_reads {
                if let Some(r) = order.get(i) {
                    let cols: Vec<_> = r["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .chain([&r["result"]])
                        .map(|x| x.as_u64().unwrap() as u32)
                        .collect();
                    program.push(table_id[r["op"].as_str().unwrap()]);
                    program.extend((0..max_cols).map(|c| cols.get(c).copied().unwrap_or(NONE)));
                } else {
                    program.extend(vec![NONE; 1 + max_cols]);
                }
            }
        }
        offsets.insert("LENS_OFF", program.len());
        for ps in &source.lenses {
            for (p, ls) in ps.iter().enumerate() {
                for l in ls {
                    program.extend([p as u32, l.aliases.len() as u32]);
                    program.extend(l.binding.iter().map(|v| v.map_or(NONE, |x| x as u32)));
                    for i in 0..max_alias {
                        program.extend(
                            l.aliases
                                .get(i)
                                .map_or([NONE, NONE], |(a, b)| [*a as u32, *b as u32]),
                        );
                    }
                }
            }
        }
        offsets.insert("PART_OFF", program.len());
        let mut masks = vec![];
        for p in &source.parts {
            let mask: BTreeSet<_> = p
                .atoms
                .iter()
                .flat_map(|a| {
                    source.sink.atoms[*a]
                        .columns
                        .iter()
                        .filter_map(|c| match c {
                            CellOperand::Slot(i) => Some(*i),
                            _ => None,
                        })
                })
                .collect();
            masks.push(mask);
            if p.cell {
                program.extend(vec![NONE; 1 + max_cols * 2]);
                continue;
            }
            let atom = &source.sink.atoms[p.atoms[0]];
            program.push(table_id[&atom.function]);
            program.extend((0..max_cols).map(|i| match atom.columns.get(i) {
                Some(CellOperand::Slot(s)) => *s as u32,
                _ => NONE,
            }));
            program.extend((0..max_cols).map(|i| match atom.columns.get(i) {
                Some(CellOperand::Constant { value, .. }) => value.rep(),
                _ => NONE,
            }));
        }
        let mut indexes = Vec::<(usize, Vec<usize>)>::new();
        let mut plans = vec![];
        for anchor in 0..source.parts.len() {
            let mut bound = masks[anchor].clone();
            let mut steps = vec![];
            for p in 0..source.parts.len() {
                if p == anchor {
                    continue;
                }
                let key = masks[p].intersection(&bound).copied().collect::<Vec<_>>();
                let id = if let Some(i) = indexes.iter().position(|x| *x == (p, key.clone())) {
                    i
                } else {
                    indexes.push((p, key));
                    indexes.len() - 1
                };
                steps.push(id as u32);
                bound.extend(&masks[p]);
            }
            plans.push(steps);
        }
        offsets.insert("INDEX_OFF", program.len());
        for (p, keys) in &indexes {
            program.extend([*p as u32, keys.len() as u32]);
            program.extend((0..k).map(|i| keys.get(i).map_or(NONE, |x| *x as u32)));
        }
        offsets.insert("PLAN_OFF", program.len());
        for p in &plans {
            program.extend(p);
        }
        let definitions = json!({"MAX_COLS":max_cols,"MAX_VALUES":max_vals,"MAX_PARAMS":max_params,"MAX_READS":max_reads,"MAX_LENSES":max_lenses,"MAX_ALIASES":max_alias,"MAX_FIELDS":max_fields,"VARS":k,"PART_COUNT":source.parts.len(),"TABLE_COUNT":names.len(),"TEMPLATE_COUNT":source.templates.len(),"INDEX_COUNT":indexes.len()});
        let config = json!({"version":1,"shader":include_str!("../resident.metal"),"definitions":definitions,"offsets":offsets,"program":program});
        let device = Device::new(worker, &config)?;
        let description = json!({"device":device.device,"definitions":definitions,"tables":names,"index_interfaces":indexes,"rule_fields":rule_names.iter().zip(&fields).collect::<Vec<_>>(),"producer_trace":"compact callbacks, no TraceSession","scope":"same positive analyzed templates; current native facts authoritative"});
        let mut stats = ResidentStats::default();
        stats.setup_seconds = t0.elapsed().as_secs_f64();
        Ok(Self {
            eg: source.eg,
            sink: source.sink,
            ruleset: source.ruleset,
            tap,
            feed,
            events: vec![],
            device,
            max_cols,
            max_fields,
            consumer_id,
            epoch: 0,
            audit: source.audit,
            seen: BTreeSet::new(),
            description,
            stats,
        })
    }
    pub fn engine(&self) -> &EGraph {
        &self.eg
    }
    pub fn describe(&self) -> Json {
        self.description.clone()
    }
    pub fn input(&mut self, text: &str) -> Result<()> {
        let cmds = self.eg.parse_program(None, text)?;
        if cmds.iter().any(|c| {
            !matches!(
                c,
                egglog::ast::Command::Action(..) | egglog::ast::Command::Check(..)
            )
        }) {
            return Err(
                "resident input accepts only actions/checks in a frozen environment".into(),
            );
        }
        self.eg.run_program(cmds)?;
        Ok(())
    }
    pub fn round(&mut self) -> Result<bool> {
        let start = Instant::now();
        let report = self.eg.step_cell_producers(&self.tap, &self.ruleset)?;
        self.stats.native_seconds += start.elapsed().as_secs_f64();
        self.stats.native_matches += report.num_matches_per_rule.values().sum::<usize>();
        let start = Instant::now();
        let mut delta = self.eg.drain_cell_commits(&mut self.feed)?;
        self.events.extend(self.tap.drain());
        if delta.rows.is_empty()
            && delta.clears.is_empty()
            && delta.unions.is_empty()
            && self.events.is_empty()
        {
            self.stats.quiet_rounds += 1;
            self.audit_rows(&[])?;
            return Ok(report.updated);
        }
        self.epoch = self.epoch.checked_add(1).ok_or("GPU epoch exhausted")?;
        let mut frame = vec![
            self.epoch,
            delta.rows.len() as u32,
            delta.unions.len() as u32,
            self.events.len() as u32,
            delta.clears.len() as u32,
        ];
        self.stats.row_deltas += delta.rows.len();
        self.stats.union_deltas += delta.unions.len();
        self.stats.producer_events += self.events.len();
        for row in delta.rows.drain(..) {
            let values = row.row.as_deref().unwrap_or(&row.key);
            frame.extend([
                row.table as u32,
                row.visible as u32,
                row.row.is_none() as u32,
                NONE,
            ]);
            frame.extend((0..self.max_cols).map(|c| values.get(c).map_or(NONE, |v| v.rep())));
        }
        for (a, b) in delta.unions {
            frame.extend([a.rep(), b.rep()]);
        }
        for e in self.events.drain(..) {
            frame.push(e.rule as u32);
            frame.extend((0..self.max_fields).map(|i| e.values.get(i).map_or(NONE, |v| v.rep())));
        }
        frame.extend(delta.clears.iter().map(|i| *i as u32));
        let bytes = frame
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        self.stats.uploaded_bytes += bytes.len();
        self.stats.event_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        self.device
            .input
            .write_all(&(u32::try_from(bytes.len())?).to_le_bytes())?;
        self.device.input.write_all(&bytes)?;
        self.device.input.flush()?;
        let response = read_json(&mut self.device.output)?;
        if response["ok"] != true || response["epoch"] != self.epoch {
            return Err(format!("GPU batch failed or stale: {response}").into());
        }
        let count = response["bindings"]
            .as_u64()
            .ok_or("missing GPU binding count")? as usize;
        let k = self.sink.slots().len();
        let mut bytes = vec![
            0;
            count
                .checked_mul(k)
                .and_then(|x| x.checked_mul(4))
                .ok_or("GPU result size overflow")?
        ];
        self.device.output.read_exact(&mut bytes)?;
        let words = bytes
            .chunks_exact(4)
            .map(|b| Value::new_const(u32::from_le_bytes(b.try_into().unwrap())))
            .collect::<Vec<_>>();
        let bindings = words.chunks(k).map(<[Value]>::to_vec).collect::<Vec<_>>();
        self.stats.downloaded_bytes += bytes.len();
        self.stats.batches += 1;
        self.stats.gpu = response;
        self.stats.device_roundtrip_seconds += start.elapsed().as_secs_f64();
        self.audit_rows(&bindings)?;
        let start = Instant::now();
        self.eg.apply_cell_bindings(&self.sink, &bindings)?;
        self.stats.rhs_seconds += start.elapsed().as_secs_f64();
        self.stats.dispatched_bindings += bindings.len();
        self.events
            .extend(bindings.into_iter().map(|values| CellProducerEvent {
                rule: self.consumer_id,
                values,
            }));
        Ok(report.updated || count != 0)
    }
    fn audit_rows(&mut self, rows: &[Vec<Value>]) -> Result<()> {
        if !self.audit {
            return Ok(());
        }
        let expected = self
            .eg
            .audit_cell_matches(&self.sink)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let actual = rows.iter().cloned().collect::<BTreeSet<_>>();
        if actual.len() != rows.len() || !actual.is_subset(&expected) {
            return Err("resident GPU emitted duplicate/invalid native bindings".into());
        }
        self.seen = self
            .seen
            .iter()
            .map(|b| self.sink.canonicalize(&self.eg, b))
            .collect::<std::result::Result<_, _>>()?;
        self.seen.extend(actual);
        self.stats.audit_queries += 1;
        self.stats
            .audit_uncovered_per_round
            .push(expected.difference(&self.seen).count());
        Ok(())
    }
}
