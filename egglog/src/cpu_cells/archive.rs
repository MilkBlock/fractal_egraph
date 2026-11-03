//! Optional read-only compact stage archive for local sampling. Not on the
//! normal inference path. Values are interned; only changed table rows are sent.
use super::*;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

struct TableState {
    name: String,
    id: FunctionId,
    schema: Vec<ArcSort>,
    version: Option<core_relations::TableVersion>,
    live_rows: usize,
    rows: BTreeMap<Vec<u32>, Vec<u32>>,
}
pub struct StageArchive {
    frames: BufWriter<File>,
    stages: BufWriter<File>,
    tables: Vec<TableState>,
    values: Vec<J>,
    by_value: BTreeMap<String, u32>,
    scalar_cache: BTreeMap<(String, Value), u32>,
    equalities: Vec<(ArcSort, Value, u32)>,
    aliases: BTreeMap<u32, u32>,
    unions: usize,
    frame: usize,
    before: usize,
    before_unions: usize,
}
impl StageArchive {
    pub fn new(eg: &EGraph, path: &Path, core: &J) -> Result<Self> {
        fs::create_dir(path).map_err(|e| err(e.to_string()))?;
        fs::write(path.join("manifest.json"),serde_json::to_vec(&json!({"schema":"eggfuse-stage-archive/v1","core":core,"semantics":"committed native CPU stage boundaries; local slices are not certified RipSat closures","values":"interned typed values; i64 and f64 bits are strings to preserve precision"})).unwrap()).map_err(|e|err(e.to_string()))?;
        let mut tables = vec![];
        for t in array(&core["tables"])? {
            let name = string(&t["name"])?;
            let f = &eg.functions[name];
            tables.push(TableState {
                name: name.into(),
                id: f.backend_id,
                schema: f
                    .schema
                    .input
                    .iter()
                    .chain([&f.schema.output])
                    .cloned()
                    .collect(),
                version: None,
                live_rows: 0,
                rows: BTreeMap::new(),
            });
        }
        let mut archive = Self {
            frames: BufWriter::new(
                File::create(path.join("frames.ndjson")).map_err(|e| err(e.to_string()))?,
            ),
            stages: BufWriter::new(
                File::create(path.join("stages.ndjson")).map_err(|e| err(e.to_string()))?,
            ),
            tables,
            values: vec![],
            by_value: BTreeMap::new(),
            scalar_cache: BTreeMap::new(),
            equalities: vec![],
            aliases: BTreeMap::new(),
            unions: eg.native_union_count(),
            frame: 0,
            before: 0,
            before_unions: eg.native_union_count(),
        };
        let initial = archive.record(
            eg,
            &json!({"ordinal":0,"ruleset":null,"path":[],"iterations":[]}),
            "initial",
        )?;
        archive.stage_line(&json!({"ordinal":0,"ruleset":null,"path":[],"iterations":[],"before":initial,"after":initial,"matches":[],"logical_matches":0,"rows":archive.row_count()}))?;
        Ok(archive)
    }
    fn value(&mut self, eg: &EGraph, v: Value, s: &ArcSort) -> Result<u32> {
        let v = if s.is_eq_sort() {
            eg.get_canonical_value(v, s)
        } else {
            v
        };
        let key = (s.name().to_owned(), v);
        if !s.is_container_sort() {
            if let Some(id) = self.scalar_cache.get(&key) {
                return Ok(*id);
            }
        }
        let mut j = eg.device_value(v, s).unwrap_or_else(
            |reason| json!({"sort":s.name(),"opaque":format!("{v:?}"),"reason":reason}),
        );
        // Sets are unordered; old handles may retain elements that have since
        // become equal. Normalize their typed children for alias comparison.
        if let Some(elements) = j.get_mut("elements").and_then(J::as_array_mut) {
            elements.sort_by_cached_key(|x| serde_json::to_string(x).unwrap());
            elements.dedup();
        }
        if let Some(n) = j["value"].as_i64() {
            j["value"] = json!(n.to_string());
        }
        if s.name() == "f64" {
            j["display"] = json!((**eg.value_to_base::<crate::sort::F>(v)).to_string());
        }
        let encoded = serde_json::to_string(&j).unwrap();
        let id = if let Some(id) = self.by_value.get(&encoded) {
            *id
        } else {
            let id = u32::try_from(self.values.len())
                .map_err(|_| err("archive value count overflow"))?;
            self.by_value.insert(encoded, id);
            self.values.push(j);
            if s.is_eq_sort() || s.is_container_sort() {
                self.equalities.push((s.clone(), v, id));
            }
            id
        };
        if !s.is_container_sort() {
            self.scalar_cache.insert(key, id);
        }
        Ok(id)
    }
    fn row_count(&self) -> usize {
        self.tables.iter().map(|t| t.rows.len()).sum()
    }
    fn record(&mut self, eg: &EGraph, stage: &J, phase: &str) -> Result<usize> {
        let value_start = self.values.len();
        let mut changes = vec![];
        for ti in 0..self.tables.len() {
            let version = eg.backend.native_function_version(self.tables[ti].id);
            let live_rows = eg.backend.table_size(self.tables[ti].id);
            // In-place tombstones need not change the append/generation version.
            if self.tables[ti].version.as_ref() == Some(&version)
                && self.tables[ti].live_rows == live_rows
            {
                continue;
            }
            let name = self.tables[ti].name.clone();
            let schema = self.tables[ti].schema.clone();
            let mut rows = BTreeMap::new();
            let mut problem = None;
            eg.function_for_each(&name, |r| {
                let values = r
                    .vals
                    .iter()
                    .zip(&schema)
                    .map(|(v, s)| self.value(eg, *v, s))
                    .collect::<Result<Vec<_>>>();
                match values {
                    Ok(mut values) => {
                        let key = values[..values.len() - 1].to_vec();
                        values.push(u32::from(r.subsumed));
                        rows.insert(key, values);
                    }
                    Err(e) => problem = Some(e),
                }
            })?;
            if let Some(e) = problem {
                return Err(e);
            }
            let old = &self.tables[ti].rows;
            let upserts = rows
                .iter()
                .filter(|(k, v)| old.get(*k) != Some(*v))
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>();
            let removed = old
                .keys()
                .filter(|k| !rows.contains_key(*k))
                .cloned()
                .collect::<Vec<_>>();
            if !upserts.is_empty() || !removed.is_empty() {
                changes.push(json!({"table":ti,"upserts":upserts,"removed":removed}));
            }
            self.tables[ti].rows = rows;
            self.tables[ti].version = Some(version);
            self.tables[ti].live_rows = live_rows;
        }
        let mut aliases = vec![];
        if self.unions != eg.native_union_count() {
            let known = self.equalities.clone();
            for (sort, value, id) in known {
                let now = eg.get_canonical_value(value, &sort);
                if now == value && !sort.is_container_sort() {
                    continue;
                }
                let to = self.value(eg, now, &sort)?;
                if to == id {
                    continue;
                }
                if self.aliases.insert(id, to) != Some(to) {
                    aliases.push([id, to]);
                }
            }
            self.unions = eg.native_union_count();
        }
        let record = json!({"frame":self.frame,"stage":stage,"phase":phase,"value_start":value_start,"values":self.values[value_start..],"changes":changes,"aliases":aliases,"rows":self.row_count()});
        serde_json::to_writer(&mut self.frames, &record).map_err(|e| err(e.to_string()))?;
        self.frames
            .write_all(b"\n")
            .and_then(|_| self.frames.flush())
            .map_err(|e| err(e.to_string()))?;
        let id = self.frame;
        self.frame += 1;
        Ok(id)
    }
    fn stage_line(&mut self, j: &J) -> Result<()> {
        serde_json::to_writer(&mut self.stages, j).map_err(|e| err(e.to_string()))?;
        self.stages
            .write_all(b"\n")
            .and_then(|_| self.stages.flush())
            .map_err(|e| err(e.to_string()))
    }
    pub fn begin(&mut self, eg: &EGraph, stage: &J) -> Result<()> {
        self.before_unions = eg.native_union_count();
        self.before = self.record(eg, stage, "before")?;
        Ok(())
    }
    pub fn finish(
        &mut self,
        eg: &EGraph,
        stage: &J,
        iteration: &egglog_reports::IterationReport,
    ) -> Result<()> {
        let after = self.record(eg, stage, "after")?;
        let mut event = stage.clone();
        let matches=iteration.rules().map(|name|json!({"rule":name.strip_prefix("eggfuse.staged.").unwrap_or(name),"count":iteration.rule_set_report.num_matches(name)})).collect::<Vec<_>>();
        event["new_unions"] = json!(eg.native_union_count() - self.before_unions);
        event["before"] = json!(self.before);
        event["after"] = json!(after);
        event["rows"] = json!(self.row_count());
        event["logical_matches"] = json!(
            matches
                .iter()
                .map(|m| m["count"].as_u64().unwrap_or(0))
                .sum::<u64>()
        );
        event["matches"] = json!(matches);
        self.stage_line(&event)
    }
}
