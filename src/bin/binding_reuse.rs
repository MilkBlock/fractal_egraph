//! Offline binding ablation fed by actual committed egglog producer writes.
//! This does not replace egglog's join engine or measure end-to-end acceleration.
use egglog::{EGraph, TraceSession, WriteOutcome};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;
type Row = (i64, i64); // connector key, payload
#[derive(Clone, Copy, Debug)]
struct Delta {
    side: bool,
    row: Row,
    producer: Option<u64>,
    parent_binding: Option<[i64; 2]>,
}
#[derive(Default)]
struct Stats {
    probes: usize,
    candidates: usize,
    dispatches: usize,
    outputs: BTreeSet<[i64; 3]>,
}
struct Case {
    name: &'static str,
    keys: usize,
    fanout: usize,
    waves: usize,
    direct: bool,
    guard: bool,
    bootstrap: bool,
}
fn output(stats: &mut Stats, p: Row, q: Row, guard: bool) {
    stats.candidates += 1;
    if !guard || p.1 < q.1 {
        stats.outputs.insert([p.1, p.0, q.1]);
    }
}
fn batches(case: &Case) -> Vec<Vec<Delta>> {
    let mut waves = vec![Vec::new(); case.waves];
    for key in 0..case.keys {
        for j in 0..case.fanout {
            let i = key * case.fanout + j;
            waves[i % case.waves].push(Delta {
                side: false,
                row: (key as i64, i as i64),
                producer: None,
                parent_binding: None,
            });
            if !case.direct {
                let z = if case.guard {
                    j as i64
                } else {
                    1_000_000 + i as i64
                };
                waves[(i + 1) % case.waves].push(Delta {
                    side: true,
                    row: (key as i64, z),
                    producer: None,
                    parent_binding: None,
                });
            }
        }
    }
    waves
}
fn mathematical_output(case: &Case, input: &[Vec<Delta>]) -> BTreeSet<[i64; 3]> {
    let ps: Vec<_> = input
        .iter()
        .flatten()
        .filter(|r| !r.side)
        .map(|r| r.row)
        .collect();
    let qs: Vec<_> = input
        .iter()
        .flatten()
        .filter(|r| r.side)
        .map(|r| r.row)
        .collect();
    let mut expected = BTreeSet::new();
    for &p in &ps {
        if case.direct {
            expected.insert([p.1, p.0, 0]);
            continue;
        }
        for &q in &qs {
            if p.0 == q.0 && (!case.guard || p.1 < q.1) {
                expected.insert([p.1, p.0, q.1]);
            }
        }
    }
    expected
}
fn native(case: &Case) -> (Vec<Vec<Delta>>, BTreeSet<[i64; 3]>, Json) {
    let consumer = if case.direct {
        "(rule ((P y x)) ((Out x y 0)) :ruleset consume :name \"consume\")".into()
    } else {
        format!(
            "(rule ((P y x) (Q y z) {}) ((Out x y z)) :ruleset consume :name \"consume\")",
            if case.guard { "(< x z)" } else { "" }
        )
    };
    let setup = format!(
        r#"
      (relation SeedP (i64 i64)) (relation SeedQ (i64 i64))
      (relation P (i64 i64)) (relation Q (i64 i64)) (relation Out (i64 i64 i64))
      (ruleset produce) (ruleset consume)
      (rule ((SeedP x y)) ((P y x)) :ruleset produce :name "make-p")
      (rule ((SeedQ z y)) ((Q y z)) :ruleset produce :name "make-q")
      {consumer}
    "#
    );
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, &setup).unwrap();
    let mut ordinary = eg.clone();
    let mut ordinary_producer_ns = 0;
    let mut ordinary_consumer_ns = 0;
    let t = TraceSession::with_dependencies();
    let mut committed = Vec::new();
    let input = batches(case);
    let mut seed_count = 0;
    let mut producer_ns = 0;
    let mut consumer_ns = 0;
    for (wave, rows) in input.iter().enumerate() {
        let mut text = String::new();
        let mut bootstrap = Vec::new();
        for &r in rows {
            if case.bootstrap && wave == 0 {
                text += &format!(
                    "({} {} {})\n",
                    if r.side { "Q" } else { "P" },
                    r.row.0,
                    r.row.1
                );
                bootstrap.push(r);
            } else {
                text += &format!(
                    "({} {} {})\n",
                    if r.side { "SeedQ" } else { "SeedP" },
                    r.row.1,
                    r.row.0
                );
                // Deliberate duplicate assertions must not generate duplicate notifications.
                text += &format!(
                    "({} {} {})\n",
                    if r.side { "SeedQ" } else { "SeedP" },
                    r.row.1,
                    r.row.0
                );
                seed_count += 2;
            }
        }
        eg.parse_and_run_program(None, &text).unwrap();
        ordinary.parse_and_run_program(None, &text).unwrap();
        let start = Instant::now();
        ordinary.step_rules("produce").unwrap();
        ordinary_producer_ns += start.elapsed().as_nanos();
        let before = t.write_events().len();
        let start = Instant::now();
        eg.step_rules_with_trace("produce", &t).unwrap();
        producer_ns += start.elapsed().as_nanos();
        let names: BTreeMap<_, _> = t.table_names().into_iter().collect();
        let matches: BTreeMap<_, _> = t.matches().into_iter().map(|m| (m.event_id, m)).collect();
        let mut delta = bootstrap;
        for w in t
            .write_events()
            .into_iter()
            .skip(before)
            .filter(|w| w.outcome == WriteOutcome::Inserted)
        {
            let name = names.get(&w.table).map(|s| s.as_ref());
            if name == Some("P") || name == Some("Q") {
                let m = &matches[&w.match_event_id];
                let side = name == Some("Q");
                assert_eq!(m.rule.as_ref(), if side { "make-q" } else { "make-p" });
                let binding = |name: &str| {
                    eg.value_to_base::<i64>(
                        m.bindings
                            .iter()
                            .find(|b| b.name.as_deref() == Some(name))
                            .unwrap()
                            .value,
                    )
                };
                let parent = [binding(if side { "z" } else { "x" }), binding("y")];
                let row = (eg.value_to_base(w.actual[0]), eg.value_to_base(w.actual[1]));
                assert_eq!(row, (parent[1], parent[0]));
                delta.push(Delta {
                    side,
                    row,
                    producer: Some(w.match_event_id),
                    parent_binding: Some(parent),
                });
            }
        }
        committed.push(delta);
        let start = Instant::now();
        eg.step_rules_with_trace("consume", &t).unwrap();
        consumer_ns += start.elapsed().as_nanos();
        let start = Instant::now();
        ordinary.step_rules("consume").unwrap();
        ordinary_consumer_ns += start.elapsed().as_nanos();
        let expected = mathematical_output(case, &input[..=wave]);
        assert_eq!(eg.get_size("Out"), expected.len());
        assert_eq!(ordinary.get_size("Out"), expected.len());
        for row in &expected {
            let key: Vec<_> = row.iter().map(|&v| eg.base_to_value(v)).collect();
            assert!(eg.lookup_function("Out", &key).is_some());
            let plain_key: Vec<_> = row.iter().map(|&v| ordinary.base_to_value(v)).collect();
            assert!(ordinary.lookup_function("Out", &plain_key).is_some());
        }
        for mode in [
            "full_rescan",
            "indexed_rescan",
            "delta_join",
            "provenance_dispatch",
        ] {
            assert_eq!(
                replay(case, &committed, mode).outputs,
                expected,
                "wave {wave}, mode {mode}"
            );
        }
    }
    let expected = mathematical_output(case, &input);
    assert_eq!(
        committed.iter().map(Vec::len).sum::<usize>(),
        input.iter().map(Vec::len).sum::<usize>()
    );
    let survived: BTreeSet<_> = t
        .action_outcomes()
        .iter()
        .filter(|o| o.outcome == egglog::RuleActionOutcome::Survived)
        .map(|o| o.match_event_id)
        .collect();
    let consumer_matches = t
        .matches()
        .iter()
        .filter(|m| m.rule.as_ref() == "consume")
        .count();
    let consumer_survived = t
        .matches()
        .iter()
        .filter(|m| m.rule.as_ref() == "consume" && survived.contains(&m.event_id))
        .count();
    (
        committed,
        expected,
        json!({"ordinary_producer_ns":ordinary_producer_ns,"ordinary_consumer_ns":ordinary_consumer_ns,"scope":"actual ordinary and dependency-traced egglog, equal after each wave; native times are single-run diagnostics, replay times are not end-to-end speedups", "producer_ns":producer_ns,"consumer_ns":consumer_ns,"consumer_candidates":consumer_matches,"consumer_survived":consumer_survived,"duplicate_seed_assertions":seed_count}),
    )
}
fn replay(case: &Case, batches: &[Vec<Delta>], mode: &str) -> Stats {
    let mut stats = Stats::default();
    let mut ps: Vec<Row> = Vec::new();
    let mut qs: Vec<Row> = Vec::new();
    let mut pi: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut qi: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for batch in batches {
        if mode == "delta_join" || mode == "provenance_dispatch" {
            for original in batch {
                let mut mapped = *original;
                if mode == "provenance_dispatch" {
                    stats.dispatches += 1;
                    if let Some(parent) = original.parent_binding {
                        // Reusable port program: key <- parent.y, payload <- parent.x/z.
                        mapped.row = (parent[1], parent[0]);
                    } // Explicit initial rows use the indexed fallback.
                }
                let d = &mapped;
                if case.direct {
                    if !d.side {
                        stats.candidates += 1;
                        stats.outputs.insert([d.row.1, d.row.0, 0]);
                    }
                    continue;
                }
                let (own, other) = if d.side {
                    (&mut qi, &pi)
                } else {
                    (&mut pi, &qi)
                };
                stats.probes += 1;
                for &v in other.get(&d.row.0).into_iter().flatten() {
                    let (p, q) = if d.side {
                        ((d.row.0, v), d.row)
                    } else {
                        (d.row, (d.row.0, v))
                    };
                    output(&mut stats, p, q, case.guard);
                }
                own.entry(d.row.0).or_default().push(d.row.1);
            }
        } else {
            for d in batch {
                if d.side {
                    qs.push(d.row);
                    qi.entry(d.row.0).or_default().push(d.row.1);
                } else {
                    ps.push(d.row);
                }
            }
            for &p in &ps {
                if case.direct {
                    stats.candidates += 1;
                    stats.outputs.insert([p.1, p.0, 0]);
                    continue;
                }
                if mode == "indexed_rescan" {
                    stats.probes += 1;
                    for &z in qi.get(&p.0).into_iter().flatten() {
                        output(&mut stats, p, (p.0, z), case.guard);
                    }
                } else {
                    for &q in &qs {
                        stats.probes += 1;
                        if p.0 == q.0 {
                            output(&mut stats, p, q, case.guard);
                        }
                    }
                }
            }
        }
    }
    stats
}
struct Factorized {
    // key, P offset/length, Q offset/length. Fixed-width lossless representation.
    groups: Vec<[i64; 5]>,
    p: Vec<i64>,
    q: Vec<i64>,
}
impl Factorized {
    fn build(stream: &[Vec<Delta>]) -> Self {
        let mut groups: BTreeMap<i64, (Vec<i64>, Vec<i64>)> = BTreeMap::new();
        for d in stream.iter().flatten() {
            let group = groups.entry(d.row.0).or_default();
            if d.side {
                group.1.push(d.row.1);
            } else {
                group.0.push(d.row.1);
            }
        }
        let mut result = Self {
            groups: vec![],
            p: vec![],
            q: vec![],
        };
        for (key, (p, q)) in groups {
            result.groups.push([
                key,
                result.p.len() as i64,
                p.len() as i64,
                result.q.len() as i64,
                q.len() as i64,
            ]);
            result.p.extend(p);
            result.q.extend(q);
        }
        result
    }
    fn encoded(&self) -> Vec<u8> {
        [
            self.groups.len() as i64,
            self.p.len() as i64,
            self.q.len() as i64,
        ]
        .iter()
        .chain(self.groups.iter().flatten())
        .chain(&self.p)
        .chain(&self.q)
        .flat_map(|x| x.to_le_bytes())
        .collect()
    }
    fn decode(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len() % 8, 0);
        let words: Vec<i64> = bytes
            .chunks_exact(8)
            .map(|x| i64::from_le_bytes(x.try_into().unwrap()))
            .collect();
        let (g, p, q) = (words[0] as usize, words[1] as usize, words[2] as usize);
        assert_eq!(words.len(), 3 + 5 * g + p + q);
        Self {
            groups: words[3..3 + 5 * g]
                .chunks_exact(5)
                .map(|x| x.try_into().unwrap())
                .collect(),
            p: words[3 + 5 * g..3 + 5 * g + p].to_vec(),
            q: words[3 + 5 * g + p..].to_vec(),
        }
    }
    fn enumerate(&self, case: &Case) -> BTreeSet<[i64; 3]> {
        let mut out = BTreeSet::new();
        for &[key, po, pn, qo, qn] in &self.groups {
            for &x in &self.p[po as usize..(po + pn) as usize] {
                if case.direct {
                    out.insert([x, key, 0]);
                    continue;
                }
                for &z in &self.q[qo as usize..(qo + qn) as usize] {
                    if !case.guard || x < z {
                        out.insert([x, key, z]);
                    }
                }
            }
        }
        out
    }
}
fn run(case: Case) -> Json {
    let (stream, expected, native) = native(&case);
    let mut modes = Vec::new();
    for mode in [
        "full_rescan",
        "indexed_rescan",
        "delta_join",
        "provenance_dispatch",
    ] {
        let mut times = Vec::new();
        let mut result = Stats::default();
        for _ in 0..7 {
            let start = Instant::now();
            result = replay(&case, &stream, mode);
            times.push(start.elapsed().as_nanos());
            assert_eq!(result.outputs, expected);
        }
        times.sort();
        modes.push(json!({"mode":mode,"median_ns":times[3],"index_probes_or_pair_comparisons":result.probes,"candidate_bindings":result.candidates,"origin_dispatches":result.dispatches,"unique_output_bindings":result.outputs.len()}));
    }
    let start = Instant::now();
    let packed = Factorized::build(&stream);
    let factor_build_ns = start.elapsed().as_nanos();
    let encoded = packed.encoded();
    let packed = Factorized::decode(&encoded);
    let start = Instant::now();
    assert_eq!(packed.enumerate(&case), expected);
    let factor_enumerate_ns = start.elapsed().as_nanos();
    let rows = stream.iter().flatten().count();
    let produced = stream
        .iter()
        .flatten()
        .filter(|d| d.producer.is_some())
        .count();
    // Exact fixed-width encoding sizes, not allocator/RSS estimates. Each side
    // has key buckets: i64 key + u64 offset + u64 length, and i64 payloads.
    let buckets = case.keys * if case.direct { 1 } else { 2 };
    let index_bytes = if case.direct {
        0
    } else {
        24 * buckets + 8 * rows
    };
    let bindings_bytes = 24 * expected.len();
    let origin_encoding: Vec<u8> = stream
        .iter()
        .flatten()
        .filter(|d| d.producer.is_some())
        .flat_map(|d| {
            let parent = d.parent_binding.unwrap();
            [
                d.producer.unwrap().to_le_bytes(),
                (d.side as u64).to_le_bytes(),
                parent[0].to_le_bytes(),
                parent[1].to_le_bytes(),
            ]
            .concat()
        })
        .collect();
    let origin_bytes = origin_encoding.len();
    assert_eq!(origin_bytes, 32 * produced); // u64 event + u64 rule + two i64 parent values
    json!({"factorized_validation":{"construct_ns":factor_build_ns,"enumerate_and_validate_ns":factor_enumerate_ns,"encoded_index_bytes":encoded.len(),"guard_program_required":case.guard},"case":case.name,"keys":case.keys,"fanout_per_key":case.fanout,"waves":case.waves,"native":native,"replay_scope":"standalone Rust ablation over real committed P/Q notifications plus explicit initial inputs; not an alternative egglog runtime", "input_rows":rows,"producer_origin_rows":produced,"bootstrap_rows":rows-produced,"results":modes,"fixed_width_encoding_bytes":{"shared_binding_index":index_bytes,"additional_origin_and_parent_records":origin_bytes,"materialized_output_bindings":bindings_bytes,"factorized_unguarded_output":if case.guard {Json::Null}else{json!(encoded.len())},"note":"Fixed-width bytes, not peak heap/RSS. shared_binding_index is a layout estimate; factorized and origin records are actually encoded. Factorized output is an alternative snapshot layout, not additive to the matching index; emitting distinct facts still needs enumeration."}})
}
fn main() {
    let cases = vec![
        Case {
            name: "smooth_permutation",
            keys: 256,
            fanout: 1,
            waves: 8,
            direct: true,
            guard: false,
            bootstrap: false,
        },
        Case {
            name: "coarse_unique_key",
            keys: 256,
            fanout: 1,
            waves: 8,
            direct: false,
            guard: false,
            bootstrap: false,
        },
        Case {
            name: "coarse_many_to_many",
            keys: 32,
            fanout: 8,
            waves: 8,
            direct: false,
            guard: false,
            bootstrap: false,
        },
        Case {
            name: "coarse_hot_key",
            keys: 1,
            fanout: 256,
            waves: 8,
            direct: false,
            guard: false,
            bootstrap: false,
        },
        Case {
            name: "coarse_guard",
            keys: 32,
            fanout: 8,
            waves: 8,
            direct: false,
            guard: true,
            bootstrap: false,
        },
        Case {
            name: "initial_fact_boundary",
            keys: 32,
            fanout: 8,
            waves: 8,
            direct: false,
            guard: false,
            bootstrap: true,
        },
    ];
    let result = json!({"experiment":"binding reuse ablation","build_mode":if cfg!(debug_assertions){"debug; timings are diagnostic only"}else{"release"},"cases":cases.into_iter().map(run).collect::<Vec<_>>()});
    let text = serde_json::to_string_pretty(&result).unwrap();
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, text).unwrap();
    } else {
        println!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guarded_late_inputs_and_initial_facts_agree_each_wave() {
        let case = Case {
            name: "test",
            keys: 3,
            fanout: 4,
            waves: 4,
            direct: false,
            guard: true,
            bootstrap: true,
        };
        let (mut stream, expected, _) = native(&case);
        for batch in &mut stream {
            batch.reverse();
        }
        assert_eq!(
            replay(&case, &stream, "provenance_dispatch").outputs,
            expected
        );
        let packed = Factorized::build(&stream);
        assert_eq!(
            Factorized::decode(&packed.encoded()).enumerate(&case),
            expected
        );
    }
    #[test]
    fn smooth_mapping_does_not_require_a_history_index() {
        let case = Case {
            name: "test",
            keys: 8,
            fanout: 1,
            waves: 4,
            direct: true,
            guard: false,
            bootstrap: false,
        };
        let (stream, expected, _) = native(&case);
        let result = replay(&case, &stream, "provenance_dispatch");
        assert_eq!(result.probes, 0);
        assert_eq!(result.candidates, expected.len());
    }
}
