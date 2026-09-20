//! Bounded, run-local ripen queue. Uses the in-memory importer, not trace files.
use super::*;
use crate::coarse_smooth::{LayerStore, RipenOrigin};
use std::path::PathBuf;

pub(super) struct Pipeline {
    out: PathBuf,
    source: String,
    jobs: Vec<Json>,
    cursor: usize,
    observed: usize,
    attempted_templates: BTreeSet<usize>,
    omitted: usize,
    cache: BTreeMap<String, PathBuf>,
    closed: Vec<PathBuf>,
    total: usize,
    per_boundary: usize,
    rounds: usize,
    milliseconds: usize,
    catalog_owned: bool,
}
fn limit(name: &str, default: usize) -> Result<usize> {
    match std::env::var(name) {
        Ok(s) => Ok(s.parse::<usize>().map_err(|_| format!("invalid {name}"))?),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(e.into()),
    }
}
impl Pipeline {
    pub fn new(out: &Path, source: String) -> Result<Self> {
        Ok(Self {
            out: out.to_path_buf(),
            source,
            jobs: vec![],
            cursor: 0,
            observed: 0,
            attempted_templates: BTreeSet::new(),
            omitted: 0,
            cache: BTreeMap::new(),
            closed: vec![],
            total: limit("EGG_LAYOUT_RIPEN_JOBS", 32)?.min(256),
            per_boundary: limit("EGG_LAYOUT_RIPEN_PER_BOUNDARY", 4)?.min(256),
            rounds: limit("EGG_LAYOUT_RIPEN_ROUNDS", 4)?.max(1),
            milliseconds: limit("EGG_LAYOUT_RIPEN_MILLISECONDS", 250)?,
            catalog_owned: false,
        })
    }
    pub fn step(&mut self, c: &Captured, layers: &LayerStore, boundary: usize) -> Result<Json> {
        std::fs::create_dir_all(&self.out)?;
        if !self.out.join("source.egg").exists() {
            std::fs::write(self.out.join("source.egg"), &c.preview_source)?;
        }
        for (id, u) in layers.reuse.uses.iter().enumerate().skip(self.observed) {
            if self.jobs.len() < 256 {
                self.jobs.push(
                    json!({"use_id":id,"template":u.template,"members":u.members,
                    "discovered_boundary":boundary,"state":"Pending"}),
                );
            } else {
                self.omitted += 1;
            }
        }
        self.observed = layers.reuse.uses.len();
        let start = Instant::now();
        let mut processed = 0;
        while self.cursor < self.jobs.len()
            && self.cursor < self.total
            && processed < self.per_boundary
            && start.elapsed().as_millis() < (self.milliseconds as u128)
        {
            let i = self
                .jobs
                .iter()
                .enumerate()
                .filter(|(_, j)| j["state"] == "Pending")
                .min_by_key(|(i, j)| {
                    (
                        self.attempted_templates
                            .contains(&(j["template"].as_u64().unwrap() as usize)),
                        *i,
                    )
                })
                .map(|(i, _)| i)
                .expect("pending job");
            self.attempted_templates
                .insert(self.jobs[i]["template"].as_u64().unwrap() as usize);
            self.cursor += 1;
            processed += 1;
            let id = self.jobs[i]["use_id"].as_u64().unwrap() as usize;
            let u = &layers.reuse.uses[id];
            // Each Use is immutable. A changed/re-cut comb is a newly allocated Use.
            let result = (|| -> Result<Json> {
                let (source, validation, mut origin) =
                    ripen::prepare(c, u, layers.reuse.templates[u.template].pattern.root)?;
                origin["use_id"] = json!(id);
                origin["history"] = json!(self.source);
                origin["capture_kind"] = json!("in_memory; history file not required");
                let key = serde_json::to_string(&(&source, &validation))?;
                let folder = self.out.join("closed/cells").join(format!("use-{id:06}"));
                std::fs::create_dir_all(&folder)?;
                let link = RipenOrigin {
                    history: self.source.clone(),
                    use_id: id,
                    template: u.template,
                    members: u.members.clone(),
                    symbolic_boundary: true,
                };
                let mut report: Json;
                let cached = self.cache.get(&key).cloned();
                if let Some(previous) = &cached {
                    report = serde_json::from_slice(&std::fs::read(previous.join("ripen.json"))?)?;
                    report["ripen"]["origin"] = serde_json::to_value(&link)?;
                    if report["tier1"]["ripen"].is_object() {
                        report["tier1"]["ripen"]["origin"] = serde_json::to_value(&link)?;
                    }
                    if previous.join("closed-state.json").exists() {
                        std::fs::copy(
                            previous.join("closed-state.json"),
                            folder.join("closed-state.json"),
                        )?;
                    }
                    report["source"] = json!(folder.join("entry.egg"));
                    std::fs::write(folder.join("entry.egg"), &source)?;
                } else {
                    // Native ripen runs in the same process. Compact mode omits
                    // history and per-cell DOT, but retains tier1 feedback.
                    let entry = self.out.join("closed").join(format!("entry-{id:06}.egg"));
                    std::fs::write(&entry, &source)?;
                    report = ripen::run_with_origin(
                        &entry,
                        &folder.join("work"),
                        self.rounds,
                        Some(link),
                        false,
                    )?;
                    if folder.join("work/closed-state.json").exists() {
                        std::fs::rename(
                            folder.join("work/closed-state.json"),
                            folder.join("closed-state.json"),
                        )?;
                    }
                    std::fs::write(folder.join("entry.egg"), &source)?;
                    // Only our fresh private cell workspace is discarded.
                    std::fs::remove_dir_all(folder.join("work"))?;
                    std::fs::remove_file(entry)?;
                    report["source"] = json!(folder.join("entry.egg"));
                    self.cache.insert(key, folder.clone());
                }
                report["origin"] = origin;
                std::fs::write(folder.join("ripen.json"), serde_json::to_vec(&report)?)?;
                if report["closed_state"]["status"] == "exported" {
                    self.closed.push(folder);
                }
                Ok(
                    json!({"state":report["ripen"]["state"],"export":report["closed_state"],
                    "cache_hit":cached.is_some(),"ripen":report["ripen"]}),
                )
            })();
            match result {
                Ok(r) => {
                    for (key, value) in r.as_object().unwrap() {
                        self.jobs[i][key] = value.clone();
                    }
                }
                Err(e) => {
                    self.jobs[i]["state"] = json!("Rejected");
                    self.jobs[i]["reason"] = json!(e.to_string());
                }
            }
            self.jobs[i]["processed_boundary"] = json!(boundary);
        }
        let mut counts = BTreeMap::<String, usize>::new();
        for j in &self.jobs {
            *counts
                .entry(j["state"].as_str().unwrap().into())
                .or_default() += 1;
        }
        let mut report = json!({"kind":"closed_snapshot","boundary":boundary,"jobs":self.jobs,
            "counts":counts,"observed_uses":self.observed,"not_queued":self.omitted,
            "limits":{"jobs":self.total,"per_boundary":self.per_boundary,"rounds":self.rounds,"milliseconds_between_jobs":self.milliseconds,"queue_capacity":256},
            "selection":"unattempted templates first; exact source plus validation required for cache reuse",
            "scope":"symbolic Use interface only; no tier0 replacement; budgets checked between jobs, not a hard per-rule time/memory limit",
            "catalog":null});
        if !self.closed.is_empty() {
            let dir = self.out.join("catalog");
            if self.catalog_owned {
                std::fs::remove_dir_all(&dir)?;
            }
            let catalog = crate::closed_state::catalog(&self.closed, &dir, 10000)?;
            self.catalog_owned = true;
            let n = catalog["closed_states"].as_u64().unwrap();
            let states = (0..n)
                .map(|i| -> Result<Json> {
                    Ok(serde_json::from_slice(&std::fs::read(
                        dir.join(format!("states/state-{i:04}.json")),
                    )?)?)
                })
                .collect::<Result<Vec<_>>>()?;
            report["catalog"] = json!({"catalog":catalog,"states":states,"dot":std::fs::read_to_string(dir.join("catalog.dot"))?});
        }
        std::fs::create_dir_all(self.out.join("closed"))?;
        std::fs::write(
            self.out.join("closed/queue.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(report)
    }
}
