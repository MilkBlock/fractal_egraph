//! Diagnostic samples from the original schedule. No survey state is consulted by execution.
use crate::program::Program;
use serde::Serialize;

#[derive(Default, Serialize)]
pub(crate) struct RuleClock {
    rhs_ns: u64,
    resumes: u64,
    calls: u64,
    active: u64,
    ns: u64,
    answers: u64,
    visits: u64,
}
#[derive(Default, Serialize)]
pub(crate) struct TableClock {
    batches: u64,
    headers: u64,
    wakes: u64,
    column_rows: u64,
    range_rows: u64,
    neighbor_rows: u64,
    column_batch_ns: u64,
    sole_column_ns: u64,
    union_headers: u64,
}
pub(crate) struct Survey {
    schedule: Vec<(u64, String)>,
    rules: Vec<RuleClock>,
    tables: Vec<TableClock>,
    // [epoch, watermark, rule, search ns, bindings, residual visits, live Spine, local/direct binding]
    pub(crate) samples: Vec<[u64; 8]>,
}
impl Survey {
    pub fn new(p: &Program) -> Self {
        Self {
            schedule: vec![],
            rules: (0..p.rules.len()).map(|_| Default::default()).collect(),
            tables: (0..p.tables.len()).map(|_| Default::default()).collect(),
            samples: vec![],
        }
    }
    pub fn start(&mut self, epoch: u64, set: &str) {
        self.schedule.push((epoch, set.into()));
    }
    pub fn rhs(&mut self, r: usize, ns: u64) {
        self.rules[r].rhs_ns += ns
    }
    pub fn resumed(&mut self, r: usize, n: u64) {
        self.rules[r].resumes += n
    }
    pub fn rule(
        &mut self,
        epoch: u64,
        wm: u64,
        r: usize,
        active: bool,
        ns: u64,
        answers: usize,
        visits: u64,
        spine: bool,
        special: bool,
    ) {
        let s = &mut self.rules[r];
        s.calls += 1;
        s.ns += ns;
        s.answers += answers as u64;
        s.visits += visits;
        if active {
            s.active += 1;
            self.samples.push([
                epoch,
                wm,
                r as u64,
                ns,
                answers as u64,
                visits,
                spine as u64,
                special as u64,
            ]);
        }
    }
    pub fn table(
        &mut self,
        ns: u64,
        t: usize,
        n: u64,
        wakes: u64,
        union: bool,
        column: bool,
        range: bool,
        neighbor: bool,
    ) {
        let s = &mut self.tables[t];
        s.batches += 1;
        s.headers += n;
        s.wakes += wakes;
        if union {
            s.union_headers += n
        }
        if column {
            s.column_rows += n;
            s.column_batch_ns += ns;
            if !range && !neighbor {
                s.sole_column_ns += ns
            }
        }
        if range {
            s.range_rows += n
        }
        if neighbor {
            s.neighbor_rows += n
        }
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"rules":self.rules,"tables":self.tables,"active_samples":self.samples,"schedule":self.schedule})
    }
}
