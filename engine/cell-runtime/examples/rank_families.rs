//! Rank read-only survey replays against independent coarse clocks.
use serde::Deserialize;
use serde_json::{Value as J, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;
type Rules = BTreeSet<usize>;
type Axis = (usize, Option<usize>);
type Key = (u64, u64, usize);
fn load(path: &str) -> Result<J> {
    Ok(serde_json::from_reader(std::io::BufReader::new(
        std::fs::File::open(path)?,
    ))?)
}
fn u(j: &J, key: &str) -> u64 {
    j[key].as_u64().unwrap_or(0)
}
fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}
fn med(mut a: Vec<f64>) -> f64 {
    a.sort_by(f64::total_cmp);
    (a[(a.len() - 1) / 2] + a[a.len() / 2]) / 2.
}
#[derive(Deserialize)]
struct RuleClock {
    calls: u64,
    active: u64,
    ns: u64,
    rhs_ns: u64,
    answers: u64,
    visits: u64,
    resumes: u64,
}
#[derive(Deserialize)]
struct TableClock {
    headers: u64,
    column_rows: u64,
    range_rows: u64,
    neighbor_rows: u64,
    sole_column_ns: u64,
    column_batch_ns: u64,
    union_headers: u64,
}
#[derive(Deserialize)]
struct Clocks {
    rules: Vec<RuleClock>,
    tables: Vec<TableClock>,
    active_samples: Vec<[u64; 8]>,
    schedule: Vec<(u64, String)>,
}
#[derive(Deserialize)]
struct Replay {
    axes: Vec<(Axis, J)>,
    families: Vec<Family>,
}
#[derive(Deserialize)]
struct Family {
    set: String,
    members: Vec<usize>,
    axes: Vec<usize>,
    stats: J,
    windows: Vec<(u64, u64, Vec<usize>, u64, u64)>,
}
struct Program {
    tables: Vec<String>,
    reads: Vec<Rules>,
    by_rule: Vec<Vec<usize>>,
    labels: Vec<String>,
    sets: Vec<String>,
    ports: Rules,
}
impl Program {
    fn new(p: &J) -> Result<Self> {
        let tables: Vec<String> = p["recipe"]["tables"]
            .as_array()
            .ok_or("missing tables")?
            .iter()
            .map(|t| t["name"].as_str().unwrap().into())
            .collect();
        let id: BTreeMap<_, _> = tables
            .iter()
            .enumerate()
            .map(|(i, t)| (t.as_str(), i))
            .collect();
        let mut reads = vec![Rules::new(); tables.len()];
        let mut by_rule = vec![];
        let mut labels = vec![];
        let mut sets = vec![];
        for (i, r) in p["recipe"]["rules"]
            .as_array()
            .ok_or("missing rules")?
            .iter()
            .enumerate()
        {
            let mut ts = r["query"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|a| a["call"]["table"].as_str().map(|n| id[n]))
                .collect::<Vec<_>>();
            for &t in &ts {
                reads[t].insert(i);
            }
            ts.sort();
            by_rule.push(ts);
            labels.push(r["metadata"]["name"].as_str().unwrap_or("").to_owned());
            sets.push(
                r["metadata"]["source_set"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            );
        }
        // Derive ports for legacy artifacts too, through the normal checked loader.
        let program = eggfuse_cell_runtime::program::Program::parse(p.clone())?;
        Ok(Self {
            tables,
            reads,
            by_rule,
            labels,
            sets,
            ports: program.survey_view_rules(),
        })
    }
}
fn annotated(metric: &str, v: f64) -> (String, f64) {
    (metric.to_owned(), v)
}
type Metrics = BTreeMap<String, f64>;
type Ledger = BTreeMap<String, (J, Vec<Metrics>)>;
fn record(
    out: &mut Ledger,
    id: String,
    description: J,
    metrics: impl IntoIterator<Item = (String, f64)>,
) {
    out.entry(id)
        .or_insert_with(|| (description, vec![]))
        .1
        .push(metrics.into_iter().collect());
}
fn summary(ledger: Ledger, order: &str) -> Vec<J> {
    let mut result = vec![];
    for (id, (desc, runs)) in ledger {
        let mut row = desc;
        row["id"] = id.into();
        row["samples"] = runs.len().into();
        for key in runs[0].keys() {
            let vals = runs.iter().map(|r| r[key]).collect::<Vec<_>>();
            row[key] = med(vals).into();
        }
        result.push(row);
    }
    result.sort_by(|a, b| {
        b[order]
            .as_f64()
            .unwrap_or(0.)
            .total_cmp(&a[order].as_f64().unwrap_or(0.))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    result
}
fn compatible(files: &[J], sha: &J, ports: Option<bool>) -> Result<()> {
    for f in files {
        if f["schema"] != "eggfuse-family-survey/v1"
            || &f["program_sha256"] != sha
            || ports.is_some_and(|p| f["view_ports"] != p)
        {
            return Err("mixed recipe or execution-mode survey inputs".into());
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [program, clocks, replays, detail, fronts, scans, output] = args.as_slice() else {
        return Err("rank_families PROGRAM CLOCK_FILES REPLAY_FILES OVERLAP_FILES|- FRONTIER_FILES|- SCAN_FILES|- NEW_OUTPUT.json (comma-separated matched replicate lists)".into());
    };
    if Path::new(output).exists() {
        return Err("use a new output file".into());
    }
    let load_list = |s: &str| -> Result<Vec<J>> {
        if s == "-" {
            return Ok(vec![]);
        }
        s.split(',').map(load).collect()
    };
    let mut raw = load(program)?;
    let digest = raw
        .as_object_mut()
        .ok_or("invalid program")?
        .remove("sha256")
        .ok_or("missing artifact digest")?;
    if digest != eggfuse_cell_runtime::checksum::artifact_digest(&raw) {
        return Err("invalid artifact digest".into());
    }
    raw["sha256"] = digest;
    let sha = &raw["sha256"];
    let p = Program::new(&raw)?;
    let clocks = load_list(clocks)?;
    let replays = load_list(replays)?;
    let detail = load_list(detail)?;
    let fronts = load_list(fronts)?;
    let scans = load_list(scans)?;
    let enhanced = clocks
        .first()
        .and_then(|f| f["view_ports"].as_bool())
        .ok_or("missing execution mode")?;
    for f in [&clocks, &replays, &detail, &fronts, &scans] {
        compatible(f, sha, Some(enhanced))?;
    }
    if clocks.len() != replays.len()
        || [&detail, &fronts, &scans]
            .iter()
            .any(|a| !a.is_empty() && a.len() != clocks.len())
    {
        return Err("replicate counts must agree".into());
    }
    let mut rules = Ledger::new();
    let mut tables = Ledger::new();
    let mut families = Ledger::new();
    let mut broad = Ledger::new();
    let mut entry_roof = Ledger::new();
    let mut fragments = Ledger::new();
    let mut raw_scans = Ledger::new();
    let mut drivers = Ledger::new();
    let mut projections = Ledger::new();
    let mut physical = Ledger::new();
    let mut totals = vec![];
    for (rep, (c, rp)) in clocks.into_iter().zip(replays).enumerate() {
        let c: Clocks = serde_json::from_value(c)?;
        let r: Replay = serde_json::from_value(rp["replay"].clone())?;
        if c.rules.len() != p.labels.len() || c.tables.len() != p.tables.len() {
            return Err("survey has wrong table or rule dimensions".into());
        }
        let sets: BTreeMap<_, _> = c.schedule.iter().cloned().collect();
        let observed: BTreeSet<_> = c.schedule.iter().map(|s| s.1.clone()).collect();
        let mut sample = BTreeMap::<Key, [u64; 8]>::new();
        let mut potential = BTreeMap::<(u64, u64, Vec<usize>), Vec<usize>>::new();
        for a in &c.active_samples {
            let (e, w, rule) = (a[0], a[1], a[2] as usize);
            if sample.insert((e, w, rule), *a).is_some() {
                return Err("duplicate rule in one scheduled occurrence".into());
            }
            if !p.by_rule[rule].is_empty() {
                potential
                    .entry((e, w, p.by_rule[rule].clone()))
                    .or_default()
                    .push(rule);
            }
        }
        let mut full_scope = BTreeSet::<Key>::new();
        let mut gross = 0;
        let mut wide_gross = 0;
        let mut shape_totals = BTreeMap::<(String, Vec<usize>), (u64, u64, Rules)>::new();
        for ((e, w, ts), rs) in potential.into_iter().filter(|(_, rs)| rs.len() > 1) {
            let cost = rs.iter().map(|&rule| sample[&(e, w, rule)][3]).sum::<u64>();
            gross += cost;
            for &rid in &rs {
                full_scope.insert((e, w, rid));
            }
            if ts.len() > 2 {
                wide_gross += cost
            }
            let v = shape_totals.entry((sets[&e].clone(), ts)).or_default();
            v.0 += cost;
            v.1 += 1;
            v.2.extend(rs);
        }
        for ((set, ts), (cost, windows, members)) in shape_totals {
            let names: Vec<_> = ts.iter().map(|t| &p.tables[*t]).collect();
            record(
                &mut broad,
                format!("{set}/{ts:?}"),
                json!({"set":set,"tables":names,"rules":members}),
                [
                    annotated("whole_search_ceiling_ms", ms(cost)),
                    annotated("windows", windows as f64),
                ],
            );
        }
        for (rule, c) in c.rules.iter().enumerate() {
            record(
                &mut rules,
                rule.to_string(),
                json!({"rule":rule,"source_set":p.sets[rule],"source":p.labels[rule],"tables":p.by_rule[rule].iter().map(|t|&p.tables[*t]).collect::<Vec<_>>()}),
                [
                    annotated("search_ms", ms(c.ns)),
                    annotated("rhs_ms", ms(c.rhs_ns)),
                    annotated("calls", c.calls as f64),
                    annotated("active", c.active as f64),
                    annotated("bindings", c.answers as f64),
                    annotated("residual_visits", c.visits as f64),
                    annotated("resumes", c.resumes as f64),
                ],
            );
        }
        for (t, c) in c.tables.iter().enumerate() {
            record(
                &mut tables,
                p.tables[t].clone(),
                json!({"readers":p.reads[t]}),
                [
                    annotated("column_batch_ms", ms(c.column_batch_ns)),
                    annotated("column_alone_ms", ms(c.sole_column_ns)),
                    annotated("headers", c.headers as f64),
                    annotated("union_run_headers", c.union_headers as f64),
                    annotated("column_rows", c.column_rows as f64),
                    annotated("range_rows", c.range_rows as f64),
                    annotated("neighbor_rows", c.neighbor_rows as f64),
                ],
            );
        }
        let mut net_positive = 0.;
        let mut no_spine_positive = 0.;
        let mut full_keys = BTreeSet::new();
        let mut coactive_ns = 0;
        for f in r.families {
            let members: Rules = f.members.iter().copied().collect();
            let mut ordinary = 0;
            let mut unclaimed = 0;
            let mut uf = 0;
            let mut unclaimed_windows = 0;
            let mut real_windows = BTreeSet::new();
            let mut verified = 0;
            for (e, w, ms, fused, _) in &f.windows {
                let mut guarded = false;
                let mut n = 0;
                for &rule in ms {
                    let key = (*e, *w, rule);
                    let a = sample
                        .get(&key)
                        .ok_or("replay window absent from original clock")?;
                    if sets.get(e) != Some(&f.set)
                        || !members.contains(&rule)
                        || !full_keys.insert(key)
                    {
                        return Err("overlapping family or wrong schedule".into());
                    }
                    real_windows.insert(key);
                    n += a[3];
                    guarded |= a[6] != 0 || a[7] != 0;
                    verified += a[4];
                }
                ordinary += n;
                if !guarded {
                    unclaimed += n;
                    uf += fused;
                    unclaimed_windows += 1
                }
            }
            if verified != u(&f.stats, "answers") {
                return Err("ordered output coverage differs between probes".into());
            }
            coactive_ns += ordinary;
            let axis_ids: BTreeSet<_> = f.axes.iter().copied().collect();
            let rels: BTreeSet<_> = axis_ids.iter().map(|&a| r.axes[a].0.0).collect();
            let cost = |key: &str| -> u64 {
                if f.windows.is_empty() && u(&f.stats, "failed_ns") == 0 {
                    0
                } else {
                    axis_ids.iter().map(|&a| u(&r.axes[a].1, key)).sum()
                }
            };
            let maint = cost("update_ns");
            let cold = cost("cold_ns");
            let union = cost("union_run_ns");
            let mut exclusive = vec![];
            let mut credit = 0;
            for &t in &rels {
                // A single uncovered active reader keeps a demand-built index necessary.
                let all_covered = p.reads[t].is_subset(&members)
                    && sample
                        .keys()
                        .filter(|(_, _, rule)| p.reads[t].contains(rule))
                        .all(|key| real_windows.contains(key));
                if all_covered
                    && c.tables[t].column_rows > 0
                    && c.tables[t].range_rows == 0
                    && c.tables[t].neighbor_rows == 0
                    && u(&f.stats, "mismatches") == 0
                {
                    exclusive.push(p.tables[t].clone());
                    credit += c.tables[t].sole_column_ns;
                }
            }
            let spent = ms(u(&f.stats, "fused_ns") + u(&f.stats, "failed_ns") + maint + cold);
            let net = ms(ordinary + credit) - spent;
            let residual = ms(unclaimed) - ms(uf + u(&f.stats, "failed_ns") + cold + maint)
                + if unclaimed_windows == f.windows.len() {
                    ms(credit)
                } else {
                    0.
                };
            let known = enhanced && members.is_subset(&p.ports);
            let valid = u(&f.stats, "mismatches") == 0 && !f.windows.is_empty();
            if valid && !known {
                net_positive += net.max(0.);
                no_spine_positive += residual.max(0.);
            }
            let axes: Vec<_> = axis_ids
                .iter()
                .map(|&a| {
                    let ((t, col), _) = r.axes[a];
                    json!([p.tables[t], col])
                })
                .collect();
            record(
                &mut families,
                format!("{}/{:?}", f.set, f.members),
                json!({"rules":f.members,"set":f.set,"axes":axes,"conditional_index_candidates":exclusive,"already_compiled":known,"ordered_match":valid}),
                [
                    annotated("model_net_ms", net),
                    annotated("unclaimed_model_ms", residual),
                    annotated("coactive_search_ms", ms(ordinary)),
                    annotated("unclaimed_search_ms", ms(unclaimed)),
                    annotated("fused_ms", ms(u(&f.stats, "fused_ns"))),
                    annotated("split_ms", ms(u(&f.stats, "split_ns"))),
                    annotated(
                        "enumerate_once_ms",
                        ms(u(&f.stats, "split_ns")) - ms(u(&f.stats, "fused_ns")),
                    ),
                    annotated("failure_ms", ms(u(&f.stats, "failed_ns"))),
                    annotated("build_ms", ms(cold)),
                    annotated("updates_ms", ms(maint)),
                    annotated("union_run_updates_ms", ms(union)),
                    annotated("index_credit_ms", ms(credit)),
                    annotated("headers", cost("changes") as f64),
                    annotated("rekeys", cost("rekeys") as f64),
                    annotated("retirements", cost("retirements") as f64),
                    annotated("windows", f.windows.len() as f64),
                    annotated("unclaimed_windows", unclaimed_windows as f64),
                    annotated("mismatches", u(&f.stats, "mismatches") as f64),
                    annotated("budget_skips", u(&f.stats, "skips") as f64),
                    annotated(
                        "late_peer_outputs_before_guards",
                        u(&f.stats, "late_peer") as f64,
                    ),
                    annotated("both_fresh_before_guards", u(&f.stats, "both_fresh") as f64),
                    annotated("bindings", verified as f64),
                    annotated(
                        "whole_workload_resumes",
                        members.iter().map(|&i| c.rules[i].resumes).sum::<u64>() as f64,
                    ),
                ],
            );
        }
        let mut repeat_ns = 0.;
        let mut memo_positive = 0.;
        let mut physical_ns = 0.;
        let mut shared_drivers = 0;
        if let Some(x) = detail.get(rep) {
            let x = &x["overlap"];
            let floor = u(x, "clock_floor_ns");
            let memo: BTreeMap<Vec<JKey>, u64> =
                serde_json::from_value::<Vec<(Vec<JKey>, u64)>>(x["memo_ns"].clone())?
                    .into_iter()
                    .collect();
            for entry in x["projections"].as_array().ok_or("missing projections")? {
                let k = &entry[0];
                let v = &entry[1];
                let id = k[2].as_u64().unwrap() as usize;
                let driver = k[1].as_u64().unwrap() as usize;
                let set = &x["sets"][k[0].as_u64().unwrap() as usize];
                let target = raw["free_join_entry"]["projections"][id]["table"]
                    .as_u64()
                    .unwrap() as usize;
                let measured = u(v, "repeat_ns").saturating_sub(u(v, "repeat") * floor);
                let cost = memo[&serde_json::from_value::<Vec<JKey>>(k.clone())?]
                    .saturating_sub(u(v, "events") * floor);
                repeat_ns += ms(measured);
                memo_positive += (ms(measured) - ms(cost)).max(0.);
                record(
                    &mut projections,
                    format!("{set}/{driver}/{id}/{}", k[3]),
                    json!({"set":set,"driver":p.tables[driver],"target":p.tables[target],"projection":id,"rules":v["members"],"columns":raw["free_join_entry"]["projections"][id]["columns"]}),
                    [
                        annotated("repeated_ms", ms(measured)),
                        annotated("memo_ms", ms(cost)),
                        annotated("memo_net_ms", ms(measured) - ms(cost)),
                        annotated("repeated_accesses", u(v, "repeat") as f64),
                        annotated("original_cache_hits", u(v, "repeat_cache_hits") as f64),
                        annotated("original_probes", u(v, "repeat_probes") as f64),
                    ],
                );
            }
            for entry in x["drivers"].as_array().ok_or("missing drivers")? {
                let k = &entry[0];
                let v = &entry[1];
                shared_drivers += u(v, "repeat");
                let set = &x["sets"][k[0].as_u64().unwrap() as usize];
                let t = k[1].as_u64().unwrap() as usize;
                record(
                    &mut drivers,
                    format!("{set}/{}", k[2]),
                    json!({"set":set,"table":p.tables[t],"rules":v["members"]}),
                    [
                        annotated("repeat", u(v, "repeat") as f64),
                        annotated("visits", u(v, "events") as f64),
                    ],
                );
            }
            for entry in x["physical"].as_array().ok_or("missing physical reads")? {
                let k = &entry[0];
                let v = &entry[1];
                let set = &x["sets"][k[0].as_u64().unwrap() as usize];
                let t = k[1].as_u64().unwrap() as usize;
                let ns = u(v, "repeat_ns").saturating_sub(u(v, "repeat") * floor);
                physical_ns += ms(ns);
                record(
                    &mut physical,
                    format!("{set}/{t}/{}", k[2]),
                    json!({"set":set,"table":p.tables[t],"channel":(["point","neighbor_hit","column_candidates"][k[2].as_u64().unwrap()as usize]),"rules":v["members"]}),
                    [
                        annotated("repeated_ms", ms(ns)),
                        annotated("repeat", u(v, "repeat") as f64),
                    ],
                );
            }
        }
        let mut remaining_entry = 0;
        if let Some(f) = fronts.get(rep) {
            let batches: Vec<[u64; 9]> = serde_json::from_value(f["entry_batches"].clone())?;
            let mut visits = BTreeMap::<(u64, u64, usize), Vec<[u64; 9]>>::new();
            for a in batches {
                visits
                    .entry((a[0], a[1], a[4] as usize))
                    .or_default()
                    .push(a);
            }
            let mut scopes = BTreeMap::<(String, usize), (u64, u64, u64, u64, Rules)>::new();
            for ((e, w, node), rs) in visits {
                let rr: Rules = rs.iter().map(|r| r[2] as usize).collect();
                if rr.len() < 2 {
                    continue;
                }
                let v = scopes.entry((sets[&e].clone(), node)).or_default();
                v.4.extend(rr);
                for a in rs {
                    v.0 += a[8];
                    v.1 += a[5];
                    v.2 += a[7];
                    if !full_scope.contains(&(e, w, a[2] as usize)) {
                        v.3 += a[8];
                        remaining_entry += a[8];
                    }
                }
            }
            for ((set, node), (ns, visits, spine, remaining, members)) in scopes {
                record(
                    &mut entry_roof,
                    format!("{set}/{node}"),
                    json!({"set":set,"node":node,"table":raw["entry_schedule"]["nodes"][node]["table"],"rules":members}),
                    [
                        annotated("inclusive_admission_ms", ms(ns)),
                        annotated("outside_full_multiset_ms", ms(remaining)),
                        annotated("delivered_to_admission", visits as f64),
                        annotated("spine_claims", spine as f64),
                    ],
                );
            }
        }
        let mut entry_positive = 0.;
        if let Some(x) = fronts.get(rep) {
            for f in x["entry_frontiers"]
                .as_array()
                .ok_or("missing frontier replay")?
            {
                let c = &f["cost"];
                let baseline = ms(u(c, "baseline_hit_ns"));
                let copy = ms(u(c, "copy_ns"));
                let setup = ms(u(c, "setup_ns") + u(c, "release_ns"));
                let net = baseline - copy - setup;
                if u(c, "invalid") == 0 {
                    entry_positive += net.max(0.)
                }
                record(
                    &mut fragments,
                    format!("{}/{}/{}", f["set"], f["node"], f["signature"]),
                    json!({"set":f["set"],"table":raw["entry_schedule"]["nodes"][f["node"].as_u64().unwrap()as usize]["table"],"rules":f["members"]}),
                    [
                        annotated("frontier_net_ms", net),
                        annotated("original_repeat_ms", baseline),
                        annotated("copy_ms", copy),
                        annotated("first_store_and_release_ms", setup),
                        annotated("after_union_store_ms", ms(u(c, "union_store_ns"))),
                        annotated("hits", u(c, "hits") as f64),
                        annotated("rows", u(c, "rows") as f64),
                        annotated("mismatches", u(c, "invalid") as f64),
                    ],
                );
            }
        }
        let mut scan_positive = 0.;
        if let Some(x) = scans.get(rep) {
            for f in x["raw_scan"]
                .as_array()
                .ok_or("missing driver scan replay")?
            {
                let c = &f["cost"];
                let old = ms(u(c, "original_ns"));
                let shared =
                    ms(u(c, "repeat_vector_ns") + u(c, "first_collect_ns") + u(c, "release_ns"));
                let t = f["table"].as_u64().unwrap() as usize;
                scan_positive += (old - shared).max(0.);
                record(
                    &mut raw_scans,
                    format!("{}/{}", f["set"], f["node"]),
                    json!({"set":f["set"],"table":p.tables[t],"node":f["node"]}),
                    [
                        annotated("scan_net_ms", old - shared),
                        annotated("shared_array_ms", shared),
                        annotated("original_scan_ms", old),
                        annotated("repeat_scan_ms", ms(u(c, "repeat_scan_ns"))),
                        annotated("reused_rows", u(c, "shared_rows") as f64),
                        annotated("hits", u(c, "hits") as f64),
                        annotated("budget_skips", u(c, "caps") as f64),
                    ],
                );
            }
        }
        totals.push(json!({"compiled_rules":c.rules.len(),"scheduled_rules":c.rules.iter().filter(|r|r.calls>0).count(),"active_rules":c.rules.iter().filter(|r|r.active>0).count(),"producing_rules":c.rules.iter().filter(|r|r.answers>0).count(),"scheduled_sets":observed.len(),"runs":sets.len(),
        "all_rule_search_ms":ms(c.rules.iter().map(|r|r.ns).sum()),"all_rule_rhs_ms":ms(c.rules.iter().map(|r|r.rhs_ns).sum()),"same_relation_multiset_whole_search_ceiling_ms":ms(gross),"three_or_more_relations_ceiling_ms":ms(wide_gross),"verified_family_search_ms":ms(coactive_ns),"other_shared_entry_inclusive_ms":fronts.get(rep).map(|_|ms(remaining_entry)),
        "positive_new_replay_ms":net_positive,"positive_new_unclaimed_ms":no_spine_positive,"shared_admission_positive_ms":fronts.get(rep).map(|_|entry_positive),"projection_repeated_ms":detail.get(rep).map(|_|repeat_ns),"projection_positive_memo_ms":detail.get(rep).map(|_|memo_positive),"physical_repeated_ms":detail.get(rep).map(|_|physical_ns),"shared_driver_revisits":detail.get(rep).map(|_|shared_drivers),"raw_scan_positive_ms":scans.get(rep).map(|_|scan_positive),
        "all_existing_column_batches_ms":ms(c.tables.iter().map(|t|t.column_batch_ns).sum()),"all_column_only_batches_ms":ms(c.tables.iter().map(|t|t.sole_column_ns).sum())}));
    }
    let result = json!({"program":sha,"view_ports":enhanced,"totals_per_replicate":totals,"families":summary(families,"model_net_ms"),"same_tables":summary(broad,"whole_search_ceiling_ms"),"rules":summary(rules,"search_ms"),"tables":summary(tables,"column_batch_ms"),"driver_entry_budgets":summary(entry_roof,"inclusive_admission_ms"),"driver_scan_replay":summary(raw_scans,"scan_net_ms"),"entry_fragments":summary(fragments,"frontier_net_ms"),"projections":summary(projections,"repeated_ms"),"physical_reads":summary(physical,"repeated_ms"),"entry_nodes":summary(drivers,"repeat")});
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(std::io::BufWriter::new(file), &result)?;
    Ok(())
}
// Projection identifiers are integer components followed by one tie-direction bit.
#[derive(Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(untagged)]
enum JKey {
    Number(u64),
    Bit(bool),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rank_inputs_cannot_mix_modes_or_recipes() {
        let f =
            json!({"schema":"eggfuse-family-survey/v1","view_ports":true,"program_sha256":"one"});
        assert!(compatible(&[f.clone()], &json!("two"), Some(true)).is_err());
        assert!(compatible(&[f.clone()], &json!("one"), Some(false)).is_err());
        assert!(compatible(&[f], &json!("one"), Some(true)).is_ok());
    }
}
