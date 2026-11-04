use crate::query::QueryIndexes;
#[path = "free_join.rs"]
mod free_join;
use free_join::{FreeJoin, Subtrie};
#[path="execute_kernel.rs"]
mod execute_kernel;
use crate::schedule::{self, Task};
use crate::{
    graph::*,
    program::*,
    value::{self, Value, canon},
};
use serde_json::{Value as J, json};
use crate::known_results::QueryFacts;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::Instant;

#[derive(Clone, Debug)]
struct Binding {
    #[cfg(feature = "iterative-bake")]
    direct: Option<crate::stage_spine::DirectOrigin>,
    known: QueryFacts,
    composition: Option<(Rc<crate::compositions::Edge>, crate::views::Context)>,
    witnesses: MatchedFacts,
    values: crate::slots::Slots,
    epoch: u64,
    parent: Option<FactRef>,
    continuation: Option<(usize, u32, bool)>,
    compiled: bool,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CompositionStats {
    match_seconds: f64,
    entry_seconds: f64,
    attempts: u64,
    bindings: u64,
    rejected: u64,
    unavailable: u64,
    known_atoms: u64,
    port_reads: u64,
    guard_checks: u64,
    constructor_reuses: u64,
    reuse_fallbacks: u64,
    root_steps: u64,
    continued_steps: u64,
    marker_continued_steps: u64,
    max_steps: u32,
    source_rules: BTreeSet<usize>,
}
enum ComposedMatch {
    Fallback,
    Rejected,
    Matched(Binding),
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ContinuationStats {
    merge_context_entries: u64,
    merge_port_reads: u64,
    merge_candidate_rows: u64,
    merge_guard_checks: u64,
    merge_bindings: u64,
    merge_bindings_by_rule: BTreeMap<usize,u64>,
    point_reads: u64,
    inverse_reads: u64,
    inverse_ambiguous: u64,
    point_primitives: u64,
    compiled_bindings: u64,
    entered: u64,
    projected_atoms: u64,
    local_effect_reads: u64,
    keyed_reads: u64,
    ambiguous_reads: u64,
    unavailable_sources: u64,
    complete_bindings: u64,
    residual_bindings: u64,
    published_facts: u64,
    function_updates: u64,
    max_depth: u32,
}
/// All bindings belong to this exact source Run occurrence. The patch is sealed
/// before publication, so no sibling rule can observe its future effects.
struct StagePatch {
    bindings: Vec<(usize, Binding)>,
}
struct Produced { rule:usize, depth:u32, values:Vec<Value>, routes:Vec<usize> }
pub struct Executor {
    range_index: Option<crate::range_index::RangeIndex>,
    fd_keys: bool,
    dedup_rows: u64,
    dedup_full_elements: u64,
    dedup_elements: u64,
    dedup_duplicates: u64,
    stage_spine:crate::stage_spine::Programs,
    entry_schedule:crate::entry_schedule::Scheduler,
    current_stage:String,
    retained:crate::retained::Contexts,
    local_sat:crate::local_sat::LocalSat,
    local_run:Option<(usize,Rc<crate::local_sat::Plan>)>,
    semantic_audit:Vec<J>,
    pub compositions_enabled: bool,
    composition_stats: CompositionStats,
    pub graph: Graph,
    pub steps: Vec<J>,
    watermarks: Vec<u64>,
    dirty: Vec<bool>,
    pending: Vec<BTreeSet<Dependency>>,
    readers: Vec<Vec<usize>>,
    pub audit: bool,
    pub snapshots: Vec<J>,
    pub binding_audit: Vec<J>,
    pub initial_audit: Option<J>,
    pub tasks: Vec<Task>,
    indexed_queries: bool,
    delta_queries: bool,
    column_queries: bool,
    query_indexes: QueryIndexes,
    free_join: FreeJoin,
    sieve: crate::sieve::Sieve,
    refutation: crate::refutation::Refutation,
    refutation_depth: crate::refutation::DepthProfile,
    union_grafts: crate::union_graft::Grafts,
    search_context_survey: crate::search_context::Survey,
    search_contexts: crate::search_context::Reuse,
    profile_queries: bool,
    query_profile: Vec<(u64, f64, u64)>,
    progress: bool,
    multistage_enabled: bool,
    point_programs: bool,
    local_projections: bool,
    continuation_stats: ContinuationStats,
    handoffs: BTreeSet<(usize, usize)>,
}
// Keep the ordinary lane's rows owned: optional shared entry caches must not
// introduce reference-counted heap allocations when the feature is disabled.
enum SeedRow { Owned(Row), Shared(Rc<Row>) }
impl std::ops::Deref for SeedRow {
    type Target = Row;
    fn deref(&self) -> &Row { match self { Self::Owned(r) => r, Self::Shared(r) => r } }
}
/// Run-local binding workspace. Only successful matches become stored bindings;
/// failed join branches undo slot assignments instead of cloning tree maps.
struct QueryEnv {
    known: QueryFacts,
    recipe_context: Option<(usize, u32)>,
    witnesses: MatchedFacts,
    values: crate::entry_schedule::Inputs,
    trail: Vec<usize>,
    domains: Vec<Subtrie>,
}
impl QueryEnv {
    fn new(slots: usize, witness_slots: usize, full_witnesses: bool, known: QueryFacts) -> Self {
        Self {
            known,
            recipe_context: None,
            witnesses: MatchedFacts::new(witness_slots, full_witnesses),
            values: crate::entry_schedule::Inputs::owned(slots),
            trail: vec![],
            domains: vec![],
        }
    }
    fn shared(seed:crate::entry_schedule::Seed,witness_slots:usize,full:bool,known: QueryFacts)->Self {
        Self{known,recipe_context:None,
            witnesses:MatchedFacts::new(witness_slots,full),values:crate::entry_schedule::Inputs::shared(seed),trail:vec![],domains:vec![]}
    }
    fn domain(&self, atom: usize) -> Option<&Rc<Vec<Dependency>>> {
        self.domains.iter().rev().find(|d| d.atom == atom).map(|d| &d.rows)
    }
    fn record(&mut self, atom: usize, witness: Option<Dependency>) {
        self.witnesses.record(atom, witness);
        self.known.record(atom, witness);
    }
    fn rollback(&mut self, mark: usize) {
        while self.trail.len() > mark {
            self.values[self.trail.pop().unwrap()] = None;
        }
    }
    fn binding(&self) -> crate::slots::Slots {
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::count(|c| c.binding_seal_copies += self.values.iter().flatten().count() as u64);
        self.values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.clone().map(|v| (i, v)))
            .collect()
    }
}
fn operand(a: &Arg, env: &QueryEnv, g: &Graph) -> Option<Value> {
    match a {
        Arg::Literal(v) => Some(canon(v, &g.parents)),
        Arg::Slot(i) => env.values[*i].clone(),
    }
}
fn bind(a: &Arg, v: &Value, env: &mut QueryEnv, g: &Graph) -> bool {
    // The graph is immutable throughout matching, so assigned values stay canonical.
    // All callers provide a canonical row, local slot, or primitive output.
    // No writes are visible during this immutable matching snapshot.
    match a {
        Arg::Literal(l) => canon(l, &g.parents) == *v,
        Arg::Slot(i) => {
            if let Some(old) = &env.values[*i] {
                old == v
            } else {
                #[cfg(feature = "slot-transfer-audit")]
                crate::transfer_audit::count(|c| c.fresh_bind_copies += 1);
                env.values[*i] = Some(v.clone());
                env.trail.push(*i);
                true
            }
        }
    }
}
impl Executor {
    #[cfg(feature = "iterative-bake")]
    pub fn collect_iterative_pairs(&mut self) {
        self.stage_spine.training.collect_pairs = true;
    }
    #[cfg(feature = "iterative-bake")]
    pub fn iterative_training(&self) -> &crate::iterative::Training {
        &self.stage_spine.training
    }

    #[cfg(feature = "spine-shapes")]
    pub fn spine_shape_report(&self) -> J {
        let mut report = self.stage_spine.shape_report();
        report["program_sha256"] = self.graph.program.artifact["sha256"].clone();
        report
    }

