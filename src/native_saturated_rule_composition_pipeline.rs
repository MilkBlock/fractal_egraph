//! Bounded, run-local ripen queue. Uses the in-memory importer, not trace files.
use super::*;
use crate::coarse_smooth::{LayerStore, RipenOrigin};
use std::path::PathBuf;

/// Bounded queue that turns observed candidates into isolated saturated
/// rule-composition replays.
///
/// The queue is a bridge between evidence and validation: it stores references
/// to source members, runs native replay for each candidate, and only then adds
/// an exported state to the catalog. A queue entry is never a replacement for
/// the source e-graph.
pub(super) struct Pipeline {
    out: PathBuf,
    source: String,
    jobs: Vec<Json>,
    cursor: usize,
    observed: usize,
    dependency_seen: usize,
    cs: cs::Store,
    cones: bool,
    dependency_jobs: usize,
    dependencies: bool,
    attempted_templates: BTreeSet<usize>,
    omitted: usize,
    cache: BTreeMap<String, PathBuf>,
    saturated_rule_compositions: Vec<PathBuf>,
    total: usize,
    per_boundary: usize,
    rounds: usize,
    milliseconds: usize,
    catalog_owned: bool,
    profile: BTreeMap<String,f64>,
    convergence: Option<crate::ripen_convergence::Index>,
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
            dependency_seen: 0,
            cs: cs::Store::default(),
            cones: std::env::var_os("EGG_LAYOUT_DEPENDENCY_CONES").is_some(),
            dependency_jobs: 0,
            dependencies: std::env::var_os("EGG_LAYOUT_USE_ONLY").is_none(),
            attempted_templates: BTreeSet::new(),
            omitted: 0,
            cache: BTreeMap::new(),
            saturated_rule_compositions: vec![],
            total: limit("EGG_LAYOUT_RIPEN_JOBS", 32)?.min(256),
            per_boundary: limit("EGG_LAYOUT_RIPEN_PER_BOUNDARY", 4)?.min(256),
            rounds: limit("EGG_LAYOUT_RIPEN_ROUNDS", 4)?.max(1),
            milliseconds: limit("EGG_LAYOUT_RIPEN_MILLISECONDS", 250)?,
            catalog_owned: false,
            profile: BTreeMap::new(),
            convergence: std::env::var_os("EGG_LAYOUT_RIPEN_CONVERGENCE").map(|_| crate::ripen_convergence::Index::new(4096, 10000).with_library_path(out.join("continuations.json"))),
        })
    }
    /// Process a bounded slice of pending candidates at one capture boundary.
    ///
    /// The time and job limits make the result operationally partial. `Pending`
    /// and `Suspended` are preserved so a caller can distinguish “not tried”
    /// from “replay ran out of budget.”
    pub fn step(&mut self, c: &Captured, layers: &LayerStore, boundary: usize) -> Result<Json> {
        let profile_started=Instant::now();
        std::fs::create_dir_all(&self.out)?;
        if !self.out.join("source.egg").exists() {
            std::fs::write(self.out.join("source.egg"), &c.preview_source)?;
        }
        for (id, u) in layers.reuse.uses.iter().enumerate().skip(self.observed) {
            if self.jobs.len() < 256
                && (!self.dependencies || self.jobs.len() - self.dependency_jobs < 128)
            {
                self.jobs.push(
                    json!({"candidate_kind":"Use","use_id":id,"template":u.template,"members":u.members,
                    "discovered_boundary":boundary,"state":"Pending"}),
                );
            } else {
                self.omitted += 1;
            }
        }
        self.observed = layers.reuse.uses.len();
        if self.dependencies && self.cones {
            for id in self.dependency_seen..layers.occurrences.len() {
                let mut members = BTreeSet::from([id]);
                let mut frontier = vec![id];
                // Whole bounded ancestor cone: bring the witnessed producers
                // inside the candidate instead of inventing an early input.
                while let Some(member) = frontier.pop() {
                    for &p in &c.records[member].parents {
                        if members.insert(p) {
                            frontier.push(p);
                        }
                    }
                    if members.len() > 16 {
                        break;
                    }
                }
                if members.len() < 2 {
                    continue;
                }
                if members.len() > 16 || self.dependency_jobs >= 128 || self.jobs.len() >= 256 {
                    self.omitted += 1;
                    continue;
                }
                let members: Vec<_> = members.into_iter().collect();
                let coarse: BTreeSet<_> = members
                    .iter()
                    .map(|i| layers.occurrences[*i].coarse_layer)
                    .collect();
                self.jobs.push(json!({"candidate_kind":"DependencyCone","candidate_id":id,"template":null,"members":members,
                    "coarse_layers":coarse,"discovered_boundary":boundary,"state":"Pending"}));
                self.dependency_jobs += 1;
            }
            self.dependency_seen = layers.occurrences.len();
        }
        if self.dependencies && !self.cones {
            for (kind, id) in self.cs.discover(c, layers, boundary)? {
                if self.dependency_jobs >= 128 || self.jobs.len() >= 256 {
                    self.omitted += 1;
                    continue;
                }
                // Store references, not copies of the component histories.
                self.jobs.push(json!({"candidate_kind":kind,"candidate_id":id,"template":null,"discovered_boundary":boundary,"state":"Pending"}));
                self.dependency_jobs += 1;
            }
        }
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
                        (j["candidate_kind"] != "Use") != (self.cursor % 2 == 0),
                        j["candidate_kind"] != "Use"
                            && j["candidate_kind"]
                                != ["CSCS", "CCSS", "CSUnit"][(self.cursor / 2) % 3],
                        self.attempted_templates
                            .contains(&(j["template"].as_u64().unwrap_or(u64::MAX) as usize)),
                        *i,
                    )
                })
                .map(|(i, _)| i)
                .expect("pending job");
            self.attempted_templates
                .insert(self.jobs[i]["template"].as_u64().unwrap_or(u64::MAX) as usize);
            self.cursor += 1;
            processed += 1;
            let dependency = self.jobs[i]["candidate_kind"] != "Use";
            let id = self.jobs[i][if dependency { "candidate_id" } else { "use_id" }]
                .as_u64()
                .unwrap() as usize;
            let kind = self.jobs[i]["candidate_kind"].as_str().unwrap().to_owned();
            let members: Vec<usize> = match kind.as_str() {
                "CSUnit" => self.cs.units[id].members(),
                "CSCS" | "CCSS" => self.cs.members(id),
                _ => serde_json::from_value(self.jobs[i]["members"].clone())?,
            };
            let job_started=Instant::now();
            let engines_before=ripen::engine_creation_snapshot();
            let result = (|| -> Result<Json> {
                // Every composition, including stale evidence, is replayed below. No live substitution is authorized.

                let (source, validation, mut origin) = if dependency {
                    ripen::prepare_members(c, &members)?
                } else {
                    let u = &layers.reuse.uses[id];
                    ripen::prepare(c, u, layers.reuse.templates[u.template].pattern.root)?
                };
                origin["candidate_kind"] = self.jobs[i]["candidate_kind"].clone();
                if matches!(kind.as_str(), "CSCS" | "CCSS") {
                    origin["composition"] = serde_json::to_value(&self.cs.compositions[id])?;
                }
                origin[if dependency { "candidate_id" } else { "use_id" }] = json!(id);
                origin["history"] = json!(self.source);
                origin["capture_kind"] = json!("in_memory; history file not required");
                let key = serde_json::to_string(&(&source, &validation))?;
                let folder = self
                    .out
                    .join("saturated-rule-composition/cells")
                    .join(format!("{}-{id:06}", kind.as_str()));
                std::fs::create_dir_all(&folder)?;
                let link = if dependency {
                    None
                } else {
                    Some(RipenOrigin {
                        history: self.source.clone(),
                        use_id: id,
                        template: layers.reuse.uses[id].template,
                        members: members.clone(),
                        symbolic_boundary: true,
                    })
                };
                let mut report: Json;
                let mut ripen_seconds=0.;
                let cached = self.cache.get(&key).cloned();
                if let Some(previous) = &cached {
                    report = serde_json::from_slice(&std::fs::read(previous.join("ripen.json"))?)?;
                    report["ripen"]["origin"] = serde_json::to_value(&link)?;
                    if report["tier1"]["ripen"].is_object() {
                        report["tier1"]["ripen"]["origin"] = serde_json::to_value(&link)?;
                    }
                    if previous.join("saturated-rule-composition.json").exists() {
                        std::fs::copy(
                            previous.join("saturated-rule-composition.json"),
                            folder.join("saturated-rule-composition.json"),
                        )?;
                    }
                    report["source"] = json!(folder.join("entry.egg"));
                    std::fs::write(folder.join("entry.egg"), &source)?;
                } else {
                    // Native ripen runs in the same process. Compact mode omits
                    // history and per-cell DOT, but retains tier1 feedback.
                    let entry = self.out.join("saturated-rule-composition").join(format!("entry-{id:06}.egg"));
                    std::fs::write(&entry, &source)?;
                    let ripen_started=Instant::now();
                    report = ripen::run_observed(
                        &entry,
                        &folder.join("work"),
                        self.rounds,
                        link,
                        false,
                        true,
                        self.convergence.as_mut(),
                    )?;
                    ripen_seconds=ripen_started.elapsed().as_secs_f64();
                    if folder.join("work/saturated-rule-composition.json").exists() {
                        std::fs::rename(
                            folder.join("work/saturated-rule-composition.json"),
                            folder.join("saturated-rule-composition.json"),
                        )?;
                    }
                    std::fs::write(folder.join("entry.egg"), &source)?;
                    // Only our fresh private cell workspace is discarded.
                    std::fs::remove_dir_all(folder.join("work"))?;
                    std::fs::remove_file(entry)?;
                    report["source"] = json!(folder.join("entry.egg"));
                    self.cache.insert(key, folder.clone());
                }
                if dependency {
                    report["ripen"]["origin"] = json!({"history":self.source,"candidate_kind":kind,"candidate_id":id,"members":members,"symbolic_boundary":true});
                }
                if matches!(kind.as_str(), "CSCS" | "CCSS") {
                    origin["composition"]["certificate"]["environment_definition"] =
                        json!(self.cs.environment());
                    origin["composition"]["certificate"]["native_validation_program"] =
                        json!(validation);
                    origin["composition"]["certificate"]["status"] =
                        json!("VerifiedSymbolicReplay");
                    origin["composition"]["certificate"]["staged_validation"] =
                        json!("native preconditions and aliases passed");
                    origin["composition"]["certificate"]["injections"] =
                        origin["staged_injections"].clone();
                }
                report["origin"] = origin;
                std::fs::write(folder.join("ripen.json"), serde_json::to_vec(&report)?)?;
                if report["saturated_rule_composition"]["status"] == "exported" {
                    self.saturated_rule_compositions.push(folder);
                    if matches!(kind.as_str(), "CSCS" | "CCSS") {
                        self.cs.promote(id, &report);
                    }
                }
                Ok(
                    json!({"state":report["ripen"]["state"],"export":report["saturated_rule_composition"],
                    "cache_hit":cached.is_some(),"ripen":report["ripen"],"ripen_seconds":ripen_seconds,"ripen_timings":if cached.is_none(){report["timings"].clone()}else{Json::Null}}),
                )
            })();
            *self.profile.entry("jobs_seconds".into()).or_default()+=job_started.elapsed().as_secs_f64();
            let engines_after=ripen::engine_creation_snapshot();
            *self.profile.entry("all_engine_creation_seconds".into()).or_default()+=engines_after.1-engines_before.1;
            *self.profile.entry("engine_creations".into()).or_default()+=(engines_after.0-engines_before.0) as f64;
            match result {
                Ok(r) => {
                    *self.profile.entry("native_ripen_wall_seconds".into()).or_default()+=r["ripen_seconds"].as_f64().unwrap_or(0.);
                    if let Some(times)=r["ripen_timings"].as_object(){for (k,v) in times {if k!="all_engine_creation_seconds"&&k!="engine_creations" {*self.profile.entry(k.clone()).or_default()+=v.as_f64().unwrap_or(0.);}}}
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
            if self.dependencies && !self.cones {
                for (kind, id) in self.cs.discover(c, layers, boundary)? {
                    if self.dependency_jobs >= 128 || self.jobs.len() >= 256 {
                        self.omitted += 1;
                        continue;
                    }
                    self.jobs.push(json!({"candidate_kind":kind,"candidate_id":id,"template":null,"discovered_boundary":boundary,"state":"Pending"}));
                    self.dependency_jobs += 1;
                }
            }
        }
        let mut counts = BTreeMap::<String, usize>::new();
        for j in &self.jobs {
            *counts
                .entry(j["state"].as_str().unwrap().into())
                .or_default() += 1;
        }
        if let Some(index)=&self.convergence {index.persist_library()?;}
        let mut report = json!({"kind":"saturated_rule_composition_snapshot","boundary":boundary,"jobs":self.jobs,
            "cs":self.cs.report(),"counts":counts,"observed_uses":self.observed,"dependency_candidates":self.dependency_jobs,"not_queued":self.omitted,
            "limits":{"jobs":self.total,"per_boundary":self.per_boundary,"rounds":self.rounds,"milliseconds_between_jobs":self.milliseconds,"queue_capacity":256},
            "selection":"unattempted templates first; exact source plus validation required for cache reuse",
            "scope":"symbolic Use interface only; no tier0 replacement; budgets checked between jobs, not a hard per-rule time/memory limit",
            "convergence":self.convergence.as_ref().map(|i|i.report()),"catalog":null});
        if !self.saturated_rule_compositions.is_empty() {
            let dir = self.out.join("catalog");
            if self.catalog_owned {
                std::fs::remove_dir_all(&dir)?;
            }
            let catalog_started=Instant::now();
            let catalog = crate::saturated_rule_composition::catalog(&self.saturated_rule_compositions, &dir, 10000)?;
            *self.profile.entry("catalog_wall_seconds".into()).or_default()+=catalog_started.elapsed().as_secs_f64();
            *self.profile.entry("catalog_compare_seconds".into()).or_default()+=catalog["timings"]["compare_seconds"].as_f64().unwrap_or(0.);
            *self.profile.entry("catalog_comparisons".into()).or_default()+=catalog["comparisons"].as_array().map_or(0,Vec::len) as f64;
            self.catalog_owned = true;
            let n = catalog["saturated_rule_compositions"].as_u64().unwrap();
            let states = (0..n)
                .map(|i| -> Result<Json> {
                    Ok(serde_json::from_slice(&std::fs::read(
                        dir.join(format!("states/state-{i:04}.json")),
                    )?)?)
                })
                .collect::<Result<Vec<_>>>()?;
            report["catalog"] = json!({"catalog":catalog,"states":states,"dot":std::fs::read_to_string(dir.join("catalog.dot"))?});
        }
        *self.profile.entry("phase_seconds".into()).or_default()+=profile_started.elapsed().as_secs_f64();
        report["profile"]=serde_json::to_value(&self.profile)?;
        std::fs::create_dir_all(self.out.join("saturated-rule-composition"))?;
        std::fs::write(
            self.out.join("saturated-rule-composition/queue.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(report)
    }
}
