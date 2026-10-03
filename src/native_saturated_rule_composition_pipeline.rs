//! Every registered CS entry is ripened. FIFO sweeps preserve live native state;
//! scheduling never drops entries or declares a running task saturated.
use super::*;
use crate::coarse_smooth::LayerStore;
use rayon::prelude::*;
use std::path::PathBuf;
#[path = "native_cell_egraph.rs"]
mod cell_egraph;
struct Work {
    id: usize,
    session: ripen::Session,
    active_seconds: f64,
}
pub(super) struct Pipeline {
    worker_pool: rayon::ThreadPool,
    learner:Option<crate::reuse_graph::library::Learner>,
    out: PathBuf,
    source: String,
    cs: cs::Store,
    cells: cell_egraph::Registry,
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
        reuse_library:Option<&Path>,
    ) -> Result<Self> {
        let threads = std::env::var("EGG_LAYOUT_ANALYSIS_THREADS")
            .or_else(|_| std::env::var("EGG_LAYOUT_QUERY_THREADS"))
            .ok()
            .map(|s| s.parse::<usize>())
            .transpose()?
            .unwrap_or(4);
        if threads == 0 {
            return Err("EGG_LAYOUT_ANALYSIS_THREADS must be positive".into());
        }
        Ok(Self {
            learner:reuse_library.map(crate::reuse_graph::library::Learner::load).transpose()?,
            worker_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()?,
            out: out.into(),
            source,
            cs: Default::default(),
            cells: cell_egraph::Registry::new()?,
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
    pub fn report(&self) -> &Json {
        &self.report
    }
    pub fn release_engines(&mut self) {
        let work = std::mem::take(&mut self.work);
        self.worker_pool
            .install(|| work.into_par_iter().with_max_len(1).for_each(drop));
    }
    pub fn remember_grafts(&mut self, keys: BTreeSet<String>) {
        if let Some(learner) = &mut self.learner {
            learner.remember_grafts(keys);
        }
    }
    pub fn save_reuse_library(&self)->Result<()> {
        if let Some(learner)=&self.learner {learner.save(&self.out.join("reuse-library.json"))?;}
        Ok(())
    }
    pub fn take_report(&mut self) -> Json {
        std::mem::take(&mut self.report)
    }
    pub fn step(&mut self, c: &Captured, layers: &LayerStore, boundary: usize) -> Result<()> {
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
        let mut pending = vec![];
        for (kind, unit) in self.cs.discover(c, layers, boundary)? {
            let id = self.jobs.len();
            let u = &self.cs.units[unit];
            let members = self.cs.members(unit);
            let mut job = json!({"candidate_kind":kind.clone(),"candidate_id":unit,"state":"Running","discovered_boundary":boundary});
            let folder = self
                .out
                .join("saturated-rule-composition/cells")
                .join(format!("CSUnit-{unit:06}"));
            let prepared = (|| -> Result<()> {
                let (source, validation, mut origin) = self.preparation.prepare(c, &members)?;
                origin["candidate_kind"] = json!(kind);
                origin["candidate_id"] = json!(unit);
                origin["history"] = json!(self.source);
                if let Some(comp) = u.source_composition {
                    origin["composition"] = json!(self.cs.compositions[comp]);
                    origin["composition_id"] = json!(comp);
                }
                let parents = u.source_composition
                    .map(|comp| self.cs.compositions[comp].parts.clone())
                    .unwrap_or_default();
                // A rejected parent cannot supply a certified coordinate contract.
                // Retain this valid composition as a standalone root in that case.
                let parents: Vec<_> = parents.into_iter().filter(|p| self.cells.contains(*p)).collect();
                self.cells.register(unit, &parents, &origin, &source, &validation)?;
                job["cell_unit"] = json!(unit);
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
                } else if crate::graph_render::graphs_enabled() {
                    let session = ripen::Session::new_cached(&entry, &folder, &self.preparation)?;
                    if session.initial.is_some() {
                        self.owners.insert(key, id);
                    }
                    self.work.push(Work {
                        id,
                        session,
                        active_seconds: 0.,
                    });
                } else {
                    pending.push((id, key, entry, folder));
                }
                Ok(())
            })();
            if let Err(e) = prepared {
                job["state"] = json!("Rejected");
                job["reason"] = json!(e.to_string());
            }
            self.jobs.push(job);
        }
        let prepare = |(id, key, entry, folder): &(usize, String, PathBuf, PathBuf)| {
            (
                *id,
                key.clone(),
                ripen::Session::new_cached(entry, folder, &self.preparation)
                    .map_err(|e| e.to_string()),
            )
        };
        // Native initialization can broadcast to Rayon. Running it as many
        // stealable Rayon jobs recursively nests constructors on worker stacks.
        // Initialize in source order; the subsequent sweeps remain parallel.
        let prepared: Vec<_> = pending.iter().map(prepare).collect();
        for (id, key, result) in prepared {
            if let Some(&owner) = self.owners.get(&key) {
                self.aliases.insert(id, owner);
                self.jobs[id]["state"] = json!("WaitingEquivalent");
                self.jobs[id]["fully_equivalent_to"] = json!(owner);
                continue;
            }
            match result {
                Ok(session) => {
                    if session.initial.is_some() {
                        self.owners.insert(key, id);
                    }
                    self.work.push(Work {
                        id,
                        session,
                        active_seconds: 0.,
                    });
                }
                Err(e) => {
                    self.jobs[id]["state"] = json!("Rejected");
                    self.jobs[id]["reason"] = json!(e);
                }
            }
        }
        mark("register_entries");
        let mut changed = false;
        // One bounded batch of native passes per live task; this is fair interleaving, not a
        // completion/round budget. execute() keeps polling until completion or stop.
        let jobs = &self.jobs;
        let observation = &self.observation;
        let advance = |work: &mut Work| {
            if jobs[work.id]["state"] != "Running" {
                return None;
            }
            let started = std::time::Instant::now();
            let result = work
                .session
                .advance_with_observation(observation, work.active_seconds)
                .map_err(|e| e.to_string());
            work.active_seconds += started.elapsed().as_secs_f64();
            let facts = work.session.report["tables"]
                .as_object()
                .map(|t| t.values().filter_map(Json::as_u64).sum::<u64>() as usize)
                .unwrap_or(0);
            let snapshot = (result.is_ok()
                && !work.session.done
                && (!observation.reached(work.active_seconds, facts).is_empty()
                    || work.session.report["native_stop_reason"].is_string()))
            .then(|| {
                work.session
                    .snapshot_unfinished()
                    .unwrap_or_else(|e| json!({"status":"unavailable","reason":e.to_string()}))
            });
            Some((result, snapshot))
        };
        let results: Vec<_> = if crate::graph_render::graphs_enabled() {
            self.work.iter_mut().map(advance).collect()
        } else {
            self.worker_pool.install(|| {
                self.work
                    .par_iter_mut()
                    .with_max_len(1)
                    .map(advance)
                    .collect()
            })
        };
        for (work, result) in self.work.iter_mut().zip(results) {
            let Some((result, snapshot)) = result else {
                continue;
            };
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
                        evidence["snapshot"] = snapshot
                            .expect("observation snapshot prepared with the same stop condition");
                        self.jobs[work.id]["state"] = json!("ObservedUnfinished");
                        self.jobs[work.id]["observation"] = evidence.clone();
                        work.session.report["ripen"]["state"] = json!("ObservedUnfinished");
                        work.session.report["observation"] = evidence.clone();
                        work.session.report["origin"] = self.jobs[work.id]["origin"].clone();
                        self.jobs[work.id]["ripen"] = work.session.report["ripen"].clone();
                        work.session.write_report()?;
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
                        work.session.report["origin"] = self.jobs[work.id]["origin"].clone();
                        work.session.write_report()?;
                        let report = &work.session.report;
                        let folder = PathBuf::from(self.jobs[work.id]["folder"].as_str().unwrap());
                        self.jobs[work.id]["state"] = json!("Saturated");
                        self.jobs[work.id]["export"] = report["saturated_rule_composition"].clone();
                        if report["saturated_rule_composition"]["status"] == "exported" {
                            if let (Some(learner),Some(state))=(&mut self.learner,&work.session.final_state) {
                                learner.observe(work.id,state.clone(),&work.session.report,work.session.text.clone());
                            }
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
            self.cells.admit_catalog(&cat)?;
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
        crate::pipeline::write_json(&self.out.join("cell-egraph.json"), &self.cells.report())?;
        // Discovery evidence is exported independently of query-plan admission.
        let cell_report=self.cells.report();
        let classes:BTreeMap<usize,Json>=cell_report["units"].as_array().into_iter().flatten()
            .filter_map(|u|u["unit"].as_u64().map(|id|(id as usize,u["eclass"].clone()))).collect();
        let targets:BTreeMap<_,_>=self.cs.units.iter().enumerate().filter_map(|(id,u)|u.source_composition.map(|comp|(comp,id))).collect();
        let observed:Vec<_>=self.cs.compositions.iter().enumerate().filter(|(_,comp)|comp.boundary.is_some()).map(|(id,comp)| {
            let target=targets[&id];
            let record=&c.records[comp.consumer];
            json!({"state":target,"parents":comp.parts,"producer_states":comp.producer_parts,"context_states":comp.context_parts,
                "event":record.id,"boundary":comp.boundary,"rule":comp.rule,"rule_name":c.rules[record.rule].rule.name,"ruleset":c.rules[record.rule].rule.ruleset,"kind":comp.kind,
                "binding":record.ports,"inputs":record.wanted,"external_facts":comp.external_facts,
                "inserted_facts":record.produced,"union_effects":comp.union_effects,"updated_rows":comp.updated_rows,
                "eclass":classes.get(&target),"query_compilation":"independent; not a discovery gate"})
        }).collect();
        crate::pipeline::write_json(&self.out.join("observed-transitions.json"),&json!({"transitions":observed,"count":observed.len(),"scope":"committed applies with versioned producer witnesses and previous-boundary bound-port context; not all edges are causal producer edges"}))?;
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
        let mut requests = vec![vec![]; self.work.len()];
        for (t, (target_id, initial, certified)) in targets.iter().enumerate() {
            let Some(pattern) = initial.as_ref() else {
                continue;
            };
            let marker = self.jobs[*target_id]["origin"]["parameter_marker"]
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
                let source = &self.work[i];
                if !certified && source.id != *target_id {
                    continue;
                }
                let round = source.session.report["ripen"]["round"]
                    .as_u64()
                    .unwrap_or(0);
                if self
                    .weak
                    .get(&(source.id, *target_id))
                    .is_some_and(|(seen, _)| *seen == round)
                    || (source.id == *target_id
                        && self
                            .structural_reentry
                            .get(&source.id)
                            .is_some_and(|r| r["observed_round"] == round))
                {
                    continue;
                }
                requests[i].push(t);
            }
        }
        let jobs = &self.jobs;
        let work = &mut self.work;
        let grouped: Vec<Vec<_>> = self.worker_pool.install(|| {
            work.par_iter_mut()
                .with_max_len(1)
                .enumerate()
                .map(|(i, source)| {
                    requests[i]
                        .iter()
                        .map(|&t| {
                            let (target_id, pattern, certified) = &targets[t];
                            let pattern = pattern.as_ref().unwrap();
                            let marker = jobs[*target_id]["origin"]["parameter_marker"]
                                .as_str()
                                .unwrap_or("");
                            let round = source.session.report["ripen"]["round"]
                                .as_u64()
                                .unwrap_or(0);
                            let result = certified.then(|| {
                                weak_dominate::observe(
                                    &mut source.session,
                                    pattern,
                                    marker,
                                    *target_id,
                                    false,
                                )
                                .unwrap_or_else(
                                    |e| json!({"status":"unresolved","reason":e.to_string()}),
                                )
                            });
                            let structural = (source.id == *target_id).then(|| {
                                weak_dominate::observe(
                                    &mut source.session,
                                    pattern,
                                    marker,
                                    *target_id,
                                    true,
                                )
                                .unwrap_or_else(
                                    |e| json!({"status":"unresolved","reason":e.to_string()}),
                                )
                            });
                            (t, i, source.id, round, result, structural)
                        })
                        .collect()
                })
                .collect()
        });
        let mut results: Vec<_> = grouped.into_iter().flatten().collect();
        results.sort_by_key(|r| (r.0, r.1));
        for (t, i, id, round, result, structural) in results {
            let (target_id, pattern, _) = &targets[t];
            if let Some(result) = result {
                self.weak.insert((id, *target_id), (round, result));
            }
            if let Some(mut structural) = structural {
                structural["observed_round"] = json!(round);
                let source = &self.work[i];
                let marker = self.jobs[*target_id]["origin"]["parameter_marker"]
                    .as_str()
                    .unwrap_or("");
                self.reentry.observe(
                    id,
                    *target_id,
                    round,
                    pattern.as_ref().unwrap(),
                    marker,
                    &source.session.effect_components,
                    source.session.union_effect,
                    &structural,
                )?;
                self.structural_reentry.insert(id, structural);
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
        self.report = json!({"analysis_threads":self.worker_pool.current_num_threads(),"weak_query_cache":{"result_cache_hits":self.work.iter().flat_map(|w|w.session.weak_queries.values()).map(|q|q.cache_hits).sum::<usize>(),"compiled_pairs":self.work.iter().map(|w|w.session.weak_queries.len()).sum::<usize>(),"current_scans":self.work.iter().flat_map(|w|w.session.weak_queries.values()).map(|q|q.executions).sum::<usize>()},"step_seconds":timings,"preparation_cache":self.preparation.stats(),"kind":"saturated_rule_composition_snapshot","boundary":boundary,"jobs":self.jobs,"cs":self.cs.report(),"cell_egraph":self.cells.report(),"weak_dominate":weak,"observation_limits":self.observation,"bucket_trigger_matches":self.bucket_matches.values().map(|(_,r)|r).collect::<Vec<_>>(),"reentry_buckets":self.reentry.report(),"structural_reentry":self.structural_reentry,"weak_unresolved":unresolved,"counts":counts,"not_queued":0,"limits":null,"scope":"observed state entries (legacy layers optional); fair resumable sweeps; no heuristic selection, live tier0 substitution or termination guess","catalog":self.catalog});
        if let Some(learner) = &self.learner {
            self.report["reuse_learning"] = learner.report();
        }
        crate::pipeline::write_json(
            &self.out.join("saturated-rule-composition/queue.json"),
            &self.report,
        )?;
        Ok(())
    }
}