    pub fn new(mut graph: Graph, audit: bool) -> Self {
        let mut readers = vec![vec![]; graph.program.tables.len()];
        for (rid, r) in graph.program.rules.iter().enumerate() {
            for a in r.query.iter() {
                if let Call::Table(t) = a.call {
                    if !readers[t].contains(&rid) {
                        readers[t].push(rid)
                    }
                }
            }
        }
        let n = graph.program.rules.len();
        let tasks = schedule::initial(&graph.program.control);
        let initial_audit = if audit { graph.export().ok() } else { None };
        let sieve = crate::sieve::Sieve::new(&graph.program);
        let mut query_indexes = QueryIndexes::default();
        query_indexes.track_changes = sieve.enabled;
        // Sieve rejections have their own dependency graph, not these read
        // leases. Keep the prototype paths separate rather than under-record it.
        let refutation_enabled =
            std::env::var("CELL_CPU_MARKER_REFUTATION").as_deref() == Ok("1") && !sieve.enabled;
        let search_contexts = crate::search_context::Reuse::new();
        let refutation=crate::refutation::Refutation::new(refutation_enabled);
        query_indexes.watch_reads = refutation_enabled && !refutation.scoped;
        let union_grafts = crate::union_graft::Grafts::new(
            &graph.program,
            std::env::var("CELL_CPU_UNION_GRAFT").as_deref() == Ok("1"),
        );
        let local_sat=crate::local_sat::LocalSat::new(&graph.program);
        let retained=crate::retained::Contexts::new(&graph.program);
        query_indexes.watch_contexts=retained.enabled;
        query_indexes.watch_context_rows=retained.shared();
        let mut stage_spine=crate::stage_spine::Programs::new(&graph.program);
        stage_spine.audit_receipts=audit;
        let free_join = FreeJoin::new(&graph.program);
        let entry_schedule = crate::entry_schedule::Scheduler::with_epoch_counts(
            &graph.program,
            std::env::var("CELL_CPU_SHARED_NODE_SCHEDULE").as_deref() == Ok("1")
                && !matches!(std::env::var("CELL_CPU_QUERY_MODE").as_deref(), Ok("scan" | "indexed")),
            free_join.planned,
        );
        graph.receipt_tracking=(stage_spine.enabled && stage_spine.receipts) || entry_schedule.enabled;
        let mut e = Self {
            range_index: (std::env::var("CELL_CPU_RANGE_GUARDS").as_deref() == Ok("1"))
                .then(Default::default),
            fd_keys: std::env::var("CELL_CPU_FD_KEYS").as_deref() == Ok("1"),
            dedup_rows: 0,
            dedup_full_elements: 0,
            dedup_elements: 0,
            dedup_duplicates: 0,
            free_join,
            stage_spine,entry_schedule,current_stage:String::new(),
            retained,
            local_sat,local_run:None,semantic_audit:vec![],
            compositions_enabled: std::env::var("CELL_CPU_COMPOSITIONS").as_deref() == Ok("1"),
            composition_stats: CompositionStats::default(),
            graph,
            steps: vec![],
            watermarks: vec![0; n],
            dirty: vec![true;n],
            pending: vec![BTreeSet::new(); n],
            readers,
            audit,
            snapshots: vec![],
            binding_audit: vec![],
            initial_audit,
            tasks,
            column_queries: std::env::var("CELL_CPU_QUERY_MODE").as_deref() != Ok("reverse"),
            query_indexes,
            sieve,
            refutation,
            refutation_depth:crate::refutation::DepthProfile::new(),
            union_grafts,
            search_context_survey: crate::search_context::Survey::new(),
            search_contexts,
            profile_queries: std::env::var_os("CELL_CPU_PROFILE").is_some(),
            query_profile: vec![(0, 0.0, 0); n],
            progress: std::env::var_os("CELL_CPU_PROGRESS").is_some(),
            multistage_enabled: std::env::var("CELL_CPU_MULTISTAGE").as_deref() != Ok("0"),
            point_programs: std::env::var("CELL_CPU_ENTRY_PROGRAMS").as_deref() != Ok("0"),
            local_projections: std::env::var("CELL_CPU_LOCAL_PROJECTIONS").as_deref() != Ok("0"),
            continuation_stats: ContinuationStats::default(),
            handoffs: BTreeSet::new(),
            indexed_queries: std::env::var("CELL_CPU_QUERY_MODE").as_deref() != Ok("scan"),
            delta_queries: !matches!(
                std::env::var("CELL_CPU_QUERY_MODE").as_deref(),
                Ok("scan" | "indexed")
            ),
        };
        e.entry_schedule.publish(&e.graph);
        e.deliver();
        e.graph.query_changes.clear();
        e
    }
    fn deliver(&mut self) {
        // All events are delivered at one published Run boundary. Repeated
        // repairs of one fact have the same final state and subscriber set.
        let mut events = std::mem::take(&mut self.graph.events);
        self.graph.stats.delivery_events += events.len() as u64;
        events.sort_unstable();
        events.dedup();
        self.graph.stats.distinct_delivery_events += events.len() as u64;
        for event in events {
            if let Some(table) = self.graph.live_table(event) {
                for &rid in &self.readers[table] {
                    self.dirty[rid]=true;
                    if !self.entry_schedule.accepts(rid) && (self.graph.program.rules[rid].direct.is_some() || self.delta_queries) {
                        self.pending[rid].insert(event);
                    }
                }
            }
        }
    }
    fn solve(
        &mut self,
        query: &[Atom],
        rid: usize,
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
    ) -> Result<()> {
        self.solve_recorded(query, rid, remaining, env, epoch, out, delta, None)
    }
    fn recurse(
        &mut self,
        query: &[Atom],
        rid: usize,
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
        evidence: Option<&mut crate::refutation::Evidence>,
    ) -> Result<()> {
        if self.refutation.enabled && self.refutation.multistage {
            self.solve_recorded(query, rid, remaining, env, epoch, out, delta, evidence)
        } else {
            self.solve_inner(query, rid, remaining, env, epoch, out, delta, evidence)
        }
    }
    fn attach_recipe_context(&mut self, rid: usize, anchor: usize, row: &Row, env: &mut QueryEnv) {
        if !self.refutation.enabled || !self.refutation.multistage || !self.multistage_enabled {
            return;
        }
        if let Some(c) = self.graph.continuation_context(row.witness) {
            if c.completed
                && self
                    .graph
                    .program
                    .multi_stage
                    .routes
                    .contains_key(&(c.template, c.op, rid, anchor))
            {
                env.recipe_context = Some((c.template, c.depth));
                self.refutation.stats.continuation_entries += 1;
                self.refutation.stats.max_continuation_depth =
                    self.refutation.stats.max_continuation_depth.max(c.depth);
            }
        }
    }
    fn reject_entry(
        &mut self,
        rid: usize,
        query: &[Atom],
        remaining: &[bool],
        env: &QueryEnv,
    ) -> bool {
        if !self.refutation.enabled || !self.refutation.multistage || self.refutation.shadow {
            return false;
        }
        let marker = query.iter().enumerate().any(|(i, a)| {
            !remaining[i]
                && matches!(a.call,Call::Table(t) if self.graph.program.views.markers.contains(&t))
        });
        if !marker && env.recipe_context.is_none() {
            return false;
        }
        // Already-read atoms remain requirements too. A failed subprogram may
        // span those atoms and an as-yet unresolved suffix in a later Stage.
        let key = self.refutation_key(query, &vec![true; query.len()], env);
        let rejected=self.refutation.reject(rid,&key,&self.query_indexes,&self.graph).is_some();
        if rejected {self.refutation_depth.reject(query.iter().enumerate().filter(|(i,a)|!remaining[*i]&&matches!(a.call,Call::Table(_))).count()+1);}
        rejected
    }
    fn refutation_key(
        &self,
        query: &[Atom],
        remaining: &[bool],
        env: &QueryEnv,
    ) -> crate::refutation::Key {
        use crate::refutation::{Operand, Operator};
        let mut variables = BTreeMap::new();
        crate::refutation::Key {
            atoms: query
                .iter()
                .enumerate()
                .filter(|(i, _)| remaining[*i])
                .map(|(_, a)| {
                    let op = match &a.call {
                        Call::Table(t) => Operator::Table(*t),
                        Call::Primitive(n, o) => Operator::Primitive(n.clone(), o.clone()),
                    };
                    let args = a
                        .args
                        .iter()
                        .map(|a| {
                            if let Some(v) = operand(a, env, &self.graph) {
                                Operand::Bound(v)
                            } else {
                                let Arg::Slot(s) = a else { unreachable!() };
                                let next = variables.len();
                                Operand::Variable(*variables.entry(*s).or_insert(next))
                            }
                        })
                        .collect();
                    (op, args)
                })
                .collect(),
        }
    }
    fn solve_recorded(
        &mut self, query:&[Atom],rid:usize,remaining:&mut [bool],env:&mut QueryEnv,
        epoch:u64,out:&mut Vec<Binding>,delta:Option<(usize,u64,Dependency)>,
        parent:Option<&mut crate::refutation::Evidence>,
    )->Result<()> {
        if self.search_contexts.enabled && self.search_contexts.residual && parent.is_none() && !self.refutation.enabled
            && !self.sieve.enabled && delta.is_some()
            && query.iter().enumerate().filter(|(i,a)|remaining[*i] && matches!(a.call,Call::Table(_))).count()>=2 {
            return self.solve_context(query,rid,remaining,env,epoch,out,delta,None);
        }
        if !self.refutation.enabled || (self.refutation.shadow && parent.as_ref().is_some_and(|p|p.shadowing)) {
            return self.solve_inner(query,rid,remaining,env,epoch,out,delta,parent);
        }
        let markers=query.iter().enumerate().filter_map(|(i,a)| match a.call {
            Call::Table(t) if !remaining[i] && self.graph.program.views.markers.contains(&t)=>Some((i,t)),_=>None
        }).collect::<Vec<_>>();
        let tables=query.iter().enumerate().filter(|(i,a)|remaining[*i]&&matches!(a.call,Call::Table(_))).count();
        let guarded=self.refutation.cores && tables>0 && query.iter().enumerate().any(|(i,a)|remaining[i]&&matches!(a.call,Call::Primitive(..)));
        if (markers.is_empty() && !(self.refutation.multistage && env.recipe_context.is_some())) || (tables<2 && !guarded) {
            return self.solve_inner(query,rid,remaining,env,epoch,out,delta,parent);
        }
        let key=self.refutation_key(query,remaining,env);
        let atoms=remaining.iter().enumerate().filter_map(|(i,p)|p.then_some(i)).collect::<Vec<_>>();
        let hit=self.refutation.reject(rid,&key,&self.query_indexes,&self.graph);
        if let Some(proof)=&hit {if !self.refutation.shadow {
            self.refutation_depth.reject(query.iter().enumerate().filter(|(i,a)|!remaining[*i]&&matches!(a.call,Call::Table(_))).count()+1);
            if let Some(parent)=parent {
                parent.query_reads+=proof.query_reads;parent.reads.extend(proof.reads.iter().cloned());
                // Reused proof ids belong to another query. Conservatively retain
                // this entire residual conjunction in any enclosing proof slice.
                if self.refutation.cores {parent.visited.extend(atoms.iter().copied());}
            }
            return Ok(())
        }}
        let mut own=crate::refutation::Evidence::default();let proof=parent.unwrap_or(&mut own);
        let first_read=proof.reads.len();let first_restriction=proof.delta_restrictions;
        let first_query_read=proof.query_reads;let first_visit=proof.visited.len();
        if !self.refutation.cores {for (i,t) in markers {
            let bound=query[i].args.iter().map(|a|operand(a,env,&self.graph)).collect::<Vec<_>>();
            proof.reads.push(self.query_indexes.read_stamp(&self.graph,t,&bound)?);
        }}
        let shadowing=proof.shadowing;proof.shadowing|=hit.is_some();
        let before=out.len();let candidates=self.graph.stats.residual_candidates;let keys=self.graph.key_lookup_calls.get();
        self.solve_inner(query,rid,remaining,env,epoch,out,delta,Some(proof))?;
        proof.shadowing=shadowing;
        if hit.is_some() {
            if out.len()!=before{return Err("refutation shadow found a binding for a rejected query".into())}
            self.refutation.shadow_work(rid,self.graph.stats.residual_candidates-candidates,self.graph.key_lookup_calls.get()-keys);
        }
        self.refutation.finish(rid,self.graph.epoch,key,crate::refutation::Evidence {
            visited:proof.visited[first_visit..].iter().filter_map(|i|atoms.iter().position(|j|i==j)).collect(),
            reads:proof.reads[first_read..].to_vec(),delta_restrictions:proof.delta_restrictions-first_restriction,
            query_reads:proof.query_reads-first_query_read,shadowing:false,
        },self.graph.stats.residual_candidates-candidates,out.len()!=before);
        Ok(())
    }
    fn solve_context(&mut self,query:&[Atom],rid:usize,remaining:&mut [bool],env:&mut QueryEnv,
        epoch:u64,out:&mut Vec<Binding>,delta:Option<(usize,u64,Dependency)>,order:Option<&[usize]>)->Result<()> {
        let key=self.refutation_key(query,remaining,env);
        let atoms=remaining.iter().enumerate().filter_map(|(i,p)|p.then_some(i)).collect::<Vec<_>>();
        // Same first-occurrence variable numbering as the alpha-normalized key.
        let mut slots=vec![];
        for &i in &atoms { for a in &query[i].args {
            if let Arg::Slot(s)=a {if operand(a,env,&self.graph).is_none() && !slots.contains(s){slots.push(*s);}}
        }}
        let cached=self.search_contexts.get(&key,rid,&self.graph,&self.query_indexes);
        let mut replay=vec![];
        if let Some(answers)=&cached {
            self.search_contexts.stats.marker_anchored_hits+=u64::from(query.iter().enumerate().any(|(i,a)|
                !remaining[i] && matches!(a.call,Call::Table(t) if self.graph.program.views.markers.contains(&t))));
            let (pivot,seed_epoch,seed_witness)=delta.unwrap();
            for answer in answers.iter() {
                self.search_contexts.stats.replay_rows+=1;
                if answer.witnesses.iter().any(|(i,e,w)|(*e,*w)>(seed_epoch,seed_witness)
                    || ((*e,*w)==(seed_epoch,seed_witness) && atoms[*i]<pivot)) {
                    self.search_contexts.stats.delta_rejected_answers+=1;continue
                }
                let mut values=env.binding();
                for (&slot,value) in slots.iter().zip(&answer.values){values.insert(slot,value.clone());}
                let mut witnesses=env.witnesses.clone();let mut known=env.known.clone();
                let mut max_epoch=epoch;
                for (i,e,w) in &answer.witnesses {
                    let atom=atoms[*i];witnesses.record(atom,Some(*w));
                    known.record(atom, Some(*w));
                    max_epoch=max_epoch.max(*e);
                }
                replay.push(Binding{
                    #[cfg(feature = "iterative-bake")]
                    direct: None,
                    values,witnesses,known,epoch:max_epoch,parent:None,continuation:None,composition:None,compiled:order.is_some()});
            }
            self.search_contexts.stats.reused_bindings+=replay.len() as u64;
            if !self.search_contexts.observe {out.extend(replay);return Ok(())}
        }
        let start=out.len();let candidates=self.graph.stats.residual_candidates+self.continuation_stats.merge_candidate_rows;let keys=self.graph.key_lookup_calls.get();
        let old=env.witnesses.clone();
        // Track complete residual witnesses independently of storage provenance.
        env.witnesses=MatchedFacts::new(query.len(),true);
        let mut proof=crate::refutation::Evidence::default();
        let result=if let Some(order)=order {
            self.merge_points(rid,query,order,env,epoch,delta.unwrap(),out,Some(&mut proof))
        } else {self.solve_inner(query,rid,remaining,env,epoch,out,delta,Some(&mut proof))};
        env.witnesses=old.clone();result?;
        let mut answers=vec![];
        for b in &mut out[start..] {
            let mut witnesses=old.clone();let mut rows=vec![];
            for (i,&atom) in atoms.iter().enumerate() {
                if let Some(w)=b.witnesses.get(atom) {
                    witnesses.record(atom,Some(w));
                    if cached.is_none() && proof.delta_restrictions==0 {
                        self.search_contexts.stats.build_witness_reads+=1;
                        let row=self.graph.row(w)?.ok_or("context witness disappeared inside sealed search")?;
                        rows.push((i,row.epoch,w));
                    }
                }
            }
            b.witnesses=witnesses;
            if cached.is_none() && proof.delta_restrictions==0 {
                answers.push(crate::search_context::Answer{values:slots.iter().map(|s|b.values[s].clone()).collect(),witnesses:rows});
            }
        }
        if cached.is_some() {
            let logical=|bs:&[Binding]|bs.iter().map(|b|(b.values.clone(),b.epoch)).collect::<BTreeSet<_>>();
            if logical(&out[start..])!=logical(&replay){return Err("presearch context shadow binding mismatch".into())}
            self.search_contexts.stats.observed_saved_candidates+=self.graph.stats.residual_candidates+self.continuation_stats.merge_candidate_rows-candidates;
            self.search_contexts.stats.observed_saved_keys+=self.graph.key_lookup_calls.get()-keys;
            *self.search_contexts.stats.observed_saved_candidates_by_rule.entry(rid).or_default()+=self.graph.stats.residual_candidates+self.continuation_stats.merge_candidate_rows-candidates;
            *self.search_contexts.stats.observed_saved_keys_by_rule.entry(rid).or_default()+=self.graph.key_lookup_calls.get()-keys;
        } else if proof.delta_restrictions>0 {self.search_contexts.stats.incomplete+=1;}
        else if answers.is_empty(){self.search_contexts.stats.empty+=1;}
        else {self.search_contexts.insert(key,answers,proof.reads,rid,self.graph.epoch);}
        Ok(())
    }
    fn solve_inner(
        &mut self,
        query: &[Atom],
        rid: usize,
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
        mut evidence: Option<&mut crate::refutation::Evidence>,
    ) -> Result<()> {
        if !self.refutation.enabled && !self.search_contexts.enabled && self.local_known(rid,env) {
            self.local_sat.stats.inner_receipts+=1;
            self.local_sat.stats.skipped_remaining_table_atoms+=query.iter().zip(remaining.iter()).filter(|(a,p)|**p && matches!(a.call,Call::Table(_))).count() as u64;
            return Ok(())
        }
        if !remaining.iter().any(|pending| *pending) {
            out.push(Binding {
                #[cfg(feature = "iterative-bake")]
                direct: None,
                composition: None,
                witnesses: env.witnesses.clone(),
                known:env.known.clone(),
                values: env.binding(),
                epoch,
                parent: None,
                continuation: None,
                compiled: false,
            });
            return Ok(());
        }
        let mut best = None;
        for (i, atom) in query.iter().enumerate().filter(|(i, _)| remaining[*i]) {
            let bound = atom
                .args
                .iter()
                .map(|a| operand(a, env, &self.graph))
                .collect::<Vec<_>>();
            let key_bound = bound[..bound.len() - 1].iter().all(Option::is_some);
            let cost = match atom.call {
                Call::Primitive(..) if key_bound => 0,
                Call::Primitive(..) => continue,
                Call::Table(_) if key_bound => 1,
                Call::Table(t) => {
                    2 + if !self.indexed_queries {
                        self.graph.witness_count(t)
                    } else if self.column_queries {
                        let n = self.query_indexes.count(&self.graph, t, &bound)?;
                        env.domain(i).map_or(n, |d| n.min(d.len()))
                    } else {
                        self.graph.candidate_count(t, &bound)
                    }
                }
            };
            if best.is_none_or(|b| (cost, i) < b) {
                best = Some((cost, i));
            }
        }
        let chosen = best.ok_or("unbound primitive query cycle")?.1;
        remaining[chosen] = false;
        let atom = &query[chosen];
        if self.refutation.cores {if let Some(proof)=evidence.as_deref_mut(){proof.visited.push(chosen);}}
        match &atom.call {
            Call::Primitive(name, output) => {
                let vals = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &env, &self.graph).ok_or("unbound primitive input"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if name.starts_with("cell-key-") {
                    self.graph.stats.key_operations += 1;
                }
                if let Some(v) = value::primitive(
                    name,
                    &vals,
                    output,
                    &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                )? {
                    let mark = env.trail.len();
                    if bind(atom.args.last().unwrap(), &v, env, &self.graph) {
                        self.recurse(
                            query,
                            rid,
                            remaining,
                            env,
                            epoch,
                            out,
                            delta,
                            evidence.as_deref_mut(),
                        )?;
                    }
                    env.rollback(mark);
                }
            }
            Call::Table(t) => {
                let depth=if self.refutation_depth.enabled(){query.iter().enumerate().filter(|(i,a)|!remaining[*i]&&matches!(a.call,Call::Table(_))).count()}else{0};
                self.refutation_depth.read(depth);
                if let Some(proof) = evidence.as_deref_mut() {
                    proof.query_reads += 1;
                    let bound = atom
                        .args
                        .iter()
                        .map(|a| operand(a, env, &self.graph))
                        .collect::<Vec<_>>();
                    proof
                        .reads
                        .push(self.query_indexes.read_stamp(&self.graph, *t, &bound)?);
                }
                let key = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| operand(a, &env, &self.graph))
                    .collect::<Option<Vec<_>>>();
                let complete_key = key.is_some();
                let witnesses = if let Some(child) = env.domain(chosen).filter(|d| d.len() <= 1) {
                    self.free_join.stats.subtrie_uses += 1;
                    child.clone()
                } else if let Some(key) = key {
                    Rc::new(
                        self.graph
                            .lookup(*t, &key)?
                            .into_iter()
                            .map(|r| r.witness)
                            .collect(),
                    )
                } else {
                    let bound = atom
                        .args
                        .iter()
                        .map(|a| operand(a, env, &self.graph))
                        .collect::<Vec<_>>();
                    if !self.indexed_queries {
                        Rc::new(self.graph.witnesses(*t))
                    } else if self.column_queries {
                        let num_bound = bound.iter().filter(|v|v.is_some()).count();
                        if let Some(domain) = env.domains.iter().rev().find(|d| d.atom==chosen)
                            .filter(|d|!self.free_join.entry_mode() || num_bound<=d.projected_columns) {
                            self.free_join.stats.subtrie_uses += 1;
                            domain.rows.clone()
                        } else {
                            self.query_indexes.candidates(&self.graph, *t, &bound)?
                        }
                    } else {
                        Rc::new(self.graph.candidates(*t, &bound))
                    }
                };
                let mut witnesses = witnesses;
                if !complete_key
                    && witnesses.len() >= crate::range_index::MIN_DOMAIN
                    && self.column_queries
                    && self.indexed_queries
                    && evidence.is_none()
                    && !self.refutation.enabled
                    && !self.search_contexts.enabled
                    && self.local_run.is_none()
                    && let Some(index) = &mut self.range_index
                {
                    for guard in &self.graph.program.physical.ranges[rid][chosen] {
                        if !remaining[guard.atom] || operand(&atom.args[guard.column], env, &self.graph).is_some() {
                            continue;
                        }
                        let Some(Value::Int(limit)) = operand(&guard.threshold, env, &self.graph) else {
                            continue;
                        };
                        let prefix = atom.args[..atom.args.len() - 1].iter().enumerate()
                            .find_map(|(i, a)| operand(a, env, &self.graph).map(|v| (i, v)));
                        if let Some(prefix) = prefix {
                            witnesses = index.select(
                                &self.graph, *t, prefix, guard.column, guard.bound, limit, &witnesses,
                            )?;
                            break;
                        }
                    }
                }
                let groups = if self.free_join.enabled
                    && !complete_key
                    && self.column_queries
                    && self.indexed_queries
                    && evidence.is_none()
                    && !self.refutation.enabled
                    && !self.search_contexts.enabled
                    && self.local_run.is_none()
                {
                    self.free_join.partition(
                        &self.graph,
                        &mut self.query_indexes,
                        rid,
                        query,
                        chosen,
                        remaining,
                        env,
                        &witnesses,
                    )?
                } else {
                    None
                };
                if let Some(groups) = groups {
                    for group in groups {
                        let base = env.domains.len();
                        env.domains.extend(group.probes);
                        self.scan_domain(
                            query,
                            rid,
                            chosen,
                            *t,
                            depth,
                            &group.cover,
                            remaining,
                            env,
                            epoch,
                            out,
                            delta,
                            None,
                        )?;
                        env.domains.truncate(base);
                    }
                } else {
                    self.scan_domain(
                        query,
                        rid,
                        chosen,
                        *t,
                        depth,
                        &witnesses,
                        remaining,
                        env,
                        epoch,
                        out,
                        delta,
                        evidence.as_deref_mut(),
                    )?;
                }
            }
        }
        remaining[chosen] = true;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn scan_domain(
        &mut self,
        query: &[Atom],
        rid: usize,
        chosen: usize,
        table: usize,
        depth: usize,
        witnesses: &[Dependency],
        remaining: &mut [bool],
        env: &mut QueryEnv,
        epoch: u64,
        out: &mut Vec<Binding>,
        delta: Option<(usize, u64, Dependency)>,
        mut evidence: Option<&mut crate::refutation::Evidence>,
    ) -> Result<()> {
        let atom = &query[chosen];
        for w in witnesses.iter().copied() {
            if !self.sieve.allow(rid, chosen, table, w, &self.query_indexes) {
                continue;
            }
            self.refutation_depth.row(depth);
            self.graph.stats.residual_candidates += 1;
            self.graph.stats.decoded_rows += 1;
            let Some(r) = self.graph.row(w)? else {
                continue;
            };
            // Disjoint partition by latest committed witness, then atom.
            // Within one RHS, later recipe positions (outer constructors)
            // own the match instead of their earlier constituent leaves.
            if r.hidden {
                continue;
            }
            if delta.is_some_and(|(pivot, epoch, witness)| {
                (r.epoch, r.witness) > (epoch, witness)
                    || ((r.epoch, r.witness) == (epoch, witness) && chosen < pivot)
            }) {
                if let Some(proof) = evidence.as_deref_mut() {
                    proof.delta_restrictions += 1;
                }
                continue;
            }
            let mark = env.trail.len();
            if atom
                .args
                .iter()
                .zip(r.values.iter())
                .all(|(a, v)| bind(a, v, env, &self.graph))
            {
                env.record(chosen, Some(r.witness));
                if self.refutation.enabled
                    && evidence.is_none()
                    && self.graph.program.views.markers.contains(&table)
                {
                    self.solve(query, rid, remaining, env, epoch.max(r.epoch), out, delta)?;
                } else {
                    self.recurse(
                        query,
                        rid,
                        remaining,
                        env,
                        epoch.max(r.epoch),
                        out,
                        delta,
                        evidence.as_deref_mut(),
                    )?;
                }
                env.record(chosen, None);
            }
            env.rollback(mark);
        }
        Ok(())
    }
    fn points(
        &mut self,
        rid: usize,
        query: &[Atom],
        order: &[usize],
        env: &mut QueryEnv,
        remaining: &mut [bool],
        epoch: &mut u64,
        delta: Option<(usize, u64, Dependency)>,
        allow_inverse: bool,
    ) -> Result<bool> {
        for &i in order {
            if !remaining[i] {
                continue;
            }
            // A completed effect is not a negative match proof. Do not publish
            // this short circuit into the separate refutation/query caches.
            if !self.refutation.enabled && !self.search_contexts.enabled && self.local_known(rid,env) {
                self.local_sat.stats.inner_receipts+=1;
                self.local_sat.stats.skipped_remaining_table_atoms+=query.iter().zip(remaining.iter()).filter(|(a,p)|**p && matches!(a.call,Call::Table(_))).count() as u64;
                return Ok(false)
            }
            let atom = &query[i];
            let args = atom.args[..atom.args.len() - 1]
                .iter()
                .map(|a| operand(a, env, &self.graph))
                .collect::<Option<Vec<_>>>();
            match &atom.call {
                Call::Primitive(name, output) => {
                    let Some(args) = args else { continue };
                    self.continuation_stats.point_primitives += 1;
                    if name.starts_with("cell-key-") {
                        self.graph.stats.key_operations += 1;
                    }
                    let Some(value) = value::primitive(
                        name,
                        &args,
                        output,
                        &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                    )?
                    else {
                        return Ok(false);
                    };
                    if !bind(atom.args.last().unwrap(), &value, env, &self.graph) {
                        return Ok(false);
                    }
                }
                Call::Table(t) => {
                    let row = if let Some(args) = args {
                        self.continuation_stats.point_reads += 1;
                        if let Some(domain) = env.domain(i).filter(|d| d.len() == 1) {
                            self.free_join.stats.entry_point_uses += 1;
                            self.graph.row(domain[0])?
                        } else {
                            self.graph.lookup_canonical(*t, &args)?
                        }
                    } else if allow_inverse
                        && matches!(
                            operand(atom.args.last().unwrap(), env, &self.graph),
                            Some(Value::Eq(_))
                        )
                    {
                        let bound = atom
                            .args
                            .iter()
                            .map(|a| operand(a, env, &self.graph))
                            .collect::<Vec<_>>();
                        match self.query_indexes.domain(&self.graph, *t, &bound)? {
                            crate::query::Domain::Empty => return Ok(false),
                            crate::query::Domain::Many => {
                                self.continuation_stats.inverse_ambiguous += 1;
                                continue;
                            }
                            crate::query::Domain::One(w) => {
                                self.continuation_stats.inverse_reads += 1;
                                self.graph.row(w)?
                            }
                        }
                    } else {
                        continue;
                    };
                    let Some(row) = row else { return Ok(false) };
                    self.graph.stats.decoded_rows += 1;
                    if row.hidden
                        || delta.is_some_and(|(pivot, stamp, w)| {
                            (row.epoch, row.witness) > (stamp, w)
                                || ((row.epoch, row.witness) == (stamp, w) && i < pivot)
                        })
                    {
                        return Ok(false);
                    }
                    if !atom
                        .args
                        .iter()
                        .zip(row.values.iter())
                        .all(|(a, v)| bind(a, v, env, &self.graph))
                    {
                        return Ok(false);
                    }
                    env.record(i, Some(row.witness));
                    *epoch = (*epoch).max(row.epoch);
                }
            }
            remaining[i] = false;
        }
        Ok(true)
    }
    fn composed_match(
        &mut self,
        rid: usize,
        anchor: usize,
        seed: &Row,
        env: &mut QueryEnv,
    ) -> Result<ComposedMatch> {
        if !self.compositions_enabled {
            return Ok(ComposedMatch::Fallback);
        }
        let start = Instant::now();
        let result = self.composed_match_inner(rid, anchor, seed, env);
        self.composition_stats.match_seconds += start.elapsed().as_secs_f64();
        result
    }
    fn composed_match_inner(
        &mut self,
        rid: usize,
        anchor: usize,
        seed: &Row,
        env: &mut QueryEnv,
    ) -> Result<ComposedMatch> {
        if !self.compositions_enabled || matches!(env.witnesses, MatchedFacts::Rows(_)) {
            return Ok(ComposedMatch::Fallback);
        }
        let Some(context) = self.graph.continuation_context(seed.witness) else {
            return Ok(ComposedMatch::Fallback);
        };
        if !context.completed {
            return Ok(ComposedMatch::Fallback);
        }
        let Some(edge) = self
            .graph
            .program
            .compositions
            .edges
            .get(&(context.template, context.op, rid, anchor))
            .cloned()
        else {
            return Ok(ComposedMatch::Fallback);
        };
        self.composition_stats.attempts += 1;
        let query = self.graph.program.rules[rid].query.clone();
        let mark = env.trail.len();
        let mut epoch = seed.epoch;
        for instruction in &edge.instructions {
            let atom = &query[instruction.atom];
            if let Some(columns) = &instruction.columns {
                let values = match self.graph.context_arguments(context, columns) {
                    Ok(v) => v,
                    Err(_) => {
                        env.rollback(mark);
                        for i in &edge.instructions {
                            env.record(i.atom, None);
                        }
                        self.composition_stats.unavailable += 1;
                        return Ok(ComposedMatch::Fallback);
                    }
                };
                self.composition_stats.known_atoms += 1;
                if !atom
                    .args
                    .iter()
                    .zip(values.iter())
                    .all(|(a, v)| bind(a, v, env, &self.graph))
                {
                    self.composition_stats.rejected += 1;
                    return Ok(ComposedMatch::Rejected);
                }
            } else {
                let key = atom.args[..atom.args.len() - 1]
                    .iter()
                    .map(|a| {
                        operand(a, env, &self.graph)
                            .ok_or_else(|| "unbound compiled port".to_owned())
                    })
                    .collect::<Result<Vec<_>>>()?;
                let value = match &atom.call {
                    Call::Table(t) => {
                        self.composition_stats.port_reads += 1;
                        let Some(row) = self.graph.lookup_canonical(*t, &key)? else {
                            self.composition_stats.rejected += 1;
                            return Ok(ComposedMatch::Rejected);
                        };
                        if row.hidden {
                            self.composition_stats.rejected += 1;
                            return Ok(ComposedMatch::Rejected);
                        }
                        epoch = epoch.max(row.epoch);
                        env.record(instruction.atom, Some(row.witness));
                        row.values.last().unwrap().clone()
                    }
                    Call::Primitive(name, output) => {
                        self.composition_stats.guard_checks += 1;
                        let Some(v) = value::primitive(
                            name,
                            &key,
                            output,
                            &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                        )?
                        else {
                            self.composition_stats.rejected += 1;
                            return Ok(ComposedMatch::Rejected);
                        };
                        v
                    }
                };
                if !bind(atom.args.last().unwrap(), &value, env, &self.graph) {
                    self.composition_stats.rejected += 1;
                    return Ok(ComposedMatch::Rejected);
                }
            }
        }
        // Every remaining key is a function of this complete anchor binding.
        // Other delta pivots still run and the existing binding map deduplicates
        // overlaps; no inverse alternative or late input is excluded.
        self.composition_stats.bindings += 1;
        let parent = if self.graph.program.rules[rid].direct.is_some() {
            match seed.witness {
                Dependency::Fact(f) => Some(f),
                _ => None,
            }
        } else {
            None
        };
        Ok(ComposedMatch::Matched(Binding {
            #[cfg(feature = "iterative-bake")]
            direct: None,
            composition: Some((edge.clone(), context)),
            witnesses: env.witnesses.clone(),
                known:env.known.clone(),
            values: env.binding(),
            epoch,
            parent,
            continuation: Some((
                edge.producer,
                context
                    .depth
                    .checked_add(1)
                    .ok_or("recipe depth exhausted")?,
                false,
            )),
            compiled: true,
        }))
    }
    fn continue_local(
        &mut self,
        rid: usize,
        pivot: usize,
        seed: &Row,
        env: &mut QueryEnv,
        remaining: &mut [bool],
        mut epoch: u64,
        wired: &mut bool,
    ) -> Result<(bool, u64, Option<(usize, u32)>)> {
        if !self.multistage_enabled || !self.local_projections {
            return Ok((true, epoch, None));
        }
        let Some(context) = self.graph.continuation_context(seed.witness) else {
            return Ok((true, epoch, None));
        };
        let Some(route) = self
            .graph
            .program
            .multi_stage
            .routes
            .get(&(context.template, context.op, rid, pivot))
            .cloned()
        else {
            return Ok((true, epoch, None));
        };
        let depth = context
            .depth
            .checked_add(1)
            .ok_or("recipe depth exhausted")?;
        self.continuation_stats.entered += 1;
        if !self.graph.cells.contains_key(&context.cell) {
            self.graph.view_stats.recovered_continuations += 1;
        }
        let query = self.graph.program.rules[rid].query.clone();
        for read in &route.reads {
            if !remaining[read.atom] {
                continue;
            }
            if !self.refutation.enabled && !self.search_contexts.enabled && self.local_known(rid,env) {
                self.local_sat.stats.inner_receipts+=1;
                self.local_sat.stats.skipped_remaining_table_atoms+=query.iter().zip(remaining.iter()).filter(|(a,p)|**p && matches!(a.call,Call::Table(_))).count() as u64;
                return Ok((false,epoch,None))
            }
            let atom = &query[read.atom];
            let bound = atom
                .args
                .iter()
                .map(|a| operand(a, env, &self.graph))
                .collect::<Vec<_>>();
            let full_key = bound[..bound.len() - 1]
                .iter()
                .cloned()
                .collect::<Option<Vec<_>>>();
            let expected = if full_key.is_none() {
                match self.query_indexes.domain(&self.graph, read.table, &bound)? {
                    crate::query::Domain::Empty => return Ok((false, epoch, None)),
                    crate::query::Domain::Many => {
                        self.continuation_stats.ambiguous_reads += 1;
                        continue;
                    }
                    crate::query::Domain::One(w) => Some(w),
                }
            } else {
                None
            };
            if (self.graph.slot_transfer.sibling_routes()
                || self.graph.slot_transfer == SlotTransfer::LocalOnly)
                && let Some((witness, stamp)) = read.wire(
                    &mut self.graph, context, seed.witness, &mut env.values, &mut env.trail, expected,
                )?
            {
                let owner = (seed.epoch, seed.witness);
                if (stamp, witness) > owner || ((stamp, witness) == owner && read.atom < pivot) {
                    return Ok((false, epoch, None));
                }
                *wired = true;
                self.graph.stats.slot_transfer.local_atoms += 1;
                env.record(read.atom, Some(witness));
                remaining[read.atom] = false;
                epoch = epoch.max(stamp);
                self.continuation_stats.projected_atoms += 1;
                continue;
            }
            // A live recipe slot can supply the fields directly, without creating
            // an intermediate Row or rediscovering its constructor in a table.
            let mut local = None;
            if let Some(op) = read.effect {
                let f = self.graph.context_fact(context, op);
                if expected.is_none_or(|w| w == Dependency::Fact(f)) {
                    #[cfg(feature = "slot-transfer-audit")]
                    let _row_sample = crate::transfer_audit::Sample::begin(crate::transfer_audit::Region::SiblingFallback);
                    if let Some(row) = self.graph.fact(f)? {
                        if !row.hidden
                            && row.table == read.table
                            && full_key
                                .as_ref()
                                .is_none_or(|key| row.values[..key.len()] == *key)
                        {
                            self.continuation_stats.local_effect_reads += 1;
                            local = Some((row.values, row.epoch, row.witness));
                        }
                    }
                }
            }
            if local.is_none() {
                let key = if let Some(key) = full_key {
                    key
                } else {
                    let Ok(key) = self
                        .graph
                        .context_arguments(context, &read.columns[..read.columns.len() - 1])
                    else {
                        self.continuation_stats.unavailable_sources += 1;
                        continue;
                    };
                    key
                };
                self.continuation_stats.keyed_reads += 1;
                let row = self.graph.lookup(read.table, &key)?;
                let Some(row) = row else {
                    if expected.is_none() {
                        return Ok((false, epoch, None));
                    }
                    self.continuation_stats.unavailable_sources += 1;
                    continue;
                };
                if expected.is_some_and(|w| w != row.witness) {
                    self.continuation_stats.unavailable_sources += 1;
                    continue;
                }
                if row.hidden {
                    return Ok((false, epoch, None));
                }
                local = Some((row.values, row.epoch, row.witness));
            }
            let (values, stamp, witness) = local.unwrap();
            if (stamp, witness) > (seed.epoch, seed.witness)
                || (read.atom < pivot && (stamp, witness) == (seed.epoch, seed.witness))
            {
                return Ok((false, epoch, None));
            }
            let mark = env.trail.len();
            if !atom
                .args
                .iter()
                .zip(values.iter())
                .all(|(a, v)| bind(a, v, env, &self.graph))
            {
                env.rollback(mark);
                return Ok((false, epoch, None));
            }
            env.record(read.atom, Some(witness));
            remaining[read.atom] = false;
            epoch = epoch.max(stamp);
            self.continuation_stats.projected_atoms += 1;
        }
        Ok((true, epoch, Some((route.producer, depth))))
    }
    fn match_union_graft(
        &mut self,
        rid: usize,
        pivot: usize,
        seed: &Row,
        env: &mut QueryEnv,
        out: &mut Vec<Binding>,
    ) -> Result<bool> {
        let Some((other, rows)) = self.union_grafts.route(&self.graph, rid, pivot, seed) else {
            return Ok(false);
        };
        let query = self.graph.program.rules[rid].query.clone();
        let before = out.len();
        for row in rows {
            self.union_grafts.stats.candidate_pairs += 1;
            if row.hidden
                || (row.epoch, row.witness) > (seed.epoch, seed.witness)
                || ((row.epoch, row.witness) == (seed.epoch, seed.witness) && other < pivot)
            {
                continue;
            }
            let mark = env.trail.len();
            if query[other]
                .args
                .iter()
                .zip(row.values.iter())
                .all(|(a, v)| bind(a, v, env, &self.graph))
            {
                env.record(other, Some(row.witness));
                let mut remaining = (0..query.len())
                    .map(|i| i != pivot && i != other)
                    .collect::<Vec<_>>();
                // Only primitives remain. Effects still enter the ordinary
                // StagePatch and are invisible until the source Run publishes.
                self.solve_inner(
                    &query,
                    rid,
                    &mut remaining,
                    env,
                    seed.epoch.max(row.epoch),
                    out,
                    Some((pivot, seed.epoch, seed.witness)),
                    None,
                )?;
                env.record(other, None);
            }
            env.rollback(mark);
        }
        self.union_grafts.stats.bindings += (out.len() - before) as u64;
        if out.len() > before {
            *self
                .union_grafts
                .stats
                .bindings_by_rule
                .entry(rid)
                .or_default() += (out.len() - before) as u64;
        }
        Ok(true)
    }
    // Fixed constructor-port wiring from the checked merge program. All alias
    // alternatives remain explicit and use the original latest-witness split.
    // No effects run here: complete bindings enter the normal sealed StagePatch.
    fn merge_points(&mut self,rid:usize,query:&[Atom],order:&[usize],env:&mut QueryEnv,epoch:u64,
        delta:(usize,u64,Dependency),out:&mut Vec<Binding>,mut evidence:Option<&mut crate::refutation::Evidence>)->Result<()> {
        if !self.refutation.enabled && !self.search_contexts.enabled && self.local_known(rid,env) {
            self.local_sat.stats.inner_receipts+=1;
            self.local_sat.stats.skipped_remaining_table_atoms+=order.iter().filter(|i|matches!(query[**i].call,Call::Table(_))).count() as u64;
            return Ok(())
        }
        let Some((&i,tail))=order.split_first() else {
            out.push(Binding{
                #[cfg(feature = "iterative-bake")]
                direct: None,
                composition:None,witnesses:env.witnesses.clone(),known:env.known.clone(),values:env.binding(),epoch,parent:None,continuation:None,compiled:true});
            return Ok(())
        };
        let a=&query[i];let mark=env.trail.len();
        if self.refutation.cores {if let Some(proof)=evidence.as_deref_mut(){proof.visited.push(i);}}
        match &a.call {
            Call::Primitive(name,sort)=>{
                self.continuation_stats.merge_guard_checks+=1;
                let args=a.args[..a.args.len()-1].iter().map(|a|operand(a,env,&self.graph).ok_or_else(||"merge guard input".to_string())).collect::<Result<Vec<_>>>()?;
                if name.starts_with("cell-key-"){self.graph.stats.key_operations+=1;}
                if let Some(v)=value::primitive(name,&args,sort,&self.graph.program.artifact["source_core"]["structural_key"]["nodes"])? {
                    if bind(a.args.last().unwrap(),&v,env,&self.graph){self.merge_points(rid,query,tail,env,epoch,delta,out,evidence.as_deref_mut())?;}
                }
            },
            Call::Table(t)=>{
                let depth=if self.refutation_depth.enabled(){query.iter().filter(|a|matches!(a.call,Call::Table(_))).count()-order.iter().filter(|i|matches!(query[**i].call,Call::Table(_))).count()+1}else{0};
                self.refutation_depth.read(depth);
                self.continuation_stats.merge_port_reads+=1;
                if let Some(proof)=evidence.as_deref_mut() {
                    let bound=a.args.iter().map(|a|operand(a,env,&self.graph)).collect::<Vec<_>>();
                    proof.reads.push(self.query_indexes.read_stamp(&self.graph,*t,&bound)?);proof.query_reads+=1;
                }
                let key=a.args[..a.args.len()-1].iter().map(|a|operand(a,env,&self.graph)).collect::<Option<Vec<_>>>();
                let rows=if let Some(key)=key {self.graph.lookup(*t,&key)?.into_iter().collect::<Vec<_>>()}else{
                    let Some(Value::Eq(id))=operand(a.args.last().unwrap(),env,&self.graph) else {return Err("merge port has no fixed key or output".into())};
                    // Reuse the exact versioned constructor result-column index.
                    // Do not traverse all incoming uses of this eclass for each
                    // comparison, nor build a second untracked context cache.
                    let mut bound=vec![None;a.args.len()];*bound.last_mut().unwrap()=Some(Value::Eq(id));
                    let refs=self.query_indexes.candidates(&self.graph,*t,&bound)?;
                    refs.iter().map(|w|self.graph.row(*w)).collect::<Result<Vec<_>>>()?.into_iter().flatten().collect()
                };
                for row in rows {
                    self.refutation_depth.row(depth);
                    self.continuation_stats.merge_candidate_rows+=1;self.graph.stats.decoded_rows+=1;
                    if row.hidden {continue}
                    if (row.epoch,row.witness)>(delta.1,delta.2) || ((row.epoch,row.witness)==(delta.1,delta.2) && i<delta.0){
                        if let Some(proof)=evidence.as_deref_mut(){proof.delta_restrictions+=1;}continue
                    }
                    if a.args.iter().zip(row.values.iter()).all(|(a,v)|bind(a,v,env,&self.graph)) {
                        env.record(i,Some(row.witness));self.merge_points(rid,query,tail,env,epoch.max(row.epoch),delta,out,evidence.as_deref_mut())?;env.record(i,None);
                    }
                    env.rollback(mark);
                }
            }
        }
        env.rollback(mark);Ok(())
    }
    fn refute_compiled_entry(&mut self,rid:usize,pivot:usize,row:&Row,env:&mut QueryEnv,out:&mut Vec<Binding>,query:&[Atom],order:&[usize])->Result<()> {
        self.refutation.stats.compiled_attempts+=1;
        let remaining=(0..query.len()).map(|i|i!=pivot).collect::<Vec<_>>();
        let atoms=remaining.iter().enumerate().filter_map(|(i,p)|p.then_some(i)).collect::<Vec<_>>();
        let key=self.refutation_key(query,&remaining,env);
        let hit=self.refutation.reject(rid,&key,&self.query_indexes,&self.graph).is_some();
        if hit {self.refutation.stats.compiled_hits+=1;if !self.refutation.shadow{self.refutation_depth.reject(2);return Ok(())}}
        let before=out.len();let candidates=self.continuation_stats.merge_candidate_rows;let keys=self.graph.key_lookup_calls.get();
        let mut proof=crate::refutation::Evidence::default();
        self.merge_points(rid,query,order,env,row.epoch,(pivot,row.epoch,row.witness),out,Some(&mut proof))?;
        if hit {
            if out.len()!=before{return Err("compiled port refutation rejected a valid binding".into())}
            self.refutation.shadow_work(rid,self.continuation_stats.merge_candidate_rows-candidates,self.graph.key_lookup_calls.get()-keys);
        }
        proof.visited=proof.visited.iter().filter_map(|i|atoms.iter().position(|j|i==j)).collect();
        self.refutation.finish(rid,self.graph.epoch,key,proof,self.continuation_stats.merge_candidate_rows-candidates,out.len()!=before);
        Ok(())
    }
    fn match_merge_program(&mut self,rid:usize,pivot:usize,row:&Row,env:&mut QueryEnv,out:&mut Vec<Binding>)->Result<bool>{
        if !self.graph.merge_programs_enabled{return Ok(false)}
        let Some(order)=self.graph.program.merge_programs.entries.get(&(rid,pivot)).cloned() else {return Ok(false)};
        self.continuation_stats.merge_context_entries+=1;
        let before=out.len();let query=self.graph.program.rules[rid].query.clone();
        if self.refutation.enabled && self.refutation.cores && self.refutation.compiled_entries {
            self.refute_compiled_entry(rid,pivot,row,env,out,&query,&order)?;
        }else if self.search_contexts.enabled && !self.refutation.enabled && !self.sieve.enabled && order.iter().any(|i|matches!(query[*i].call,Call::Table(_))) {
            let mut remaining=(0..query.len()).map(|i|i!=pivot).collect::<Vec<_>>();
            self.solve_context(&query,rid,&mut remaining,env,row.epoch,out,Some((pivot,row.epoch,row.witness)),Some(&order))?;
        }else{self.merge_points(rid,&query,&order,env,row.epoch,(pivot,row.epoch,row.witness),out,None)?;}
        let n=(out.len()-before) as u64;self.continuation_stats.merge_bindings+=n;
        *self.continuation_stats.merge_bindings_by_rule.entry(rid).or_default()+=n;
        Ok(true)
    }
    fn matches(&mut self, rid: usize, probe: bool) -> Result<Vec<Binding>> {
        let rule = self.graph.program.rules[rid].clone();
        let full_witnesses = !self.graph.marker_fields_copied
            && self
                .graph
                .program
                .views
                .layouts
                .get(&rule.template)
                .is_some_and(|l| l.reference_only);
        // Witnesses are storage provenance, not part of logical LHS matching.
        // Copied markers without takeover do not consume them at all.
        let witness_slots = if self.graph.views_enabled
            && !probe
            && self
                .graph
                .program
                .views
                .layouts
                .get(&rule.template)
                .is_some_and(|l| {
                    !l.carriers.is_empty()
                        && (self.graph.absorption_enabled
                            || (l.reference_only && !self.graph.marker_fields_copied))
                }) {
            rule.query.len()
        } else {
            0
        };
        if !probe && rule.seminaive && !std::mem::replace(&mut self.dirty[rid],false) {
            return Ok(vec![]);
        }
        // No join can succeed if a required relation is empty. This is a query
        // property, not a Cell proof or a prediction about future stages.
        if self.indexed_queries
            && rule.query.iter().any(|a| match a.call {
                Call::Table(t) => self.graph.witness_count(t) == 0,
                _ => false,
            })
        {
            if !probe {
                self.pending[rid].clear();
            }
            return Ok(vec![]);
        }
        let query_facts = match self.graph.known_mode.as_str() {
            "observe" | "on" if self.graph.program.known_results.queries.contains(&rid) => {
                QueryFacts::All(vec![None; rule.query.len()])
            }
            "static" if !probe => self.stage_spine.alias_query_facts(rid),
            _ => QueryFacts::None,
        };
        let scheduled_entry = !probe && self.entry_schedule.accepts(rid);
        // A necessary join summary can reject an empty domain before either route.
        let early_sieve = scheduled_entry && self.sieve.enabled && self.delta_queries
            && rule.direct.is_none() && !self.retained.accepts(rid);
        if early_sieve {
            self.sieve.prepare(rid, &self.graph, &mut self.query_indexes)?;
            if self.sieve.empty_join(rid) {
                self.pending[rid].clear();
                return Ok(vec![]);
            }
        }
        let early_join = scheduled_entry
            && self.delta_queries
            && rule.direct.is_none()
            && !self.retained.accepts(rid)
            && self.local_run.is_none()
            && !self.refutation.enabled
            && !self.search_contexts.enabled
            && self.free_join.has_entry(rid);
        let route_spine = scheduled_entry && self.delta_queries && self.stage_spine.enabled;
        let active_spine = route_spine && self.stage_spine.active_for_rule(rid);
        let mut spine_routes = Vec::new();
        let mut entry_domains = Vec::new();
        let scheduled = if scheduled_entry {
            let watermark = self.watermarks[rid];
            Some(if early_join || route_spine {
                self.entry_schedule
                    .batches(&self.graph, rid, watermark, |batch, out| {
                        let atom = batch.atom();
                        let start = out.len();
                        let claimed = spine_routes.len();
                        let cursors = if active_spine {
                            self.stage_spine.scheduled_cursors(rid, atom, batch.table)
                        } else {
                            None
                        };
                        let inverted = if early_join && self.free_join.planned {
                            self.free_join
                                .reverse(&self.graph, &mut self.query_indexes, rid, batch)?
                        } else {
                            None
                        };
                        let mut deliver = |w| {
                            if early_sieve
                                && !self
                                    .sieve
                                    .allow(rid, atom, batch.table, w, &self.query_indexes)
                            {
                                return Ok(());
                            }
                            let packet = batch.packet(w).expect("current delta entry");
                            if let Some(cursor) = cursors.and_then(|entries| entries.get(&w)) {
                                spine_routes.push((out.len(), cursor.clone()));
                                // Keep domain slots aligned with the shared seed vector.
                                if early_join {
                                    entry_domains.push(Vec::new());
                                }
                            } else if early_join {
                                let Some(domains) = self.free_join.admit(
                                    &self.graph, &mut self.query_indexes, rid, atom, &packet.row,
                                )? else {
                                    return Ok(());
                                };
                                entry_domains.push(domains);
                            }
                            out.push(batch.seed(packet));
                            Ok::<(), String>(())
                        };
                        if let Some(rows) = inverted {
                            for w in rows {
                                deliver(w)?
                            }
                        } else {
                            for &(_, w) in batch.delta() {
                                deliver(w)?
                            }
                        }
                        if active_spine {
                            let ordinary = (out.len() - start) - (spine_routes.len() - claimed);
                            self.stage_spine.record_fallbacks(rid, atom, ordinary as u64);
                        }
                        Ok(())
                    })?
            } else if early_sieve {
                self.entry_schedule
                    .seeds_where(&self.graph, rid, watermark, |atom, table, w| {
                        self.sieve.allow(rid, atom, table, w, &self.query_indexes)
                    })?
            } else {
                self.entry_schedule.seeds(&self.graph, rid, watermark)?
            })
        } else {
            None
        };
        let mut stage_covered = BTreeSet::new();
        let mut found = vec![];
        if !probe && rule.seminaive && self.delta_queries {
            let watermark = self.watermarks[rid];
            let coverage = match &scheduled {
                Some(seeds) if route_spine => self.stage_spine.answers_routed(
                    &mut self.graph, rid, seeds, spine_routes, watermark,
                )?,
                Some(_) => None,
                None => self.stage_spine.answers(&mut self.graph, rid, &self.pending[rid], watermark)?,
            };
            if let Some(coverage)=coverage {
                found = coverage.answers.into_iter().map(|a| {
                    let mut witnesses=MatchedFacts::new(witness_slots,full_witnesses);
                    let mut known=query_facts.clone();
                    for (i,w) in a.witnesses.into_iter().enumerate() {witnesses.record(i,w);known.record(i,w);}
                    if a.parameter_parent.is_some() {
                        self.graph.stats.slot_transfer.rhs_parent_bindings += 1;
                    }
                    Binding {
                        #[cfg(feature = "iterative-bake")]
                        direct: a.direct,
                        values: rule.inputs.iter().copied().zip(a.values).collect(),
                        witnesses, known, composition: None,
                        parent: a.parameter_parent.filter(|_| self.graph.slot_transfer != SlotTransfer::ParentCheck),
                        continuation: None, epoch: a.epoch, compiled: true,
                    }
                }).collect();
                if coverage.complete {
                    self.pending[rid].clear();
                    return Ok(found);
                }
                stage_covered = coverage.covered;
            }
        }
        if !probe && self.retained.accepts(rid) {
            let answers=self.retained.matches(&mut self.graph,&mut self.query_indexes,rid,self.watermarks[rid])?;
            self.pending[rid].clear();
            return Ok(answers.into_iter().map(|a| {
                let mut witnesses=MatchedFacts::new(witness_slots,full_witnesses);
                let mut known=query_facts.clone();
                for (i,w) in a.witnesses.into_iter().enumerate() {
                    witnesses.record(i,w);
                    known.record(i,w);
                }
                Binding{
                    #[cfg(feature = "iterative-bake")]
                    direct: None,
                    values:rule.inputs.iter().copied().zip(a.values).collect(),witnesses,known,
                    composition:None,parent:None,continuation:None,epoch:a.epoch,compiled:true}
            }).collect())
        }
        if !early_sieve {
            self.sieve.prepare(rid, &self.graph, &mut self.query_indexes)?;
        }
        let shared_seeds = !probe && rule.seminaive && self.delta_queries && self.stage_spine.shared();
        if let Some(anchor) = rule.direct {
            let atom = &rule.query[anchor];
            let Call::Table(t) = atom.call else {
                unreachable!()
            };
            let candidates = if let Some(seeds)=&scheduled {seeds.iter().map(|s|s.packet.row.witness).collect()}
            else if probe || !rule.seminaive {
                self.graph.witnesses(t)
            } else {
                std::mem::take(&mut self.pending[rid]).into_iter().collect()
            };
            for (seed_index,w) in candidates.into_iter().enumerate() {
                if stage_covered.contains(&(anchor, w)) { continue; }
                let supplied=scheduled.as_ref().map(|seeds|seeds[seed_index].clone());
                let row = if let Some(seed)=&supplied {SeedRow::Shared(seed.packet.row.clone())} else {
                    let row = if shared_seeds { self.stage_spine.seed_row(&mut self.graph, w)?.map(SeedRow::Shared) } else {
                        self.graph.stats.decoded_rows += 1;self.graph.row(w)?.map(SeedRow::Owned)
                    };
                    let Some(row)=row else {continue};row
                };
                if row.hidden || (!probe && rule.seminaive && row.epoch < self.watermarks[rid]) { continue; }
                let mut env = if let Some(seed)=supplied {QueryEnv::shared(seed,witness_slots,full_witnesses,query_facts.clone())}
                    else {QueryEnv::new(self.graph.program.templates[rule.template].sorts.len(),witness_slots,full_witnesses,query_facts.clone())};
                let live = if scheduled.is_some() {true} else if shared_seeds { self.stage_spine.bind_seed(&self.graph,rid,anchor,&row,env.values.owned_mut()) }
                    else {self.graph.stats.direct_candidates+=1;atom.args.iter().zip(row.values.iter()).all(|(a,v)|bind(a,v,&mut env,&self.graph))};
                if live {
                    if self.local_known(rid,&env){self.local_sat.stats.seed_receipts+=1;continue;}
                    env.record(anchor, Some(row.witness));
                    self.attach_recipe_context(rid, anchor, &row, &mut env);
                    if !probe {
                        match self.composed_match(rid, anchor, &row, &mut env)? {
                            ComposedMatch::Matched(b) => {
                                self.graph.stats.direct_bindings += 1;
                                found.push(b);
                                continue;
                            }
                            ComposedMatch::Rejected => continue,
                            ComposedMatch::Fallback => {}
                        }
                    }
                    let n = found.len();
                    let mut remaining = (0..rule.query.len())
                        .map(|i| i != anchor)
                        .collect::<Vec<_>>();
                    let mut epoch = row.epoch;
                    if self.reject_entry(rid, &rule.query, &remaining, &env) {
                        continue;
                    }
                    if self.multistage_enabled && self.point_programs {
                        if let Some(order) = self
                            .graph
                            .program
                            .multi_stage
                            .entries
                            .get(&(rid, anchor))
                            .cloned()
                        {
                            if !self.points(
                                rid,
                                &rule.query,
                                &order,
                                &mut env,
                                &mut remaining,
                                &mut epoch,
                                None,
                                false,
                            )? {
                                continue;
                            }
                        }
                    }
                    if self.multistage_enabled && !remaining.iter().any(|v| *v) {
                        found.push(Binding {
                            #[cfg(feature = "iterative-bake")]
                            direct: None,
                            composition: None,
                            witnesses: env.witnesses.clone(),
                known:env.known.clone(),
                            values: env.binding(),
                            epoch,
                            parent: None,
                            continuation: None,
                            compiled: true,
                        });
                    } else {
                        self.solve(
                            &rule.query,
                            rid,
                            &mut remaining,
                            &mut env,
                            epoch,
                            &mut found,
                            None,
                        )?;
                    }
                    if let Some(f) = self.graph.origin(row.witness) {
                        for b in &mut found[n..] {
                            if matches!(row.witness, Dependency::Fact(_)) {
                                b.parent = Some(f);
                            }
                            let source = self
                                .graph
                                .continuation_context(row.witness)
                                .ok_or("missing continuation context")?;
                            if self.multistage_enabled {
                                if let Some(route) = self.graph.program.multi_stage.routes.get(&(
                                    source.template,
                                    source.op,
                                    rid,
                                    anchor,
                                )) {
                                    b.continuation = Some((
                                        route.producer,
                                        source
                                            .depth
                                            .checked_add(1)
                                            .ok_or("recipe depth exhausted")?,
                                        false,
                                    ));
                                }
                            }
                        }
                    }
                    self.graph.stats.direct_bindings += (found.len() - n) as u64;
                }
            }
        } else if self.delta_queries
            && rule.seminaive
            && !probe
            && rule.query.iter().any(|a| matches!(a.call, Call::Table(_)))
        {
            let watermark = self.watermarks[rid];
            let changed = if scheduled.is_some(){vec![]}else{std::mem::take(&mut self.pending[rid])
                .into_iter().map(|w|if shared_seeds{self.stage_spine.seed_row(&mut self.graph,w).map(|r|r.map(SeedRow::Shared))}else{self.graph.row(w).map(|r|r.map(SeedRow::Owned))})
                .collect::<Result<Vec<_>>>()?.into_iter().flatten().filter(|r|!r.hidden&&r.epoch>=watermark).collect::<Vec<_>>()};
            let use_kernel = self.multistage_enabled
                && self.indexed_queries
                && self.free_join.planned
                && scheduled.is_some()
                && !self.graph.views_enabled
                && !self.compositions_enabled
                && self.local_run.is_none()
                && !self.search_contexts.enabled
                && !self.refutation.enabled
                && !self.union_grafts.enabled
                && self.graph.slot_transfer == SlotTransfer::Off;
            let mut regs = None;
            for (pivot, atom) in rule.query.iter().enumerate() {
                let Call::Table(table)=atom.call else {continue};
                let kernel = use_kernel
                    .then(|| self.graph.program.kernels[rid][pivot].clone())
                    .flatten()
                    .filter(|k| {
                        self.range_index.is_none()
                            || !k.steps.iter().any(|s| {
                                matches!(s.access, crate::row_kernel::Access::Expand)
                                    && !self.graph.program.physical.ranges[rid][s.atom].is_empty()
                            })
                    });
                let candidates=scheduled.as_ref().into_iter().flat_map(|seeds|seeds.iter().enumerate().filter(move|(_,s)|s.atom==pivot).map(|(i,s)|(Some((i,s)),s.packet.row.as_ref())))
                    .chain(changed.iter().filter(|r|r.table==table).map(|r|(None,&**r)));
                for (supplied,row) in candidates {
                    if stage_covered.contains(&(pivot,row.witness)){continue;}
                    if !early_sieve && !self.sieve.allow(rid,pivot,table,row.witness,&self.query_indexes){continue;}
                    if !shared_seeds && scheduled.is_none(){self.graph.stats.residual_candidates+=1;self.graph.stats.decoded_rows+=1;}
                    if let (Some(kernel), Some((i, _))) = (&kernel, supplied) {
                        let regs = regs.get_or_insert_with(|| execute_kernel::Registers::new(&rule));
                        let domains = if early_join {
                            entry_domains[i].as_slice()
                        } else {
                            &[]
                        };
                        regs.start(self, &rule, kernel, row, domains, &query_facts, &mut found)?;
                        continue;
                    }
                    let mut env = if let Some((i, seed)) = supplied {
                        let mut env = QueryEnv::shared(
                            seed.clone(),
                            witness_slots,
                            full_witnesses,
                            query_facts.clone(),
                        );
                        if early_join {
                            env.domains = std::mem::take(&mut entry_domains[i]);
                        }
                        env
                    } else {
                        QueryEnv::new(
                            self.graph.program.templates[rule.template].sorts.len(),
                            witness_slots,
                            full_witnesses,
                            query_facts.clone(),
                        )
                    };
                    let live=if scheduled.is_some(){true}else if shared_seeds{self.stage_spine.bind_seed(&self.graph,rid,pivot,row,env.values.owned_mut())}
                        else{atom.args.iter().zip(row.values.iter()).all(|(a,v)|bind(a,v,&mut env,&self.graph))};
                    if live {
                        if self.local_known(rid,&env){self.local_sat.stats.seed_receipts+=1;continue;}
                        self.refutation_depth.begin(rid);
                        let ticket = if self.search_context_survey.enabled {self.search_context_survey.start(&self.graph, rid, pivot,
                            &env.values.materialize(), self.continuation_stats.merge_candidate_rows, found.len())} else {None};
                        (|| -> Result<()> {
                        let mut remaining = (0..rule.query.len())
                            .map(|i| i != pivot)
                            .collect::<Vec<_>>();
                        let mut epoch = row.epoch;
                        env.record(pivot, Some(row.witness));
                        if self.match_merge_program(rid,pivot,row,&mut env,&mut found)? {return Ok(()); }
                        if self.match_union_graft(rid, pivot, row, &mut env, &mut found)? {
                            return Ok(());
                        }
                        self.attach_recipe_context(rid, pivot, row, &mut env);
                        if self.reject_entry(rid, &rule.query, &remaining, &env) {
                            return Ok(());
                        }
                        match self.composed_match(rid, pivot, row, &mut env)? {
                            ComposedMatch::Matched(b) => {
                                found.push(b);
                                return Ok(());
                            }
                            ComposedMatch::Rejected => return Ok(()),
                            ComposedMatch::Fallback => {}
                        }
                        let delta = Some((pivot, row.epoch, row.witness));
                        if self.multistage_enabled && self.point_programs {
                            if let Some(order) = self
                                .graph
                                .program
                                .multi_stage
                                .entries
                                .get(&(rid, pivot))
                                .cloned()
                            {
                                if !self.points(
                                    rid,
                                    &rule.query,
                                    &order,
                                    &mut env,
                                    &mut remaining,
                                    &mut epoch,
                                    delta,
                                    false,
                                )? {
                                    return Ok(());
                                }
                            }
                        }
                        let mut wired = false;
                        let (live, mut epoch, origin) = self.continue_local(
                            rid,
                            pivot,
                            row,
                            &mut env,
                            &mut remaining,
                            epoch,
                            &mut wired,
                        )?;
                        if !live {
                            return Ok(());
                        }
                        // Prefer certified producer-local slots. Only unresolved
                        // reads use conditional singleton projections of the current
                        // public interface (including data-loader-origin nodes).
                        if self.multistage_enabled && self.point_programs {
                            if let Some(order) = self
                                .graph
                                .program
                                .multi_stage
                                .entries
                                .get(&(rid, pivot))
                                .cloned()
                            {
                                if !self.points(
                                    rid,
                                    &rule.query,
                                    &order,
                                    &mut env,
                                    &mut remaining,
                                    &mut epoch,
                                    delta,
                                    true,
                                )? {
                                    return Ok(());
                                }
                            }
                        }
                        let residual = remaining
                            .iter()
                            .enumerate()
                            .any(|(i, p)| *p && matches!(rule.query[i].call, Call::Table(_)));
                        if self.reject_entry(rid, &rule.query, &remaining, &env) {
                            return Ok(());
                        }
                        let before = found.len();
                        if self.multistage_enabled && self.point_programs && !residual {
                            if let Some(order) =
                                self.graph.program.multi_stage.finishes.get(rid).cloned()
                            {
                                if !self.points(
                                    rid,
                                    &rule.query,
                                    &order,
                                    &mut env,
                                    &mut remaining,
                                    &mut epoch,
                                    delta,
                                    false,
                                )? {
                                    return Ok(());
                                }
                            }
                        }
                        if self.multistage_enabled && !remaining.iter().any(|v| *v) {
                            found.push(Binding {
                                #[cfg(feature = "iterative-bake")]
                                direct: None,
                                composition: None,
                                witnesses: env.witnesses.clone(),
                                known: env.known.clone(),
                                values: if wired {
                                    rule.inputs.iter().map(|&i| {
                                        env.values.take_slot(i, &mut self.graph.stats.slot_transfer.sealed_moves)
                                            .map(|v| (i, v))
                                            .ok_or_else(|| "incomplete wired RHS input".to_owned())
                                    }).collect::<Result<_>>()?
                                } else {
                                    env.binding()
                                },
                                epoch,
                                parent: None,
                                continuation: None,
                                compiled: true,
                            });
                        } else {
                            self.solve(
                                &rule.query,
                                rid,
                                &mut remaining,
                                &mut env,
                                epoch,
                                &mut found,
                                delta,
                            )?;
                        }
                        if origin.is_some() {
                            for binding in &mut found[before..] {
                                binding.continuation = origin.map(|(p, d)| (p, d, residual));
                            }
                        }
                        Ok(())
                        })()?;
                        self.search_context_survey.finish(ticket, &self.graph,
                            self.continuation_stats.merge_candidate_rows, found.len());
                        self.refutation_depth.finish();
                    }
                }
            }
        } else {
            self.solve(
                &rule.query,
                rid,
                &mut vec![true; rule.query.len()],
                &mut QueryEnv::new(
                    self.graph.program.templates[rule.template].sorts.len(),
                    witness_slots,
                    full_witnesses,
                    query_facts.clone(),
                ),
                0,
                &mut found,
                None,
            )?;
        }
        let key_slots = if self.fd_keys {
            self.graph.program.physical.dedup[rid].as_slice()
        } else {
            rule.inputs.as_slice()
        };
        found.retain(|b| probe || !rule.seminaive || b.epoch >= self.watermarks[rid]);
        let n = found.len() as u64;
        self.dedup_rows += n;
        self.dedup_full_elements += n * rule.inputs.len() as u64;
        self.dedup_elements += n * key_slots.len() as u64;
        if found
            .iter()
            .any(|b| key_slots.iter().any(|s| !b.values.contains_key(s)))
        {
            return Err("incomplete binding".into());
        }
        let compare = |a: &Binding, b: &Binding| {
            key_slots
                .iter()
                .map(|s| &a.values[s])
                .cmp(key_slots.iter().map(|s| &b.values[s]))
        };
        found.sort_by(compare);
        found.dedup_by(|later, first| {
            if compare(first, later).is_ne() {
                return false;
            }
            if first.composition.is_none() && later.composition.is_some() {
                std::mem::swap(first, later);
            }
            true
        });
        self.dedup_duplicates += n - found.len() as u64;
        Ok(found)

    }
    fn apply(&mut self, rid: usize, b: Binding) -> Result<Option<Produced>> {
        #[cfg(feature = "slot-transfer-audit")]
        let _rhs = crate::transfer_audit::RhsScope::enter();
        let stage_cursor=self.stage_spine.begin_step(&self.graph.program,rid,&b.values,&self.current_stage,self.graph.epoch, #[cfg(feature = "iterative-bake")] b.direct);
        let stage_counts=stage_cursor.as_ref().map(|_|(self.graph.stats.constructor_requests,self.graph.stats.function_updates,self.graph.stats.union_requests));
        let template = self.graph.program.rules[rid].template;
        let origin = b.continuation;
        if b.compiled {
            self.continuation_stats.compiled_bindings += 1;
        }
        let facts = self.graph.stats.logical_facts_created;
        let updates = self.graph.stats.function_updates;
        let eliminate_static = self.graph.known_mode == "static";
        let observe_static = self.graph.known_mode == "observe";
        if !self.graph.views_enabled
            && !self.compositions_enabled
            && self.local_run.is_none()
            && origin.is_none()
            && !self.graph.union_lineage_enabled
            && (eliminate_static || self.graph.known_mode == "off")
            && let Some(write) = self.graph.program.writes[rid].clone()
        {
            let (cell, index, key, changed) = self
                .graph
                .apply_write(template, b.values, b.parent, &write)?;
            if let Some(step) = stage_cursor {
                self.stage_spine.effects(0, u64::from(changed), 0);
                self.stage_spine.capture_write(
                    step,
                    rid,
                    cell,
                    write.op,
                    write.table,
                    key,
                    Dependency::Function(index),
                    self.graph.stats.unions,
                    #[cfg(feature = "iterative-bake")] &self.graph.program.iterative,
                    #[cfg(feature = "iterative-bake")] &b.known,
                );
            }
            return Ok(None);
        }
        #[cfg(feature = "iterative-bake")]
        let available = stage_cursor
            .as_ref()
            .filter(|_| {
                eliminate_static
                    && !self.graph.views_enabled
                    && !self.compositions_enabled
                    && self.local_run.is_none()
                    && !self.graph.union_lineage_enabled
            })
            .map(|s| s.result_inputs(&self.graph.program.iterative))
            .unwrap_or_default();
        #[cfg(feature = "iterative-bake")]
        if !available.is_empty()
            && stage_cursor
                .as_ref()
                .is_some_and(|s| s.closed(&self.graph.program.iterative))
        {
            if let Some((cell, mut keys, receipts, unions)) =
                self.graph.finish_handoff(template, &b.values, &available)?
            {
                self.stage_spine.effects(0, 0, unions);
                self.stage_spine.capture_keys(
                    &self.graph,
                    rid,
                    cell,
                    stage_cursor.unwrap(),
                    |i| b.known.get(i).map(Dependency::Fact),
                    Some(&receipts),
                    |i, _| {
                        keys[i]
                            .take()
                            .ok_or("missing completed publication key".into())
                    },
                )?;
                return Ok(None);
            }
            self.graph.stats.handoff_closed_fallback += 1;
        }
        let static_skip = if eliminate_static || observe_static {
            self.graph.program.known_results.static_skip.get(rid).cloned()
        } else {
            None
        };
        let aliases = static_skip.as_deref().map(Vec::as_slice).filter(|_| eliminate_static);
        let (cell, first_op) = if self.graph.program.rules[rid].constructor_only
            && !self.graph.views_enabled && (self.graph.known_mode == "off" || eliminate_static)
            && !self.compositions_enabled && stage_cursor.is_none()
            && self.local_run.is_none() && origin.is_none() && !self.graph.union_lineage_enabled
        {
            let Some(start) = self.graph.start_constructor_rhs(template, b.values, b.parent, aliases)? else {
                return Ok(None);
            };
            start
        } else {
            (self.graph.reference_slots(template, b.values, b.parent, &b.witnesses)?, 0)
        };
        if matches!(self.graph.known_mode.as_str(),"observe"|"on") {
            if let Some(plans)=self.graph.program.known_results.rules.get(&rid){
                for (op,source) in plans.iter().enumerate(){
                    if let Some(crate::known_results::Source::Query(i))=source && let Some(f)=b.known.get(*i){
                        self.graph.cells.get_mut(&cell).unwrap().known_results.insert(op,f);
                    }
                }
            }
        }
        if self.compositions_enabled {
            let started = Instant::now();
            if let Some((edge, context)) = &b.composition {
                let old = context.composition.filter(|c| c.program == edge.program);
                let cursor = crate::compositions::Cursor {
                    program: edge.program,
                    entry: old.map_or(context.cell, |c| c.entry),
                    steps: old
                        .map_or(1, |c| c.steps)
                        .checked_add(1)
                        .ok_or("composition step overflow")?,
                    marker_entry: old.is_some_and(|c| c.marker_entry),
                };
                self.graph.cells.get_mut(&cell).unwrap().composition = Some(cursor);
                self.composition_stats.continued_steps += 1;
                self.composition_stats.marker_continued_steps += u64::from(cursor.marker_entry);
                self.composition_stats.max_steps =
                    self.composition_stats.max_steps.max(cursor.steps);
                self.composition_stats.source_rules.insert(rid);
            } else if let Some(&program) = self.graph.program.compositions.rule_program.get(&rid) {
                self.graph.cells.get_mut(&cell).unwrap().composition =
                    Some(crate::compositions::Cursor {
                        program,
                        entry: cell,
                        steps: 1,
                        marker_entry: self
                            .graph
                            .program
                            .compositions
                            .marker_entries
                            .contains(&rid),
                    });
                self.composition_stats.root_steps += 1;
            }
            self.composition_stats.entry_seconds += started.elapsed().as_secs_f64();
        }
        if let Some((producer, depth, residual)) = origin {
            self.graph.cells.get_mut(&cell).unwrap().recipe_depth = depth;
            self.handoffs.insert((producer, rid));
            if residual {
                self.continuation_stats.residual_bindings += 1;
            } else {
                self.continuation_stats.complete_bindings += 1;
            }
            self.continuation_stats.max_depth = self.continuation_stats.max_depth.max(depth);
        }
        let capture_layout=self.local_run.as_ref().and_then(|(_,p)|p.captures.get(&rid)).cloned();
        let mut visible_ops=BTreeSet::new();
        let ops = self.graph.program.templates[template].ops.clone();
        let mut effect_receipts=self.stage_spine.capture_receipts(rid).then(||vec![None;ops.len()]);
        let known_values=if self.graph.known_mode=="value" {self.graph.program.known_results.values.get(&rid).cloned()}else{None};
        let mut completed = true;
        let mut completed_prefix=first_op;
        #[cfg(feature = "iterative-bake")]
        let mut transfers=available.as_slice();
        for (i, op) in ops.iter().enumerate().skip(first_op) {
            if self.graph.known_stats.elide_static(aliases, i) {
                completed_prefix=i+1;continue;
            }
            if self.graph.union_lineage_enabled {
                self.graph.active_union_cause = Some(crate::union_graft::Cause::Apply {
                    rule: rid,
                    frame: cell,
                    op: i,
                });
            }
            #[cfg(feature = "iterative-bake")]
            if let Some((input, tail)) = transfers.split_first().filter(|(r, _)| r.op == i) {
                transfers = tail;
                self.graph.stats.handoff_attempts += 1;
                if let Some(f) = input.fact {
                    let (t, args) = match op {
                        Op::Let(_, Call::Table(t), a) | Op::Set(t, a, _) => (*t, a),
                        _ => unreachable!("validated constructor handoff"),
                    };
                    if let Some(row) = self.graph.handoff_row(f, cell, t, args)? {
                        let value = row.values.last().unwrap().clone();
                        match op {
                            Op::Let(dst, ..) => {
                                self.graph
                                    .cells
                                    .get_mut(&cell)
                                    .unwrap()
                                    .values
                                    .insert(*dst, value);
                                self.graph.stats.handoff_lets += 1;
                            }
                            Op::Set(_, _, v) => {
                                let rhs = self.graph.argument(cell, v)?;
                                if input.equal_output && value == rhs {
                                    self.graph.stats.handoff_noop_unions += 1;
                                } else {
                                    self.graph.enqueue_union(value, rhs)?;
                                }
                                self.graph.stats.handoff_sets += 1;
                            }
                            _ => unreachable!(),
                        }
                        if let Some(e) = &mut effect_receipts {
                            e[i] = Some(row.witness);
                        }
                        self.graph.stats.handoff_deep += u64::from(input.back > 1);
                        completed_prefix = i + 1;
                        continue;
                    }
                    self.graph.stats.handoff_stale += 1;
                } else {
                    self.graph.stats.handoff_missing += 1;
                }
            }
            match op {
                Op::Copy(..) => {}
                Op::Let(dst, Call::Table(t), aa) => {
                    if self.graph.program.tables[*t].virtual_fact {
                        if let Some(arg)=known_values.as_ref().and_then(|v|v.get(i)).and_then(Option::as_ref) {
                            if self.graph.reuse_result_slot(cell,*t,*dst,arg)? {
                                completed_prefix=i+1;
                                continue;
                            }
                        }
                        if let Some((edge, context)) = &b.composition
                            && let Some(columns) = edge.reuse.get(&i)
                        {
                            if let Ok(values) = self.graph.context_arguments(*context, columns) {
                                let key = self.graph.arguments(cell, aa)?;
                                if values[..values.len() - 1] == key {
                                    self.graph
                                        .cells
                                        .get_mut(&cell)
                                        .unwrap()
                                        .values
                                        .insert(*dst, values.last().unwrap().clone());
                                    self.composition_stats.constructor_reuses += 1;
                                    completed_prefix=i+1;
                                    continue;
                                }
                            }
                            self.composition_stats.reuse_fallbacks += 1;
                        }
                        let actual=if let Some(receipts)=&mut effect_receipts {
                            let mut visible=false;
                            let value=self.graph.construct_recorded(cell,i,*t,aa,*dst,&mut receipts[i],&mut visible)?;
                            if visible && capture_layout.as_ref().is_some_and(|c|c.effects.contains(&i)) { visible_ops.insert(i); }
                            value
                        }else if capture_layout.as_ref().is_some_and(|c|c.effects.contains(&i)) {
                            let mut visible=false;let v=self.graph.construct_visible(cell,i,*t,aa,*dst,&mut visible)?;
                            if visible{visible_ops.insert(i);}v
                        }else{self.graph.construct(cell,i,*t,aa,*dst)?};
                        if observe_static && static_skip.as_ref().is_some_and(|s|s[i]){
                            let a=self.graph.program.known_results.static_slots[template][*dst].clone().unwrap();
                            if self.graph.argument(cell,&a)?==actual{self.graph.known_stats.static_observed_equal+=1;}else{self.graph.known_stats.static_observed_other+=1;}
                        }
                    } else {
                        let key = self.graph.arguments(cell, &aa)?;
                        let row = self.graph.lookup_canonical(*t, &key)?.ok_or_else(|| {
                            format!(
                                "missing function read {}",
                                self.graph.program.tables[*t].name
                            )
                        })?;
                        if let Some(receipts)=&mut effect_receipts { receipts[i]=Some(row.witness); }
                        self.graph
                            .cells
                            .get_mut(&cell)
                            .unwrap()
                            .values
                            .insert(*dst, row.values.last().unwrap().clone());
                    }
                }
                Op::Let(dst, Call::Primitive(name, output), aa) => {
                    let args = self.graph.arguments(cell, &aa)?;
                    if name.starts_with("cell-key-") {
                        self.graph.stats.key_operations += 1;
                    }
                    let v = value::primitive(
                        &name,
                        &args,
                        &output,
                        &self.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                    )?
                    .ok_or_else(|| format!("RHS primitive failed: {name}"))?;
                    self.graph
                        .cells
                        .get_mut(&cell)
                        .unwrap()
                        .values
                        .insert(*dst, v);
                }
                Op::Set(t, aa, v) => {
                    if let Some(receipts)=&mut effect_receipts {
                        let mut visible=false;self.graph.set_recorded(cell,i,*t,aa,v,&mut receipts[i],&mut visible)?;
                        if visible && capture_layout.as_ref().is_some_and(|c|c.effects.contains(&i)) { visible_ops.insert(i); }
                    }else if capture_layout.as_ref().is_some_and(|c|c.effects.contains(&i)){let mut visible=false;self.graph.set_visible(cell,i,*t,aa,v,&mut visible)?;if visible{visible_ops.insert(i);}}
                    else{self.graph.set(cell,i,*t,aa,v)?;}
                },
                Op::Union(a, b) => {
                    let a = self.graph.argument(cell, &a)?;
                    let b = self.graph.argument(cell, &b)?;
                    self.graph.enqueue_union(a, b)?;
                }
                Op::Hide(t, aa, delete) => {
                    let key = self.graph.arguments(cell, &aa)?;
                    if !self.graph.hide(*t, &key, *delete)? {
                        completed_prefix=i+1;
                        completed = false;
                        break;
                    }
                }
                Op::Panic => return Err("source panic".into()),
            }
            completed_prefix=i+1;
        }
        self.graph.active_union_cause = None;
        if origin.is_some() {
            self.continuation_stats.published_facts +=
                self.graph.stats.logical_facts_created - facts;
            self.continuation_stats.function_updates += self.graph.stats.function_updates - updates;
        }
        let state=self.graph.cells.get_mut(&cell).unwrap();state.completed=completed;state.completed_prefix=completed_prefix;
        self.graph.queue_absorption(cell, &b.witnesses);
        let mut produced=None;
        if completed && let Some((_,plan))=&self.local_run && let Some(routes)=plan.outgoing.get(&rid) {
            let ready=routes.iter().enumerate().filter_map(|(i,t)|t.rows.iter().all(|(_,_,_,op)|op.is_none_or(|op|visible_ops.contains(&op))).then_some(i)).collect::<Vec<_>>();
            self.local_sat.stats.incomplete_contexts+=(routes.len()-ready.len()) as u64;
            if !ready.is_empty(){
                let values=capture_layout.as_ref().unwrap().slots.iter().map(|i|self.graph.argument(cell,&Arg::Slot(*i))).collect::<Result<Vec<_>>>()?;
                self.local_sat.stats.captures+=1;produced=Some(Produced{rule:rid,depth:self.graph.cells[&cell].recipe_depth,values,routes:ready});
            }
        }
        if let Some((c,f,u))=stage_counts {self.stage_spine.effects(self.graph.stats.constructor_requests-c,self.graph.stats.function_updates-f,self.graph.stats.union_requests-u);}
        if completed && let Some(cursor) = stage_cursor {
            self.stage_spine.capture_step(
                &self.graph, rid, cell, cursor,
                |i| b.known.get(i).map(Dependency::Fact), effect_receipts.as_deref(),
            )?;
        }
        self.graph.release_empty(cell);
        Ok(produced)
    }
    fn local_known(&self,rid:usize,env:&QueryEnv)->bool {
        let Some((session,plan))=&self.local_run else{return false};
        if !plan.targets.contains(&rid) || self.local_sat.sessions[*session].seen.is_empty(){return false}
        let key=plan.completion_keys[&rid].iter().map(|i|env.values[*i].clone()).collect::<Option<Vec<_>>>();
        key.is_some_and(|key|self.local_sat.sessions[*session].seen.contains(&(rid,key)))
    }
    fn apply_once(&mut self,rid:usize,b:Binding)->Result<(bool,Option<Produced>)> {
        let key=self.local_run.as_ref().filter(|(_,p)|p.targets.contains(&rid)).map(|(session,plan)|{
            (*session,plan.completion_keys[&rid].iter().map(|i|b.values[i].clone()).collect::<Vec<_>>())
        });
        if let Some((session,key))=&key && self.local_sat.sessions[*session].seen.contains(&(rid,key.clone())) {
            self.local_sat.stats.duplicate_bindings+=1;return Ok((false,None))
        }
        let source=self.apply(rid,b)?;
        // Completion means the exact pure RHS has been executed/enqueued. It
        // becomes externally committed only at the normal block repair below.
        if let Some((session,key))=key {
            self.local_sat.sessions[session].seen.insert((rid,key));
            self.local_sat.stats.peak_seen=self.local_sat.stats.peak_seen.max(self.local_sat.sessions[session].seen.len());
        }
        Ok((true,source))
    }
    fn local_successor(&mut self,source:&Produced,transfer:&crate::local_sat::Transfer)->Result<Option<Binding>> {
        let rule=self.graph.program.rules[transfer.consumer].clone();
        let mut env=QueryEnv::new(self.graph.program.templates[rule.template].sorts.len(),0,false,QueryFacts::None);
        for (slot,source_arg) in &transfer.assignments {
            let v=match source_arg{Arg::Literal(v)=>canon(v,&self.graph.parents),Arg::Slot(i)=>source.values[*i].clone()};
            env.values[*slot]=Some(v);
        }
        for &i in &*self.graph.program.multi_stage.finishes[transfer.consumer] {
            let atom=&rule.query[i];let Call::Primitive(name,output)=&atom.call else{return Err("local finish includes a table search".into())};
            let args=atom.args[..atom.args.len()-1].iter().map(|a|operand(a,&env,&self.graph).ok_or_else(||"unbound local guard".to_string())).collect::<Result<Vec<_>>>()?;
            let Some(value)=value::primitive(name,&args,output,&self.graph.program.artifact["source_core"]["structural_key"]["nodes"])? else {return Ok(None)};
            if !bind(atom.args.last().unwrap(),&value,&mut env,&self.graph){return Ok(None)}
        }
        let values=env.binding();
        if rule.inputs.iter().any(|i|!values.contains_key(i)){return Err("incomplete local binding".into())}
        if self.local_sat.check {
            for atom in &*rule.query {if let Call::Table(t)=atom.call {
                let expected=atom.args.iter().map(|a|operand(a,&env,&self.graph).ok_or_else(||"unready local audit operand".to_string())).collect::<Result<Vec<_>>>()?;
                let row=self.graph.lookup(t,&expected[..expected.len()-1])?;
                if row.is_none_or(|r|r.hidden || *r.values!=expected){return Err(format!("local LHS witness missing: rule {} table {}",transfer.consumer,self.graph.program.tables[t].name));}
            }}
            self.local_sat.stats.audit_bindings+=1;
        }
        Ok(Some(Binding {
            #[cfg(feature = "iterative-bake")]
            direct: None,
            values,
            witnesses: MatchedFacts::None,
            known: QueryFacts::None,
            composition: None,
            parent: None,
            continuation: None,
            compiled: true,
            epoch: self.graph.epoch,
        }))
    }
    fn probe(&mut self, set: &str) -> Result<bool> {
        let rules = self
            .graph
            .program
            .sets
            .get(set)
            .ok_or("unknown probe ruleset")?
            .clone();
        for r in rules {
            if !self.matches(r, true)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn run(&mut self, set: &str, path: &[usize], iterations: &[u64]) -> Result<bool> {
        self.current_stage=set.into();
        #[cfg(feature = "iterative-bake")]
        {
            self.stage_spine.training.run = self.steps.len() + 1;
        }
        self.graph.receipt_forwards.clear();
        if let Some(survey)=self.graph.union_impact.as_mut() {survey.run_start_cell=self.graph.next_cell;}
        self.graph.epoch = self.graph.epoch.checked_add(1).ok_or("epoch exhausted")?;
        self.graph.changed = false;
        let start = Instant::now();
        if self.progress {
            eprintln!("[cell-cpu] Run {}: {set}", self.steps.len() + 1);
        }
        let rules = self
            .graph
            .program
            .sets
            .get(set)
            .ok_or_else(|| format!("unknown ruleset {set}"))?
            .clone();
        self.local_run=self.local_sat.active(set);
        if self.local_run.is_some(){self.local_sat.stats.source_runs+=1;}
        let mut work = vec![];
        // Every LHS is read before any effect is made visible. Prepared successor
        // recipes therefore cannot leak into this finite Run's matching snapshot.
        for rid in rules {
            let clock = self.profile_queries.then(Instant::now);
            let candidates = self.graph.stats.residual_candidates;
            if self.progress && self.profile_queries {
                eprintln!("[cell-cpu] query {rid}");
            }
            let bindings = self.matches(rid, false)?;
            if let Some(clock) = clock {
                self.query_profile[rid].0 += 1;
                self.query_profile[rid].1 += clock.elapsed().as_secs_f64();
                self.query_profile[rid].2 += self.graph.stats.residual_candidates - candidates;
            }
            self.watermarks[rid] = self.graph.epoch;
            work.extend(bindings.into_iter().map(|b| (rid, b)));
        }
        if std::env::var("CELL_CPU_QUERY_INDEX_SCOPE").as_deref() == Ok("run") {
            self.query_indexes.clear();
            self.sieve.invalidate();
        }
        if let Some((session,plan))=&self.local_run && self.local_sat.sessions[*session].seen.is_empty()
            && !work.iter().any(|(r,_)|plan.outgoing.contains_key(r)) {
            self.local_sat.stats.idle_runs+=1;self.local_run=None;
        }
        let patch = StagePatch { bindings: work };
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::record_run(patch.bindings.iter().map(|(rid, b)| (rid, &b.values)))?;
        let matches = patch.bindings.len();
        let search_seconds = start.elapsed().as_secs_f64();
        if self.audit {
            self.binding_audit.push(json!({"ruleset":set,"bindings":patch.bindings.iter().map(|(rid,b)|{let template=&self.graph.program.templates[self.graph.program.rules[*rid].template];json!({"rule":rid,"values":b.values.iter().map(|(i,v)|json!({"slot":i,"value":crate::value::exported(v,&template.sorts[*i])})).collect::<Vec<_>>()})}).collect::<Vec<_>>()}));
        }
        let phase = Instant::now();
        let local_before=self.local_sat.stats.local_bindings;
        let mut queue=VecDeque::new();let mut executed=0usize;
        for (rid, b) in patch.bindings {
            let (applied,source)=self.apply_once(rid,b)?;
            if let Some(source)=source {queue.push_back((source,vec![(rid,0usize)]));}
            executed+=usize::from(applied);
        }
        let ordinary_rhs_seconds=phase.elapsed().as_secs_f64();
        let local_clock=self.local_run.is_some().then(Instant::now);
        // Original snapshot RHS order finishes before any local successor.
        while let Some((source,lineage))=queue.pop_front() {
            self.local_sat.stats.max_pending=self.local_sat.stats.max_pending.max(queue.len()+1);
            let plan=self.local_run.as_ref().unwrap().1.clone();
            for &route in &source.routes {
                let transfer=&plan.outgoing[&source.rule][route];
                self.local_sat.stats.transfer_attempts+=1;
                let Some(mut binding)=self.local_successor(&source,transfer)? else {self.local_sat.stats.guard_rejections+=1;continue};
                let rid=transfer.consumer;
                // Structural recursion boundary, not a depth/work budget.
                let generation=lineage.last().unwrap().1+usize::from(!transfer.nonexpanding);
                let prior=lineage.iter().rev().find(|(r,_)|*r==rid);
                if prior.is_some_and(|(_,g)|*g!=generation){
                    let session=self.local_run.as_ref().unwrap().0;
                    let key=plan.completion_keys[&rid].iter().map(|i|binding.values[i].clone()).collect::<Vec<_>>();
                    if self.local_sat.sessions[session].seen.contains(&(rid,key)){self.local_sat.stats.duplicate_bindings+=1;}
                    else{self.local_sat.stats.cycle_deferrals+=1;}
                    continue
                }
                binding.continuation=Some((source.rule,source.depth.checked_add(1).ok_or("local recipe depth overflow")?,false));
                let (applied,next)=self.apply_once(rid,binding)?;
                if !applied{continue}
                if prior.is_some(){self.local_sat.stats.finite_reentries+=1;}
                let mut chain=lineage.clone();chain.push((rid,generation));
                self.local_sat.stats.max_chain=self.local_sat.stats.max_chain.max(chain.len());
                self.local_sat.stats.local_bindings+=1;*self.local_sat.stats.by_rule.entry(rid).or_default()+=1;
                *self.local_sat.stats.dispatch_sets.entry(set.into()).or_default()+=1;
                let owner=self.graph.program.artifact["recipe"]["rules"][rid]["metadata"]["source_set"].as_str().unwrap_or(set);
                *self.local_sat.stats.by_set.entry(owner.into()).or_default()+=1;
                if let Some(next)=next {queue.push_back((next,chain));}
                executed+=1;
            }
        }
        self.local_run=None;
        let local_seconds=local_clock.map_or(0.0,|clock|clock.elapsed().as_secs_f64());
        let rhs_seconds=ordinary_rhs_seconds;
        self.graph.stats.bindings += executed as u64;
        let phase = Instant::now();
        let old_unions=self.graph.stats.unions;
        self.graph.repair()?;
        if self.graph.stats.unions!=old_unions {self.local_sat.repair_receipts(&self.graph.parents);}
        if let Some(mut survey)=self.graph.union_impact.take() {
            survey.finish(&self.graph,set)?;
            self.graph.union_impact=Some(survey);
        }
        let moved = self.graph.absorb_ready()?;
        if !moved.is_empty() {
            // Only readers of the relocated fact's table can have that handle
            // pending. Keep all unrelated rules and pending sets untouched.
            for &old in moved.keys() {
                let new = self.graph.resolve_reference(old);
                let table = self
                    .graph
                    .dependency_table(Dependency::Fact(new))
                    .ok_or("relocated fact has no table")?;
                for &rid in &self.readers[table] {
                    if self.pending[rid].remove(&Dependency::Fact(old)) {
                        self.pending[rid].insert(Dependency::Fact(new));
                    }
                }
            }
            self.graph.events = self
                .graph
                .events
                .iter()
                .map(|d| match *d {
                    Dependency::Fact(f) => Dependency::Fact(self.graph.resolve_reference(f)),
                    _ => *d,
                })
                .collect();
        }
        let repair_seconds = phase.elapsed().as_secs_f64();
        let phase = Instant::now();
        self.stage_spine.publish(&mut self.graph)?;
        self.entry_schedule.publish(&self.graph);
        self.graph.receipt_forwards.clear();
        let union_roots = std::mem::take(&mut self.graph.union_roots);
        self.union_grafts.publish(&self.graph, union_roots)?;
        let context_changed_tables=if self.search_contexts.enabled || (self.refutation.enabled && self.refutation.scoped) {
            self.graph.query_changes.iter().map(|(_,t)|*t).collect::<BTreeSet<_>>()
        } else {BTreeSet::new()};
        if let Some(index) = &mut self.range_index {
            index.update(&self.graph)?;
        }
        if std::env::var("CELL_CPU_QUERY_INDEX_SCOPE").as_deref() == Ok("table") {
            self.query_indexes.invalidate(&self.graph);
            self.sieve.invalidate();
            self.graph.query_changes.clear();
        } else {
            self.query_indexes.update(&mut self.graph)?;
            self.sieve.publish(&mut self.query_indexes);
        }
        if self.refutation.enabled {
            self.refutation.publish(&self.query_indexes, &self.graph,&context_changed_tables);
        }
        self.search_contexts.publish(&self.graph,&self.query_indexes,&context_changed_tables);
        self.retained.publish(&mut self.graph,&mut self.query_indexes)?;
        let query_update_seconds = phase.elapsed().as_secs_f64();
        let phase = Instant::now();
        self.deliver();
        self.graph.collect_garbage();
        let delivery_seconds = phase.elapsed().as_secs_f64();
        let changed = self.graph.changed;
        self.steps.push(json!({"ordinal":self.steps.len()+1,"ruleset":set,"path":path,"iterations":iterations,"epoch":self.graph.epoch,"logical_matches":matches,"local_bindings":self.local_sat.stats.local_bindings-local_before,"executed_bindings":executed,"changed":changed,"live_cells":self.graph.cells.len(),"elapsed_seconds":start.elapsed().as_secs_f64(),"phases":{"search":search_seconds,"rhs":rhs_seconds,"local_sat":local_seconds,"repair":repair_seconds,"delivery_gc":delivery_seconds,"query_update":query_update_seconds}}));
        if self.audit {
            self.snapshots.push(self.graph.export()?);
        }
        if self.local_sat.check && !self.tasks.iter().any(|t|matches!(t,Task::Saturate{..})) {
            self.semantic_audit.push(json!({"kind":"run","set":set,"path":path,"iterations":iterations,"state":self.graph.export()?}));
        }
        Ok(changed)
    }
    pub fn execute(&mut self) -> Result<()> {
        self.execute_until(None)
    }
    pub fn execute_until(&mut self, stop: Option<usize>) -> Result<()> {
        while !self.tasks.is_empty() {
            if stop.is_some_and(|n| self.steps.len() >= n) {
                break;
            }
            match self.tasks.pop().unwrap() {
                Task::Eval {
                    node,
                    path,
                    iterations,
                } => {
                    if let Some(set) = node["run"].as_str() {
                        if let Some(probe) = node["until_run"].as_str() {
                            if self.probe(probe)? {
                                continue;
                            }
                        }
                        if node["assert"].as_bool() == Some(true) {
                            if !self.probe(set)? {
                                return Err(format!("source check failed: {set}"));
                            }
                            continue;
                        }
                        if !node["until"].is_null() || !node["assert_table"].is_null() {
                            return Err("unlowered source probe".into());
                        }
                        self.run(set, &path, &iterations)?;
                    } else if let Some(seq) = node["sequence"].as_array() {
                        for (i, child) in seq.iter().enumerate().rev() {
                            let mut p = path.clone();
                            p.push(i);
                            self.tasks.push(Task::Eval {
                                node: child.clone(),
                                path: p,
                                iterations: iterations.clone(),
                            });
                        }
                    } else if let Some(count) = node["repeat"].as_u64() {
                        if count > 0 {
                            self.tasks.push(Task::Repeat {
                                body: node["body"].clone(),
                                count,
                                next: 0,
                                start: self.steps.len(),
                                path,
                                iterations,
                            });
                        }
                    } else if !node["saturate"].is_null() {
                        let body = node["saturate"].clone();
                        self.local_sat.enter(&self.graph.program,&body,&path,&iterations);
                        let mut it = iterations.clone();
                        it.push(0);
                        self.tasks.push(Task::Saturate {
                            body: body.clone(),
                            iteration: 0,
                            start: self.steps.len(),
                            path: path.clone(),
                            iterations,
                        });
                        self.tasks.push(Task::Eval {
                            node: body,
                            path,
                            iterations: it,
                        });
                    } else {
                        return Err("unknown schedule instruction".into());
                    }
                }
                Task::Repeat {
                    body,
                    count,
                    next,
                    start,
                    path,
                    iterations,
                } => {
                    if next > 0 && !self.steps[start..].iter().any(|s| s["changed"] == true) {
                        continue;
                    }
                    if next < count {
                        let mut it = iterations.clone();
                        it.push(next);
                        self.tasks.push(Task::Repeat {
                            body: body.clone(),
                            count,
                            next: next + 1,
                            start: self.steps.len(),
                            path: path.clone(),
                            iterations,
                        });
                        self.tasks.push(Task::Eval {
                            node: body,
                            path,
                            iterations: it,
                        });
                    }
                }
                Task::Saturate {
                    body,
                    iteration,
                    start,
                    path,
                    iterations,
                } => {
                    if self.steps[start..].iter().any(|s| s["changed"] == true) {
                        let iteration = iteration
                            .checked_add(1)
                            .ok_or("saturation occurrence exhausted")?;
                        let mut it = iterations.clone();
                        it.push(iteration);
                        self.tasks.push(Task::Saturate {
                            body: body.clone(),
                            iteration,
                            start: self.steps.len(),
                            path: path.clone(),
                            iterations,
                        });
                        self.tasks.push(Task::Eval {
                            node: body,
                            path,
                            iterations: it,
                        });
                    } else {
                        self.local_sat.exit(&path,&iterations);
                        if self.local_sat.check && !self.tasks.iter().any(|t|matches!(t,Task::Saturate{..})) {
                            self.semantic_audit.push(json!({"kind":"sat_exit","path":path,"iterations":iterations,"state":self.graph.export()?}));
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn checkpoint(&self) -> J {
        let mut c = json!({"schema":"eggfuse-cell-checkpoint/v1","program":self.graph.program.artifact,"graph":self.graph.image(),"tasks":self.tasks,"watermarks":self.watermarks,"steps":self.steps,"continuation_stats":self.continuation_stats,"handoffs":self.handoffs});
        c["local_saturation"]=self.local_sat.image();
        c["compositions_enabled"] = self.compositions_enabled.into();
        c["composition_stats"] = json!(self.composition_stats);
        c["sha256"] = crate::checksum::digest(&c).into();
        c
    }
    pub fn resume(program: Program, mut checkpoint: J, audit: bool) -> Result<Self> {
        let checksum = checkpoint
            .as_object_mut()
            .ok_or("checkpoint object")?
            .remove("sha256")
            .ok_or("checkpoint checksum missing")?;
        if checksum != crate::checksum::digest(&checkpoint) {
            return Err("checkpoint checksum differs".into());
        }
        if checkpoint["schema"] != "eggfuse-cell-checkpoint/v1"
            || checkpoint["program"] != program.artifact
        {
            return Err("checkpoint program differs".into());
        }
        let graph = Graph::restore(program, &checkpoint["graph"])?;
        let mut e = Self::new(graph, audit);
        if checkpoint["compositions_enabled"]
            .as_bool()
            .unwrap_or(false)
            != e.compositions_enabled
        {
            return Err("checkpoint composition mode differs".into());
        }
        if !checkpoint["composition_stats"].is_null() {
            e.composition_stats = serde_json::from_value(checkpoint["composition_stats"].clone())
                .map_err(|x| x.to_string())?;
        }
        e.tasks = serde_json::from_value(checkpoint["tasks"].clone()).map_err(|x| x.to_string())?;
        if let Some(local)=checkpoint.get("local_saturation") {
            if local["enabled"].as_bool()!=Some(e.local_sat.enabled){return Err("checkpoint local-saturation mode differs".into())}
            e.local_sat.sessions=serde_json::from_value(local["sessions"].clone()).map_err(|x|x.to_string())?;
            e.local_sat.stats=serde_json::from_value(local["stats"].clone()).map_err(|x|x.to_string())?;
            for session in &e.local_sat.sessions {
                let body=e.tasks.iter().find_map(|t|match t{Task::Saturate{body,path,iterations,..} if *path==session.key.path && *iterations==session.key.iterations=>Some(body),_=>None}).ok_or("checkpoint local scope missing")?;
                if crate::local_sat::scope_sets(&e.graph.program,body).as_ref()!=Some(&session.sets){return Err("checkpoint local permissions differ".into())}
                let plan=e.local_sat.plans.get(&session.sets).ok_or("checkpoint local plan missing")?;
                for (r,key) in &session.seen {if !plan.targets.contains(r) || plan.completion_keys[r].len()!=key.len(){return Err("checkpoint completion key invalid".into())}}
            }
        }

        e.watermarks =
            serde_json::from_value(checkpoint["watermarks"].clone()).map_err(|x| x.to_string())?;
        e.steps = serde_json::from_value(checkpoint["steps"].clone()).map_err(|x| x.to_string())?;
        if !checkpoint["continuation_stats"].is_null() {
            e.continuation_stats = serde_json::from_value(checkpoint["continuation_stats"].clone())
                .map_err(|x| x.to_string())?;
        }
        if !checkpoint["handoffs"].is_null() {
            e.handoffs = serde_json::from_value(checkpoint["handoffs"].clone())
                .map_err(|x| x.to_string())?;
        }
        if e.watermarks.len() != e.graph.program.rules.len()
            || e.watermarks.iter().any(|v| *v > e.graph.epoch)
            || e.steps.len() as u64 != e.graph.epoch
        {
            return Err("checkpoint stage metadata invalid".into());
        }
        for task in &e.tasks {
            let start = match task {
                Task::Repeat { start, .. } | Task::Saturate { start, .. } => *start,
                _ => 0,
            };
            if start > e.steps.len() {
                return Err("checkpoint continuation invalid".into());
            }
        }
        Ok(e)
    }
    pub fn take_semantic_audit(&mut self)->Vec<J>{std::mem::take(&mut self.semantic_audit)}
    pub fn report(&self) -> J {
        json!({"physical":{"range":self.range_index.as_ref().map(|r| &r.stats),"dedup_rows":self.dedup_rows,"dedup_full_elements":self.dedup_full_elements,"dedup_elements":self.dedup_elements,"dedup_duplicates":self.dedup_duplicates},"free_join":self.free_join.stats,"entry_schedule":self.entry_schedule.report(),"stage_spine":self.stage_spine.report(),"retained_contexts":self.retained.report(),"local_saturation":self.local_sat.report(),"semantic_audit":self.semantic_audit,"refutation_depth":self.refutation_depth.report(),"search_contexts":self.search_contexts.report(),"presearch_survey":self.search_context_survey.report(&self.graph),"features":{"merge_programs":self.graph.merge_programs_enabled},"known_results":self.graph.known_report(),"union_impact":self.graph.union_impact.as_ref().map(|s|s.report()),"union_graft":self.union_grafts.report(),"union_lineage":{"enabled":self.graph.union_lineage_enabled,"events":self.graph.union_lineage.events,"scope":"effective unions; selected cause-DAG height, not rebuild loop count or shortest distance"},"marker_refutation":self.refutation.report(),"key_lookup_calls":self.graph.key_lookup_calls.get(),"compositions":{"enabled":self.compositions_enabled,"programs":self.graph.program.compositions.programs,"edges":self.graph.program.compositions.edges.len(),"stats":self.composition_stats,"live_cursors":self.graph.cells.values().filter(|c|c.composition.is_some()).count()},"reference_views":self.graph.reference_report(),"local_congruence":self.graph.congruence_report(),"sieve":self.sieve.report(),"join_refinements":{"stats":self.query_indexes.refinements,"live_entries_and_references":self.query_indexes.refinement_storage()},"multi_stage":{"enabled":self.multistage_enabled,"entry_programs_enabled":self.point_programs,"local_projections_enabled":self.local_projections,"entry_programs":self.graph.program.multi_stage.entries.len(),"compiled_routes":self.graph.program.multi_stage.routes.len(),"stats":self.continuation_stats,"executed_rule_pairs":self.handoffs,"stage_patches":self.steps.len()},"query_profile": if self.profile_queries {Some(self.query_profile.iter().enumerate().filter(|(_,v)|v.0>0).map(|(rid,v)|json!({"rule":rid,"name":self.graph.program.templates[self.graph.program.rules[rid].template].name,"calls":v.0,"seconds":v.1,"candidates":v.2})).collect::<Vec<_>>())}else{None},"query_indexes":{"builds":self.query_indexes.builds,"decoded_facts":self.query_indexes.decoded_facts,"updated_facts":self.query_indexes.updated_facts,"peak_fact_references":self.query_indexes.peak_references,"scope":"versioned virtual-fact indexes; references only"},"query_mode":std::env::var("CELL_CPU_QUERY_MODE").unwrap_or_else(|_|"delta".into()),"backend":"cell-cpu","scope":"standalone Cell recipe executor; native frontend only; no egglog runtime linked","stats":self.graph.stats,"live_cells":self.graph.cells.len(),"stored_binding_values":self.graph.cells.values().map(|c|c.values.len()).sum::<usize>(),"virtual_constructor_facts":self.graph.cells.values().map(|c|c.facts.len()).sum::<usize>(),"function_ports":self.graph.functions.iter().filter(|f|f.live).count(),"promoted_aliases":self.graph.parents.len(),"steps":self.steps,"status":if self.tasks.is_empty(){"complete"}else{"checkpointed"}})
    }
}
