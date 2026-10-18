//! Shared matrix program: relative coordinates, whole-binding alternatives and
//! target gathers. No egglog table names, runtime Join plans or guessed Cells.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Port,
    Lift2,
    Mean,
    Concat,
    Moments,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub op: Op,
    #[serde(default)]
    pub args: Vec<usize>,
    #[serde(default)]
    pub slot: usize,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub template: usize,
    pub binding: Option<Vec<u32>>,
    pub partition: Vec<usize>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Wire(pub usize, pub usize);
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub left: Wire,
    pub right: Wire,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub condition: usize,
    pub depth: usize,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// False for a runtime request whose closure has not completed Bake yet.
    #[serde(default = "yes")]
    pub certified: bool,
    pub cell: usize,
    pub bindings: Vec<Wire>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specializations: Vec<Specialization>,
}
fn yes() -> bool {
    true
}
/// A certified alias case, installed offline or by the live native Baker. The exact partition guards the whole raw
/// target frame, before quotienting slots. The target owns a distinct template
/// and coordinate frame; we never relabel an unproved old Cell as valid.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Specialization {
    pub cell: usize,
    pub partition: Vec<usize>,
    pub projection: Vec<usize>,
    pub certificate: Value,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub label: String,
    pub sources: Vec<usize>,
    pub alternatives: Vec<Vec<Requirement>>,
    pub target: Option<Target>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub cell: usize,
    pub slot: usize,
    pub value: u32,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub version: u32,
    pub bits: usize,
    pub slots: usize,
    pub nodes: Vec<Node>,
    pub cells: Vec<Cell>,
    pub templates: Vec<Value>,
    pub conditions: Vec<Condition>,
    pub channels: Vec<Channel>,
    pub stages: Vec<Vec<usize>>,
    #[serde(default)]
    pub updates: Vec<Vec<Update>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric: Option<crate::numeric::Plan>,
    /// Scope/provenance comes from the compiler's baked evidence. This field is
    /// not itself a proof and does not authorize an unknown semantic domain.
    pub evidence: Value,
}
#[derive(Clone, Debug)]
pub struct Feature {
    pub width: usize,
    pub bound: i64,
    pub offset: usize,
    pub shape: String,
    pub support: BTreeSet<usize>,
}
#[derive(Clone, Debug)]
pub struct Group {
    pub width: usize,
    pub wires: Vec<Wire>,
    pub conditions: Vec<usize>,
}
#[derive(Clone, Debug)]
pub struct Checked {
    pub program: Program,
    pub features: Vec<Feature>,
    pub width: usize,
    pub known: Vec<u32>,
    pub groups: Vec<Group>,
    pub levels: Vec<Vec<usize>>,
}
impl Program {
    pub fn check(self) -> Result<Checked> {
        if self.version != 1
            || !(1..=32).contains(&self.bits)
            || self.slots == 0
            || self.slots >= u32::MAX as usize
            || self.cells.is_empty()
        {
            return Err("invalid matrix program dimensions/version".into());
        }
        if let Some(n) = &self.numeric {
            n.check(&self)?;
        }
        let mut features: Vec<Feature> = vec![];
        let mut width = 0usize;
        for (i, n) in self.nodes.iter().enumerate() {
            if n.args.iter().any(|a| *a >= i) {
                return Err("feature graph must be topologically ordered".into());
            }
            let args = n.args.iter().map(|a| &features[*a]).collect::<Vec<_>>();
            let (w, b, shape, support) = match n.op {
                Op::Port => {
                    if !args.is_empty() || n.slot >= self.slots {
                        return Err("bad port".into());
                    };
                    (self.bits, 1, "I".into(), BTreeSet::from([n.slot]))
                }
                Op::Lift2 => {
                    if args.len() != 1 || self.nodes[n.args[0]].op != Op::Port {
                        return Err("lift2 requires signed-bit input".into());
                    };
                    let a = args[0];
                    (
                        a.width * (a.width + 1) / 2,
                        1,
                        "lift(I)".into(),
                        a.support.clone(),
                    )
                }
                Op::Mean => {
                    if args.len() != 2 || n.args.iter().any(|a| self.nodes[*a].op != Op::Lift2) {
                        return Err("mean requires two signed-bit pair lifts".into());
                    };
                    let a = args[0];
                    let b = args[1];
                    if a.width != b.width {
                        return Err("mean shape mismatch".into());
                    };
                    (
                        a.width,
                        1,
                        "A(I,I)".into(),
                        a.support.union(&b.support).copied().collect(),
                    )
                }
                Op::Concat => {
                    if args.len() != 2 {
                        return Err("concat arity".into());
                    };
                    let a = args[0];
                    let b = args[1];
                    (
                        a.width + b.width,
                        a.bound.max(b.bound),
                        format!("C({},{})", a.shape, b.shape),
                        a.support.union(&b.support).copied().collect(),
                    )
                }
                Op::Moments => {
                    if args.len() != 2 || args[0].shape != args[1].shape {
                        return Err("moments requires equal child shapes".into());
                    };
                    let a = args[0];
                    let b = args[1];
                    let bound = a.bound.max(b.bound);
                    (
                        a.width + a.width * (a.width + 1) / 2,
                        2 * bound * bound,
                        format!("A({},{})", a.shape, b.shape),
                        a.support.union(&b.support).copied().collect(),
                    )
                }
            };
            // Conservative integer FP32 contract for feature arithmetic and Gram distances.
            if w == 0 || 4i128 * w as i128 * b as i128 * b as i128 >= 1 << 24 {
                return Err(
                    "coordinate lift exceeds exact FP32 range; choose another representation"
                        .into(),
                );
            }
            features.push(Feature {
                width: w,
                bound: b,
                offset: width,
                shape,
                support,
            });
            width = width.checked_add(w).ok_or("feature size overflow")?;
        }
        if features.is_empty() {
            return Err("empty feature program".into());
        }
        for c in &self.cells {
            if c.template >= self.templates.len() {
                return Err("missing Cell template descriptor".into());
            }
            if c.partition.len() > self.slots
                || c.partition
                    .iter()
                    .enumerate()
                    .any(|(i, p)| *p > i || c.partition[*p] != *p)
            {
                return Err("invalid equality partition".into());
            }
            if let Some(v) = &c.binding {
                if v.len() != c.partition.len()
                    || v.iter()
                        .any(|x| self.bits < 32 && (*x as u64) >= 1u64 << self.bits)
                {
                    return Err("input coordinate domain mismatch".into());
                }
            }
        }
        let mut known = vec![];
        let mut groups: Vec<Group> = vec![];
        for (ci, c) in self.conditions.iter().enumerate() {
            for w in [c.left, c.right] {
                if w.0 >= self.cells.len()
                    || w.1 >= features.len()
                    || features[w.1]
                        .support
                        .iter()
                        .any(|p| *p >= self.cells[w.0].partition.len())
                {
                    return Err("condition reads an absent coordinate".into());
                }
            }
            let a = &features[c.left.1];
            let b = &features[c.right.1];
            let k = if a.shape != b.shape {
                1
            } else if c.left == c.right {
                2
            } else if c.left.0 == c.right.0
                && self.nodes[c.left.1].op == Op::Port
                && self.nodes[c.right.1].op == Op::Port
            {
                if self.cells[c.left.0].partition[self.nodes[c.left.1].slot]
                    == self.cells[c.right.0].partition[self.nodes[c.right.1].slot]
                {
                    2
                } else {
                    1
                }
            } else {
                0
            };
            known.push(k);
            if k != 0 {
                continue;
            }
            let g = if let Some(g) = groups
                .iter()
                .position(|g| features[g.wires[0].1].shape == a.shape)
            {
                g
            } else {
                groups.push(Group {
                    width: a.width,
                    wires: vec![c.left],
                    conditions: vec![],
                });
                groups.len() - 1
            };
            groups[g].conditions.push(ci);
            groups[g].wires.extend([c.left, c.right]);
        }
        for g in &mut groups {
            g.wires.sort();
            g.wires.dedup();
        }
        let mut claimed = self
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.binding.is_some())
            .map(|(i, _)| i)
            .collect::<BTreeSet<_>>();
        let mut channel_seen = BTreeSet::new();
        let mut levels = vec![];
        for stage in &self.stages {
            let mut targets = BTreeSet::new();
            let mut ds = BTreeSet::new();
            for &ci in stage {
                let c = self.channels.get(ci).ok_or("invalid channel")?;
                if !channel_seen.insert(ci)
                    || c.sources.is_empty()
                    || c.sources.iter().any(|s| !claimed.contains(s))
                {
                    return Err("channel/source order violation".into());
                }
                for branch in &c.alternatives {
                    for r in branch {
                        if r.depth >= u32::MAX as usize {
                            return Err("depth exceeds the matrix ABI range".into());
                        }
                        let cond = self
                            .conditions
                            .get(r.condition)
                            .ok_or("invalid condition")?;
                        if !c.sources.contains(&cond.left.0) || !c.sources.contains(&cond.right.0) {
                            return Err("binding condition reads an undeclared source".into());
                        }
                        ds.insert(r.depth);
                    }
                }
                if let Some(t) = &c.target {
                    let target = self.cells.get(t.cell).ok_or("invalid target")?;
                    if claimed.contains(&t.cell)
                        || !targets.insert(t.cell)
                        || t.bindings.len() != target.partition.len()
                        || t.bindings.iter().any(|w| {
                            !c.sources.contains(&w.0) || w.1 >= self.cells[w.0].partition.len()
                        })
                    {
                        return Err("target gather/order violation".into());
                    }
                    let mut partitions = BTreeSet::new();
                    if t.certified {
                        partitions.insert(target.partition.clone());
                    }
                    for sp in &t.specializations {
                        let out = self
                            .cells
                            .get(sp.cell)
                            .ok_or("invalid specialization target")?;
                        if claimed.contains(&sp.cell)
                            || !targets.insert(sp.cell)
                            || sp.partition.len() != t.bindings.len()
                            || sp
                                .partition
                                .iter()
                                .enumerate()
                                .any(|(i, v)| *v > i || sp.partition[*v] != *v)
                            || !partitions.insert(sp.partition.clone())
                            || sp.projection.len() != out.partition.len()
                            || sp.projection.iter().any(|v| *v >= t.bindings.len())
                            || sp.certificate.is_null()
                        {
                            return Err(
                                "invalid specialization frame/projection/certificate".into()
                            );
                        }
                        for i in 0..sp.projection.len() {
                            for j in 0..i {
                                if (sp.partition[sp.projection[i]]
                                    == sp.partition[sp.projection[j]])
                                    != (out.partition[i] == out.partition[j])
                                {
                                    return Err(
                                        "specialized target partition differs from projected frame"
                                            .into(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let Some(n) = &self.numeric {
                ds.extend(
                    n.guards
                        .iter()
                        .filter(|g| stage.contains(&g.channel))
                        .map(|g| g.depth),
                );
            }
            claimed.extend(targets);
            levels.push(ds.into_iter().collect());
        }
        if channel_seen.len() != self.channels.len() {
            return Err("unexecuted channel in program".into());
        }
        for updates in &self.updates {
            let mut seen = BTreeSet::new();
            for u in updates {
                let c = self.cells.get(u.cell).ok_or("bad update cell")?;
                if c.binding.is_none()
                    || u.slot >= c.partition.len()
                    || !seen.insert((u.cell, u.slot))
                    || self.bits < 32 && (u.value as u64) >= 1u64 << self.bits
                {
                    return Err("invalid input update".into());
                }
            }
        }
        Ok(Checked {
            program: self,
            features,
            width,
            known,
            groups,
            levels,
        })
    }
}

#[derive(Default, Clone, Debug, Serialize, PartialEq)]
pub struct Work {
    pub matrix_calls: usize,
    pub matrix_positions: usize,
    pub matrix_macs: usize,
    pub requested_conditions: usize,
    pub computed_features: usize,
    pub reused_features: usize,
    pub feature_components_written: usize,
    pub feature_adds: usize,
    pub feature_multiplies: usize,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Level {
    pub depth: Option<usize>,
    pub complete: usize,
    pub rejected: usize,
    pub continued: usize,
    pub work: Work,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Stage {
    pub levels: Vec<Level>,
    pub decisions: Vec<u32>,
    pub cells: Vec<Vec<u32>>,
    pub live: Vec<u32>,
    pub target_status: Vec<u32>,
    pub specialization_targets: Vec<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric: Option<crate::numeric::Snapshot>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Report {
    pub backend: String,
    pub epochs: Vec<Vec<Stage>>,
}
struct State {
    binding: Vec<Vec<u32>>,
    live: Vec<u32>,
    tape: Vec<Vec<Vec<i64>>>,
    valid: Vec<Vec<bool>>,
    evidence: Vec<u32>,
    decision: Vec<u32>,
    numeric: Option<crate::numeric::State>,
}
impl Checked {
    fn invalidate(&self, s: &mut State, dirty: &[BTreeSet<usize>]) {
        for (c, slots) in dirty.iter().enumerate() {
            for (n, f) in self.features.iter().enumerate() {
                if !f.support.is_disjoint(slots) {
                    s.valid[c][n] = false
                }
            }
        }
        for (i, c) in self.program.conditions.iter().enumerate() {
            if !s.valid[c.left.0][c.left.1] || !s.valid[c.right.0][c.right.1] {
                s.evidence[i] = self.known[i]
            }
        }
    }
    fn partition(&self, values: &[u32], p: &[usize]) -> bool {
        (0..p.len()).all(|i| (0..i).all(|j| (values[i] == values[j]) == (p[i] == p[j])))
    }
    fn classify(&self, s: &mut State, stage: &[usize], depth: Option<usize>) -> BTreeSet<usize> {
        let mut needed = BTreeSet::new();
        for &ci in stage {
            let c = &self.program.channels[ci];
            if s.decision[ci] != 0 {
                continue;
            }
            if c.sources.iter().any(|x| s.live[*x] == 0) {
                s.decision[ci] = 2;
                continue;
            }
            if c.sources.iter().any(|x| s.live[*x] != 1) {
                continue;
            }
            let numeric = s
                .numeric
                .as_ref()
                .zip(self.program.numeric.as_ref())
                .map_or(2, |(s, p)| s.gate(p, ci));
            if numeric == 1 {
                s.decision[ci] = 2;
                continue;
            }
            let alive = c
                .alternatives
                .iter()
                .filter(|b| b.iter().all(|r| s.evidence[r.condition] != 1))
                .collect::<Vec<_>>();
            if alive.is_empty() {
                s.decision[ci] = 2;
                continue;
            }
            if alive
                .iter()
                .any(|b| b.iter().all(|r| s.evidence[r.condition] == 2))
            {
                if numeric == 2 {
                    s.decision[ci] = 1;
                }
                continue;
            }
            if let Some(d) = depth {
                for b in alive {
                    for r in b {
                        if r.depth <= d && s.evidence[r.condition] == 0 {
                            needed.insert(r.condition);
                        }
                    }
                }
            }
        }
        needed
    }
    fn build(&self, s: &mut State, wires: &[Wire], work: &mut Work) {
        let p = &self.program;
        let mut need = vec![vec![false; p.nodes.len()]; p.cells.len()];
        for Wire(c, n) in wires {
            need[*c][*n] = true;
        }
        for c in 0..p.cells.len() {
            for n in (0..p.nodes.len()).rev() {
                if need[c][n] && !s.valid[c][n] {
                    for a in &p.nodes[n].args {
                        need[c][*a] = true
                    }
                }
            }
            for (n, node) in p.nodes.iter().enumerate() {
                if !need[c][n] {
                    continue;
                }
                if s.valid[c][n] {
                    work.reused_features += 1;
                    continue;
                }
                let a = node
                    .args
                    .first()
                    .map(|a| s.tape[c][*a].clone())
                    .unwrap_or_default();
                let b = node
                    .args
                    .get(1)
                    .map(|b| s.tape[c][*b].clone())
                    .unwrap_or_default();
                s.tape[c][n] = match node.op {
                    Op::Port => (0..p.bits)
                        .map(|bit| {
                            if s.binding[c][node.slot] >> bit & 1 == 1 {
                                1
                            } else {
                                -1
                            }
                        })
                        .collect(),
                    Op::Lift2 => {
                        let mut v = a.clone();
                        for i in 0..a.len() {
                            for j in i + 1..a.len() {
                                v.push(a[i] * a[j]);
                            }
                        }
                        v
                    }
                    Op::Mean => a.iter().zip(&b).map(|(x, y)| (x + y) / 2).collect(),
                    Op::Concat => a.iter().chain(&b).copied().collect(),
                    Op::Moments => {
                        let mut v = a.iter().zip(&b).map(|(x, y)| x + y).collect::<Vec<_>>();
                        for i in 0..a.len() {
                            for j in i..a.len() {
                                v.push(a[i] * a[j] + b[i] * b[j]);
                            }
                        }
                        v
                    }
                };
                s.valid[c][n] = true;
                work.computed_features += 1;
                work.feature_components_written += self.features[n].width;
                match node.op {
                    Op::Lift2 => work.feature_multiplies += a.len() * (a.len() - 1) / 2,
                    Op::Mean => {
                        work.feature_adds += a.len();
                        work.feature_multiplies += a.len();
                    }
                    Op::Moments => {
                        work.feature_adds += self.features[n].width;
                        work.feature_multiplies += a.len() * (a.len() + 1);
                    }
                    _ => (),
                }
            }
        }
    }
    fn level(&self, s: &State, stage: &[usize], depth: Option<usize>, work: Work) -> Level {
        Level {
            depth,
            complete: stage.iter().filter(|i| s.decision[**i] == 1).count(),
            rejected: stage.iter().filter(|i| s.decision[**i] == 2).count(),
            continued: stage.iter().filter(|i| s.decision[**i] == 0).count(),
            work,
        }
    }
    pub fn execute(&self) -> Report {
        let p = &self.program;
        let mut s = State {
            binding: p
                .cells
                .iter()
                .map(|c| {
                    let mut v = c.binding.clone().unwrap_or_default();
                    v.resize(p.slots, 0);
                    v
                })
                .collect(),
            live: p
                .cells
                .iter()
                .map(|c| u32::from(c.binding.is_some()))
                .collect(),
            tape: vec![vec![vec![]; p.nodes.len()]; p.cells.len()],
            valid: vec![vec![false; p.nodes.len()]; p.cells.len()],
            evidence: self.known.clone(),
            decision: vec![0; p.channels.len()],
            numeric: p.numeric.as_ref().map(|n| n.new_state()),
        };
        let mut epochs = vec![];
        'epochs: for epoch in 0..=p.updates.len() {
            let mut dirty = vec![BTreeSet::new(); p.cells.len()];
            if epoch > 0 {
                for u in &p.updates[epoch - 1] {
                    if s.binding[u.cell][u.slot] != u.value {
                        s.binding[u.cell][u.slot] = u.value;
                        dirty[u.cell].insert(u.slot);
                    }
                }
            }
            self.invalidate(&mut s, &dirty);
            for (i, c) in p.cells.iter().enumerate() {
                if c.binding.is_none() {
                    s.live[i] = 0;
                }
                if c.binding.is_some() {
                    s.live[i] = if self.partition(&s.binding[i], &c.partition) {
                        1
                    } else {
                        2
                    };
                }
            }
            let mut stages = vec![];
            for (si, stage) in p.stages.iter().enumerate() {
                if let Some(n) = &mut s.numeric {
                    n.begin(p.numeric.as_ref().unwrap(), stage);
                }
                for c in stage {
                    s.decision[*c] = 0;
                }
                self.classify(&mut s, stage, None);
                let mut levels = vec![self.level(&s, stage, None, Work::default())];
                for &depth in &self.levels[si] {
                    let mut work = Work::default();
                    if let Some(n) = &mut s.numeric {
                        n.guards(
                            p.numeric.as_ref().unwrap(),
                            p,
                            stage,
                            depth,
                            &s.decision,
                            &s.live,
                        );
                    }
                    for g in &self.groups {
                        let needed = self.classify(&mut s, stage, Some(depth));
                        let selected = g
                            .conditions
                            .iter()
                            .copied()
                            .filter(|c| needed.contains(c) && s.evidence[*c] == 0)
                            .collect::<Vec<_>>();
                        if selected.is_empty() {
                            continue;
                        }
                        work.requested_conditions += selected.len();
                        let ws = selected
                            .iter()
                            .flat_map(|c| [p.conditions[*c].left, p.conditions[*c].right])
                            .collect::<BTreeSet<_>>();
                        let wires = ws.iter().copied().collect::<Vec<_>>();
                        self.build(&mut s, &wires, &mut work);
                        work.matrix_calls += 1;
                        work.matrix_positions += wires.len() * wires.len();
                        work.matrix_macs += wires.len() * wires.len() * g.width;
                        // The GPU computes the complete Gram block; retain all
                        // condition results whose endpoints were actually covered.
                        for &ci in &g.conditions {
                            let c = &p.conditions[ci];
                            if ws.contains(&c.left) && ws.contains(&c.right) {
                                let a = &s.tape[c.left.0][c.left.1];
                                let b = &s.tape[c.right.0][c.right.1];
                                s.evidence[ci] = if a == b { 2 } else { 1 };
                            }
                        }
                    }
                    self.classify(&mut s, stage, None);
                    levels.push(self.level(&s, stage, Some(depth), work));
                }
                if let Some(n) = &mut s.numeric {
                    n.commit(p.numeric.as_ref().unwrap(), stage, &s.decision);
                }
                let mut target_status = vec![];
                let mut specialization_targets = vec![];
                let mut dirty = vec![BTreeSet::new(); p.cells.len()];
                for &ci in stage {
                    let c = &p.channels[ci];
                    let mut status = 0;
                    let mut specialized = None;
                    if let Some(t) = &c.target {
                        s.live[t.cell] = 0;
                        for sp in &t.specializations {
                            s.live[sp.cell] = if s.decision[ci] == 0 { 2 } else { 0 };
                        }
                        if s.decision[ci] == 1 {
                            let vals = t
                                .bindings
                                .iter()
                                .map(|w| s.binding[w.0][w.1])
                                .collect::<Vec<_>>();
                            for (slot, &v) in vals.iter().enumerate() {
                                if s.binding[t.cell][slot] != v {
                                    dirty[t.cell].insert(slot);
                                }
                                s.binding[t.cell][slot] = v;
                            }
                            status = if t.certified
                                && self.partition(&vals, &p.cells[t.cell].partition)
                            {
                                1
                            } else {
                                2
                            };
                            s.live[t.cell] = status;
                            if status == 2 {
                                for sp in &t.specializations {
                                    if !self.partition(&vals, &sp.partition) {
                                        continue;
                                    }
                                    for (slot, from) in sp.projection.iter().enumerate() {
                                        let v = vals[*from];
                                        if s.binding[sp.cell][slot] != v {
                                            dirty[sp.cell].insert(slot);
                                        }
                                        s.binding[sp.cell][slot] = v;
                                    }
                                    s.live[sp.cell] = 1;
                                    specialized = Some(sp.cell);
                                    status = 3;
                                }
                            }
                        } else if s.decision[ci] == 0 {
                            s.live[t.cell] = 2;
                        }
                    }
                    target_status.push(status);
                    specialization_targets.push(specialized);
                }
                self.invalidate(&mut s, &dirty);
                stages.push(Stage {
                    levels,
                    decisions: stage.iter().map(|i| s.decision[*i]).collect(),
                    cells: s.binding.clone(),
                    live: s.live.clone(),
                    target_status,
                    specialization_targets,
                    numeric: s.numeric.as_ref().map(|n| n.snapshot()),
                });
                if s.numeric.as_ref().is_some_and(|n| n.failed()) {
                    epochs.push(stages);
                    break 'epochs;
                }
            }
            epochs.push(stages);
        }
        Report {
            backend: "cpu-reference".into(),
            epochs,
        }
    }
}
