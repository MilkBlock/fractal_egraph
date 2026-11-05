//! Time just the driver enumeration that is identical across consumers. An
//! original inverse plan has a different domain and never enters this replay.
use super::*;
use crate::entry_schedule::Batch;
use serde::Serialize;
use std::hint::black_box;
#[derive(Default, Serialize)]
struct Cost {
    calls: u64,
    hits: u64,
    shared_rows: u64,
    original_ns: u64,
    repeat_scan_ns: u64,
    first_collect_ns: u64,
    repeat_vector_ns: u64,
    release_ns: u64,
    caps: u64,
}
#[derive(Default)]
struct Node {
    epoch: Option<u64>,
    windows: BTreeMap<u64, (usize, Vec<(u64, Dependency)>)>,
    refs: usize,
    cost: Cost,
}
#[derive(Default)]
pub(super) struct Scan {
    nodes: BTreeMap<(String, usize, usize), Node>,
    possible: BTreeSet<(String, usize)>,
}
impl Scan {
    pub fn new(p: &Program) -> Self {
        let mut s = Self::default();
        for (set, rules) in &p.sets {
            let mut nodes = BTreeMap::<_, BTreeSet<_>>::new();
            for &r in rules {
                for (_, node) in p.entry_schedule.survey_nodes(r) {
                    nodes.entry(node).or_default().insert(r);
                }
            }
            for (n, rs) in nodes {
                if rs.len() > 1 {
                    s.possible.insert((set.clone(), n));
                }
            }
        }
        s
    }
    pub fn record(&mut self, set: &str, epoch: u64, rid: usize, b: Batch<'_>) {
        let n = b.node();
        if !self.possible.contains(&(set.into(), n)) {
            return;
        }
        let node = self.nodes.entry((set.into(), n, b.table)).or_default();
        let wm = b.watermark();
        if node.epoch != Some(epoch) {
            let c = Instant::now();
            node.windows.clear();
            node.refs = 0;
            node.epoch = Some(epoch);
            node.cost.release_ns += c.elapsed().as_nanos() as u64;
        }
        let baseline = || {
            let c = Instant::now();
            for item in b.delta() {
                black_box(item);
            }
            c.elapsed().as_nanos() as u64
        };
        let mut ns = 0;
        let first = node.cost.calls % 2 == 0;
        if first {
            ns = baseline();
        }
        if let Some((prior, rows)) = node.windows.get(&wm) {
            if *prior == rid {
                return;
            }
            let c = Instant::now();
            for item in rows {
                black_box(item);
            }
            node.cost.repeat_vector_ns += c.elapsed().as_nanos() as u64;
            if !first {
                ns = baseline();
            }
            node.cost.repeat_scan_ns += ns;
            node.cost.hits += 1;
            node.cost.shared_rows += rows.len() as u64;
        } else {
            let len = b.delta_len().unwrap_or(0);
            if node.refs + len > 65536 {
                node.cost.caps += 1;
                return;
            }
            let c = Instant::now();
            let rows = b
                .delta()
                .map(|&item| {
                    black_box(item);
                    item
                })
                .collect::<Vec<_>>();
            node.refs += rows.len();
            node.windows.insert(wm, (rid, rows));
            node.cost.first_collect_ns += c.elapsed().as_nanos() as u64;
            if !first {
                ns = baseline();
            }
        }
        node.cost.original_ns += ns;
        node.cost.calls += 1;
    }
    pub fn report(&self) -> J {
        J::Array(
            self.nodes
                .iter()
                .map(|((s, n, t), v)| json!({"set":s,"node":n,"table":t,"cost":v.cost}))
                .collect(),
        )
    }
}
