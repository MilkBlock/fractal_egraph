//! Every registered CS entry is ripened. FIFO sweeps preserve live native state;
//! scheduling never drops entries or declares a running task saturated.
use super::*;
use crate::coarse_smooth::LayerStore;
use std::path::PathBuf;
struct Work {
    id: usize,
    session: ripen::Session,
    active_seconds: f64,
}
pub(super) struct Pipeline {
    out: PathBuf,
    source: String,
    cs: cs::Store,
    jobs: Vec<Json>,
    work: Vec<Work>,
    owners: BTreeMap<String, usize>,
    aliases: BTreeMap<usize, usize>,
    completed: Vec<PathBuf>,
    catalog: Option<Json>,
    report: Json,
    observation: crate::ripen_observation::Limits,
    bucket_matches: BTreeMap<(usize, usize), (u64, Json)>,
    preparation: ripen::PreparationCache,
    reentry: crate::reentry_bucket::Buckets,
    structural_reentry: BTreeMap<usize, Json>,
    weak: BTreeMap<(usize, usize), (u64, Json)>,
}
impl Pipeline {
    pub fn new(
        out: &Path,
        source: String,
        observation: crate::ripen_observation::Limits,
    ) -> Result<Self> {
        Ok(Self {
            out: out.into(),
            source,
            cs: Default::default(),
            jobs: vec![],
            work: vec![],
            owners: BTreeMap::new(),
            aliases: BTreeMap::new(),
            completed: vec![],
            catalog: None,
            report: Json::Null,
            observation,
            bucket_matches: BTreeMap::new(),
            preparation: Default::default(),
            reentry: Default::default(),
            structural_reentry: BTreeMap::new(),
            weak: BTreeMap::new(),
        })
    }
    pub fn has_work(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j["state"].as_str(), Some("Running" | "WaitingEquivalent")))
    }
    pub fn report(&self) -> Json {
        self.report.clone()
    }
    pub fn step(&mut self, c: &Captured, layers: &LayerStore, boundary: usize) -> Result<Json> {
        let profile = std::env::var_os("EGG_LAYOUT_PROFILE").is_some();
        let mut phase = std::time::Instant::now();
        let mut timings = BTreeMap::new();
        let mut mark = |name: &str| {
            let seconds = phase.elapsed().as_secs_f64();
            timings.insert(name.to_string(), seconds);
            if profile {
                eprintln!("[perf-ripen] boundary={boundary} phase={name} seconds={seconds:.6}");
            }
            phase = std::time::Instant::now();
        };
        std::fs::create_dir_all(self.out.join("saturated-rule-composition/cells"))?;
        if !self.out.join("source.egg").exists() {
            std::fs::write(self.out.join("source.egg"), &c.preview_source)?;
        }
        for (_, unit) in self.cs.discover(c, layers, boundary)? {
            let id = self.jobs.len();
            let u = &self.cs.units[unit];
            let members = u.members();
            let mut job = json!({"candidate_kind":"CSUnit","candidate_id":unit,"state":"Running","discovered_boundary":boundary});
            let folder = self
                .out
                .join("saturated-rule-composition/cells")
                .join(format!("CSUnit-{unit:06}"));
            let prepared = (|| -> Result<()> {
                let (source, validation, mut origin) = self.preparation.prepare(c, &members)?;
                origin["candidate_kind"] = json!("CSUnit");
                origin["candidate_id"] = json!(unit);
                origin["history"] = json!(self.source);
                if let Some(comp) = u.source_composition {
                    origin["composition"] = json!(self.cs.compositions[comp]);
                    origin["composition_id"] = json!(comp);
                }
                let key = serde_json::to_string(&(&source, &validation))?;
                job["origin"] = origin;
                std::fs::create_dir_all(&folder)?;
                let entry = folder.join("source.egg");
                std::fs::write(&entry, &source)?;
                std::fs::write(folder.join("entry.egg"), &source)?;
                job["folder"] = json!(folder);
                if let Some(&owner) = self.owners.get(&key) {
                    self.aliases.insert(id, owner);
                    job["state"] = json!("WaitingEquivalent");
                    job["fully_equivalent_to"] = json!(owner);
                } else {
                    let session = ripen::Session::new_cached(&entry, &folder, &self.preparation)?;
                    if session.initial.is_some() {
                        self.owners.insert(key, id);
                    }
                    self.work.push(Work {
                        id,
                        session,
                        active_seconds: 0.,
                    });
                }
                Ok(())
            })();
            if let Err(e) = prepared {
                job["state"] = json!("Rejected");
                job["reason"] = json!(e.to_string());
            }
            self.jobs.push(job);
        }
        mark("register_entries");
        let mut changed = false;
        // One bounded batch of native passes per live task; this is fair interleaving, not a
        // completion/round budget. execute() keeps polling until completion or stop.
        for work in &mut self.work {
            if self.jobs[work.id]["state"] != "Running" {
                continue;
            }
            let started = std::time::Instant::now();
            let result = work
                .session
                .advance_with_observation(&self.observation, work.active_seconds);
            work.active_seconds += started.elapsed().as_secs_f64();
            match result {
                Ok(()) => {
                    self.jobs[work.id]["ripen"] = work.session.report["ripen"].clone();
                    let facts = work.session.report["tables"]
                        .as_object()
                        .map(|t| t.values().filter_map(Json::as_u64).sum::<u64>() as usize)
                        .unwrap_or(0);
                    let mut reasons = self.observation.reached(work.active_seconds, facts);
                    if let Some(reason) = work.session.report["native_stop_reason"].as_str() {
                        if !reasons.contains(&reason) {
                            reasons.push(reason);
                        }
                    }
                    if !work.session.done && !reasons.is_empty() {
                        let mut evidence = json!({"admission":"growth_observation","unfinished":true,"scope":"unfinished ripen; starting condition unresolved","growth":{"before_facts":work.session.initial_facts,"after_facts":facts,"active_seconds":work.active_seconds,"reasons":reasons,"limits":self.observation}});
                        evidence["snapshot"] = work.session.snapshot_unfinished().unwrap_or_else(
                            |e| json!({"status":"unavailable","reason":e.to_string()}),
                        );
                        self.jobs[work.id]["state"] = json!("ObservedUnfinished");
                        self.jobs[work.id]["observation"] = evidence.clone();
                        work.session.report["ripen"]["state"] = json!("ObservedUnfinished");
                        work.session.report["observation"] = evidence.clone();
                        work.session.report["origin"] = self.jobs[work.id]["origin"].clone();
                        self.jobs[work.id]["ripen"] = work.session.report["ripen"].clone();
                        let folder = PathBuf::from(self.jobs[work.id]["folder"].as_str().unwrap());
                        crate::pipeline::write_json(
                            &folder.join("ripen.json"),
                            &work.session.report,
                        )?;
                        if let Some(trigger) = &work.session.trigger {
                            let marker = self.jobs[work.id]["origin"]["parameter_marker"]
                                .as_str()
                                .unwrap_or("");
                            self.reentry.observe(
                                work.id,
                                work.id,
                                work.session.report["ripen"]["round"].as_u64().unwrap_or(0),
                                trigger,
                                marker,
                                &work.session.effect_components,
                                work.session.union_effect,
                                &evidence,
                            )?;
                        }
                    }
                    if work.session.done {
                        let mut report = work.session.report.clone();
                        report["origin"] = self.jobs[work.id]["origin"].clone();
                        let folder = PathBuf::from(self.jobs[work.id]["folder"].as_str().unwrap());
                        std::fs::write(folder.join("ripen.json"), serde_json::to_vec(&report)?)?;
                        self.jobs[work.id]["state"] = json!("Saturated");
                        self.jobs[work.id]["export"] = report["saturated_rule_composition"].clone();
                        if report["saturated_rule_composition"]["status"] == "exported" {
                            self.completed.push(folder);
                            changed = true;
                        }
                    }
                }
                Err(e) => {
                    self.jobs[work.id]["state"] = json!("Rejected");
                    self.jobs[work.id]["reason"] = json!(e.to_string());
                }
            }
        }
        for (&id, &owner) in &self.aliases {
            if self.jobs[id]["state"] != "WaitingEquivalent" {
                continue;
            }
            if self.jobs[owner]["state"] == "Rejected" {
                self.jobs[id]["state"] = json!("Rejected");
                self.jobs[id]["reason"] = self.jobs[owner]["reason"].clone();
            } else if self.jobs[owner]["state"] == "ObservedUnfinished" {
                self.jobs[id]["state"] = json!("ObservedUnfinished");
                self.jobs[id]["observation"] = self.jobs[owner]["observation"].clone();
                self.jobs[id]["ripen"] = self.jobs[owner]["ripen"].clone();
            } else if self.jobs[owner]["state"] == "Saturated" {
                let folder = PathBuf::from(self.jobs[id]["folder"].as_str().unwrap());
                let source = PathBuf::from(self.jobs[owner]["folder"].as_str().unwrap());
                let mut report: Json =
                    serde_json::from_slice(&std::fs::read(source.join("ripen.json"))?)?;
                report["origin"] = self.jobs[id]["origin"].clone();
                report["source"] = json!(folder.join("source.egg"));
                std::fs::write(folder.join("ripen.json"), serde_json::to_vec(&report)?)?;
                for name in [
                    "native-egraph.dot",
                    "native-egraph.svg",
                    "native-egraph-initial.dot",
                    "native-egraph-initial.svg",
                ] {
                    if name.ends_with(".svg") {
                        crate::graph_render::copy(&source.join(name), &folder.join(name))?;
                    } else if source.join(name).exists() {
                        std::fs::copy(source.join(name), folder.join(name))?;
                    }
                }
                self.jobs[id]["state"] = json!("Saturated");
                self.jobs[id]["export"] = report["saturated_rule_composition"].clone();
                if source.join("saturated-rule-composition.json").exists() {
                    std::fs::copy(
                        source.join("saturated-rule-composition.json"),
                        folder.join("saturated-rule-composition.json"),
                    )?;
                    self.completed.push(folder);
                    changed = true;
                }
            }
        }
        mark("advance_and_share");
        if changed {
            let dir = self.out.join("catalog");
            let cat = crate::saturated_rule_composition::refresh_catalog(&self.completed, &dir)?;
            let states = (0..cat["saturated_rule_compositions"].as_u64().unwrap())
                .map(|i| -> Result<Json> {
                    Ok(serde_json::from_slice(&std::fs::read(
                        dir.join(format!("states/state-{i:04}.json")),
                    )?)?)
                })
                .collect::<Result<Vec<_>>>()?;
            self.catalog = Some(
                json!({"catalog":cat,"states":states,"dot":if crate::graph_render::graphs_enabled() {Some(std::fs::read_to_string(dir.join("catalog.dot"))?)} else {None}}),
            );
        }
        mark("exact_catalog");
        // Necessary operator conditions are an exact inverted index, not Top-K.
        let mut bodies = BTreeMap::<String, BTreeSet<usize>>::new();
        for (i, w) in self.work.iter().enumerate() {
            if self.jobs[w.id]["state"] != "Rejected" {
                if let Some(t) = w.session.report["tables"].as_object() {
                    for (op, n) in t {
                        if n.as_u64().unwrap_or(0) > 0 {
                            bodies.entry(op.clone()).or_default().insert(i);
                        }
                    }
                }
            }
        }
        let targets: Vec<_> = self
            .work
            .iter()
            .map(|w| (w.id, w.session.trigger.clone(), w.session.initial.is_some()))
            .collect();
        for (target_id, initial, certified) in targets {
            let Some(pattern) = initial.as_ref() else {
                continue;
            };
            let marker = self.jobs[target_id]["origin"]["parameter_marker"]
                .as_str()
                .unwrap_or("");
            let ops: BTreeSet<_> = pattern
                .rows
                .iter()
                .filter(|r| r.op != marker)
                .map(|r| r.op.clone())
                .collect();
            let mut possible = None::<BTreeSet<usize>>;
            for op in ops {
                let ids = bodies.get(&op).cloned().unwrap_or_default();
                possible =
                    Some(possible.map_or(ids.clone(), |p| p.intersection(&ids).copied().collect()));
            }
            for i in possible.unwrap_or_default() {
                let source = &mut self.work[i];
                if !certified && source.id != target_id {
                    continue;
                }
                let round = source.session.report["ripen"]["round"]
                    .as_u64()
                    .unwrap_or(0);
                let key = (source.id, target_id);
                if self.weak.get(&key).is_some_and(|(seen, _)| *seen == round)
                    || (source.id == target_id
                        && self
                            .structural_reentry
                            .get(&source.id)
                            .is_some_and(|r| r["observed_round"] == round))
                {
                    continue;
                }
                if certified {
                    let result = weak_dominate::observe(
                        &mut source.session,
                        pattern,
                        marker,
                        target_id,
                        false,
                    )
                    .unwrap_or_else(|e| json!({"status":"unresolved","reason":e.to_string()}));
                    self.weak.insert(key, (round, result));
                }
                if source.id == target_id {
                    let mut structural = weak_dominate::observe(
                        &mut source.session,
                        pattern,
                        marker,
                        target_id,
                        true,
                    )
                    .unwrap_or_else(|e| json!({"status":"unresolved","reason":e.to_string()}));
                    structural["observed_round"] = json!(round);
                    self.reentry.observe(
                        source.id,
                        target_id,
                        round,
                        pattern,
                        marker,
                        &source.session.effect_components,
                        source.session.union_effect,
                        &structural,
                    )?;
                    self.structural_reentry.insert(source.id, structural);
                }
            }
        }
        // Search existing bucket C conditions in each paused unfinished body.
        // Presence is recorded separately from evidence of a fresh match.
        let known = self.reentry.known_targets();
        let triggers: Vec<_> = self
            .work
            .iter()
            .filter(|w| known.contains(&w.id))
            .filter_map(|w| w.session.trigger.clone().map(|p| (w.id, p)))
            .collect();
        for source in &mut self.work {
            if self.jobs[source.id]["state"] != "ObservedUnfinished" {
                continue;
            }
            let round = source.session.report["ripen"]["round"]
                .as_u64()
                .unwrap_or(0);
            for (target, pattern) in &triggers {
                let key = (source.id, *target);
                if self
                    .bucket_matches
                    .get(&key)
                    .is_some_and(|(old, _)| *old == round)
                {
                    continue;
                }
                let marker = self.jobs[*target]["origin"]["parameter_marker"]
                    .as_str()
                    .unwrap_or("");
                let result =
                    weak_dominate::observe(&mut source.session, pattern, marker, *target, true)
                        .unwrap_or_else(|e| json!({"status":"unresolved","reason":e.to_string()}));
                self.bucket_matches.insert(key,(round,json!({"source":source.id,"target":target,"round":round,"contains_known_C":result["bindings"].as_array().is_some_and(|r|!r.is_empty()),"fresh_match":result["new_bindings"].as_array().is_some_and(|r|!r.is_empty()),"evidence":result})));
            }
        }
        let mut families = BTreeMap::new();
        if let Some(cat) = &self.catalog {
            for t in cat["catalog"]["triggers"].as_array().into_iter().flatten() {
                if let (Some(unit), Some(family)) = (
                    t["binding_origin"]["candidate_id"].as_u64(),
                    t["saturated_rule_composition"].as_u64(),
                ) {
                    if let Some(job) = self.jobs.iter().position(|j| j["candidate_id"] == unit) {
                        families.insert(job, family);
                    }
                }
            }
        }
        let weak:Vec<_>=self.weak.iter().filter(|(_,(_,r))|r["bindings"].as_array().is_some_and(|b|!b.is_empty())).map(|((from,to),(_,r))|json!({"from":from,"to":to,"same_ripened_family":from==to||families.get(from).zip(families.get(to)).is_some_and(|(a,b)|a==b),"reentry_observation":(from==to||families.get(from).zip(families.get(to)).is_some_and(|(a,b)|a==b))&&r["new_effects"]==true&&r["new_bindings"].as_array().is_some_and(|v|!v.is_empty()),"classification":"unclassified; fresh effects and bindings do not prove a fractal family","evidence":r})).collect();
        for e in weak
            .iter()
            .filter(|e| e["reentry_observation"] == true && e["from"] != e["to"])
        {
            let from = e["from"].as_u64().unwrap() as usize;
            let to = e["to"].as_u64().unwrap() as usize;
            let source = self.work.iter().find(|w| w.id == from).unwrap();
            let target = self.work.iter().find(|w| w.id == to).unwrap();
            if let Some(pattern) = &target.session.initial {
                let marker = self.jobs[to]["origin"]["parameter_marker"]
                    .as_str()
                    .unwrap_or("");
                self.reentry.observe(
                    from,
                    to,
                    source.session.report["ripen"]["round"]
                        .as_u64()
                        .unwrap_or(0),
                    pattern,
                    marker,
                    &source.session.effect_components,
                    source.session.union_effect,
                    &e["evidence"],
                )?;
            }
        }
        let unresolved: Vec<_> = self
            .weak
            .iter()
            .filter(|(_, (_, r))| !matches!(r["status"].as_str(), Some("matched" | "not_matched")))
            .map(|((from, to), (_, r))| json!({"from":from,"to":to,"diagnostic":r}))
            .collect();
        let mut counts = BTreeMap::<String, usize>::new();
        for j in &self.jobs {
            *counts
                .entry(j["state"].as_str().unwrap().into())
                .or_default() += 1;
        }
        mark("weak_queries_and_evidence");
        self.report = json!({"step_seconds":timings,"preparation_cache":self.preparation.stats(),"kind":"saturated_rule_composition_snapshot","boundary":boundary,"jobs":self.jobs,"cs":self.cs.report(),"weak_dominate":weak,"observation_limits":self.observation,"bucket_trigger_matches":self.bucket_matches.values().map(|(_,r)|r).collect::<Vec<_>>(),"reentry_buckets":self.reentry.report(),"structural_reentry":self.structural_reentry,"weak_unresolved":unresolved,"counts":counts,"not_queued":0,"limits":null,"scope":"all observed CS entries; fair resumable sweeps; no heuristic selection, live tier0 substitution or termination guess","catalog":self.catalog});
        crate::pipeline::write_json(
            &self.out.join("saturated-rule-composition/queue.json"),
            &self.report,
        )?;
        Ok(self.report())
    }
}
