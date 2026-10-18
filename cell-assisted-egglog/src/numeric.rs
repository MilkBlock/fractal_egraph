//! Exact scalar dataflow in the shared matrix IR. Identity coordinates and i64
//! payloads have different types. Grouping keys are pinned canonical identities;
//! only associative, commutative min/max merges may be reduced in parallel.
use crate::{Result, program::Program};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    Input { slot: usize },
    State { slot: usize },
    Const { value: i64 },
    Add { a: usize, b: usize },
    Sub { a: usize, b: usize },
    Mul { a: usize, b: usize },
    Min { a: usize, b: usize },
    Max { a: usize, b: usize },
}
impl Expr {
    pub fn args(&self) -> Vec<usize> {
        match self {
            Self::Add { a, b }
            | Self::Sub { a, b }
            | Self::Mul { a, b }
            | Self::Min { a, b }
            | Self::Max { a, b } => vec![*a, *b],
            _ => vec![],
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Compare {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}
impl Compare {
    fn test(self, a: i64, b: i64) -> bool {
        match self {
            Self::Lt => a < b,
            Self::Le => a <= b,
            Self::Gt => a > b,
            Self::Ge => a >= b,
            Self::Eq => a == b,
            Self::Ne => a != b,
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Merge {
    Min,
    Max,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub scope: String,
    pub key: Vec<u32>,
    pub merge: Merge,
    pub initial: Option<i64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub channel: usize,
    pub left: usize,
    pub right: usize,
    pub compare: Compare,
    pub depth: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    pub channel: usize,
    pub group: usize,
    pub value: usize,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// rows = binding channels, columns = typed i64 inputs / current-state slots
    pub inputs: Vec<Vec<i64>>,
    pub state_bindings: Vec<Vec<usize>>,
    pub exprs: Vec<Expr>,
    pub guards: Vec<Guard>,
    pub groups: Vec<Group>,
    pub writes: Vec<Write>,
}
impl Plan {
    pub fn check(&self, p: &Program) -> Result<()> {
        let lanes = p.channels.len();
        if self.inputs.len() != lanes || self.state_bindings.len() != lanes {
            return Err("numeric rows must align with binding channels".into());
        }
        let width = self.inputs.first().map_or(0, Vec::len);
        let sw = self.state_bindings.first().map_or(0, Vec::len);
        if self.inputs.iter().any(|r| r.len() != width)
            || self
                .state_bindings
                .iter()
                .any(|r| r.len() != sw || r.iter().any(|i| *i >= self.groups.len()))
        {
            return Err("numeric input/state matrix shape mismatch".into());
        }
        for (i, e) in self.exprs.iter().enumerate() {
            if e.args().iter().any(|a| *a >= i) {
                return Err("numeric expression cycle/forward reference".into());
            }
            match e {
                Expr::Input { slot } if *slot >= width => {
                    return Err("numeric input slot absent".into());
                }
                Expr::State { slot } if *slot >= sw => {
                    return Err("numeric state slot absent".into());
                }
                _ => (),
            }
        }
        for g in &self.guards {
            if g.channel >= lanes
                || g.left >= self.exprs.len()
                || g.right >= self.exprs.len()
                || g.depth >= u32::MAX as usize
            {
                return Err("invalid numeric guard".into());
            }
        }
        let mut keys = BTreeSet::new();
        for g in &self.groups {
            if !keys.insert((&g.scope, &g.key)) {
                return Err("duplicate canonical merge key; group before reduction".into());
            }
        }
        for w in &self.writes {
            if w.channel >= lanes || w.group >= self.groups.len() || w.value >= self.exprs.len() {
                return Err("invalid numeric write".into());
            }
            if p.channels[w.channel].target.is_some() {
                return Err(
                    "numeric writes and a pending Cell target require an explicit commit protocol"
                        .into(),
                );
            }
        }
        for stage in &p.stages {
            if self.writes.iter().any(|w| stage.contains(&w.channel))
                && stage.iter().any(|c| p.channels[*c].target.is_some())
            {
                return Err(
                    "split numeric mutation and Cell target publication into separate stages"
                        .into(),
                );
            }
        }
        Ok(())
    }
    pub fn new_state(&self) -> State {
        State {
            values: self.groups.iter().map(|g| g.initial).collect(),
            versions: vec![0; self.groups.len()],
            evidence: vec![0; self.guards.len()],
            tape: vec![vec![]; self.inputs.len()],
            valid: vec![false; self.inputs.len()],
            proposals: vec![None; self.writes.len()],
            status: vec![0; self.writes.len()],
            committed: vec![false; self.writes.len()],
            work: Work::default(),
            error: false,
        }
    }
}
#[derive(Default, Clone, Debug, Serialize, PartialEq)]
pub struct Work {
    pub expressions: usize,
    pub guards: usize,
    pub proposals: usize,
    pub reductions: usize,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Snapshot {
    pub values: Vec<Option<i64>>,
    pub versions: Vec<u32>,
    pub guards: Vec<u32>,
    pub proposals: Vec<Option<i64>>,
    pub status: Vec<u32>,
    pub arithmetic_error: bool,
    pub work: Work,
}
pub struct State {
    values: Vec<Option<i64>>,
    versions: Vec<u32>,
    evidence: Vec<u32>,
    tape: Vec<Vec<Option<i64>>>,
    valid: Vec<bool>,
    proposals: Vec<Option<i64>>,
    status: Vec<u32>,
    committed: Vec<bool>,
    work: Work,
    error: bool,
}
impl State {
    pub fn begin(&mut self, p: &Plan, stage: &[usize]) {
        self.work = Work::default();
        self.error = false;
        for &c in stage {
            self.valid[c] = false;
        }
        for (i, g) in p.guards.iter().enumerate() {
            if stage.contains(&g.channel) {
                self.evidence[i] = 0;
            }
        }
        for (i, w) in p.writes.iter().enumerate() {
            if stage.contains(&w.channel) {
                self.proposals[i] = None;
                self.status[i] = 0;
                self.committed[i] = false;
            }
        }
    }
    fn eval(&mut self, p: &Plan, c: usize) {
        if self.valid[c] {
            return;
        }
        let mut v: Vec<Option<i64>> = vec![];
        for e in &p.exprs {
            let a = e.args();
            let x = a.first().and_then(|i| v[*i]);
            let y = a.get(1).and_then(|i| v[*i]);
            let r = match e {
                Expr::Input { slot } => Some(p.inputs[c][*slot]),
                Expr::State { slot } => self.values[p.state_bindings[c][*slot]],
                Expr::Const { value } => Some(*value),
                Expr::Add { .. } => x.zip(y).and_then(|(a, b)| a.checked_add(b)),
                Expr::Sub { .. } => x.zip(y).and_then(|(a, b)| a.checked_sub(b)),
                Expr::Mul { .. } => x.zip(y).and_then(|(a, b)| a.checked_mul(b)),
                Expr::Min { .. } => x.zip(y).map(|(a, b)| a.min(b)),
                Expr::Max { .. } => x.zip(y).map(|(a, b)| a.max(b)),
            };
            v.push(r);
            self.work.expressions += 1;
        }
        self.tape[c] = v;
        self.valid[c] = true;
    }
    pub fn gate(&self, p: &Plan, c: usize) -> u32 {
        let mut pending = false;
        for (i, _) in p.guards.iter().enumerate().filter(|(_, g)| g.channel == c) {
            match self.evidence[i] {
                1 => return 1,
                0 => pending = true,
                _ => (),
            }
        }
        if pending { 0 } else { 2 }
    }
    pub fn guards(
        &mut self,
        n: &Plan,
        p: &Program,
        stage: &[usize],
        depth: usize,
        decisions: &[u32],
        live: &[u32],
    ) {
        for &c in stage {
            if decisions[c] != 0 || p.channels[c].sources.iter().any(|i| live[*i] != 1) {
                continue;
            }
            for (i, g) in n
                .guards
                .iter()
                .enumerate()
                .filter(|(_, g)| g.channel == c && g.depth <= depth)
            {
                if self.evidence[i] != 0 {
                    continue;
                }
                self.eval(n, c);
                self.evidence[i] = if self.tape[c][g.left]
                    .zip(self.tape[c][g.right])
                    .is_some_and(|(a, b)| g.compare.test(a, b))
                {
                    2
                } else {
                    1
                };
                self.work.guards += 1;
            }
        }
    }
    pub fn commit(&mut self, p: &Plan, stage: &[usize], decisions: &[u32]) {
        for (i, w) in p.writes.iter().enumerate() {
            if !stage.contains(&w.channel) || decisions[w.channel] != 1 || self.committed[i] {
                continue;
            }
            self.eval(p, w.channel);
            let v = self.tape[w.channel][w.value];
            self.proposals[i] = v;
            self.status[i] = if v.is_some() { 1 } else { 2 };
            self.error |= v.is_none();
            self.work.proposals += 1;
        }
        if self.error {
            return;
        }
        for (g, group) in p.groups.iter().enumerate() {
            let mut value = self.values[g];
            let mut used = false;
            for (i, w) in p.writes.iter().enumerate() {
                if w.group != g
                    || !stage.contains(&w.channel)
                    || self.committed[i]
                    || self.status[i] != 1
                {
                    continue;
                }
                let v = self.proposals[i].unwrap();
                value = Some(value.map_or(v, |old| match group.merge {
                    Merge::Min => old.min(v),
                    Merge::Max => old.max(v),
                }));
                self.committed[i] = true;
                used = true;
            }
            if used {
                self.work.reductions += 1;
                if value != self.values[g] {
                    self.versions[g] += 1;
                }
                self.values[g] = value;
            }
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            values: self.values.clone(),
            versions: self.versions.clone(),
            guards: self.evidence.clone(),
            proposals: self.proposals.clone(),
            status: self.status.clone(),
            arithmetic_error: self.error,
            work: self.work.clone(),
        }
    }
    pub fn failed(&self) -> bool {
        self.error
    }
}
