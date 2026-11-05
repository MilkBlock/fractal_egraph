//! Two-pass diagnostic. The second pass asks when the two keys of a previously
//! observed forwarding first become equal under explicit queued RHS unions.
//! The oracle's target never participates in execution or matching.
use crate::{
    graph::{FactRef, Graph},
    program::Result,
    value::{Parents, Value, canon, root},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) struct Pair {
    pub epoch: u64,
    pub rule: usize,
    pub source: FactRef,
    pub keeper: FactRef,
}
#[derive(Default, Serialize)]
struct Counts {
    tested: u64,
    at_insert: u64,
    pending_prefix: u64,
    later_rhs: u64,
    needs_repair: u64,
}
struct Keys {
    pair: Pair,
    left: Vec<Value>,
    right: Vec<Value>,
    prefix: bool,
}
#[derive(Default)]
pub(crate) struct Proof {
    expected: Option<BTreeMap<FactRef, Pair>>,
    observed: Vec<Pair>,
    queue: Vec<Keys>,
    shadow: Parents,
    applied: usize,
    by_rule: BTreeMap<usize, Counts>,
}
impl Proof {
    pub fn load(&mut self, g: &Graph, path: &std::path::Path) -> Result<()> {
        #[derive(Deserialize)]
        struct Data {
            program: String,
            pairs: Vec<Pair>,
        }
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let data: Data =
            serde_json::from_reader(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
        if g.program.artifact["sha256"] != data.program {
            return Err("keeper program mismatch".into());
        }
        let mut m = BTreeMap::new();
        for p in data.pairs {
            if m.insert(p.source, p).is_some() {
                return Err("duplicate keeper source".into());
            }
        }
        self.expected = Some(m);
        Ok(())
    }
    pub fn expects(&self, f: FactRef) -> Option<Pair> {
        self.expected.as_ref()?.get(&f).copied()
    }
    fn extend(&mut self, parents: &Parents, unions: &[(u64, u64)]) {
        for &(a, b) in &unions[self.applied..] {
            let a = root(&self.shadow, root(parents, a));
            let b = root(&self.shadow, root(parents, b));
            if a != b {
                self.shadow.insert(a.max(b), a.min(b));
            }
        }
        self.applied = unions.len();
    }
    fn equal(&self, a: &[Value], b: &[Value]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| canon(x, &self.shadow) == canon(y, &self.shadow))
    }
    pub fn at_birth(
        &mut self,
        pair: Pair,
        epoch: u64,
        key: &[Value],
        old: &[Value],
        parents: &Parents,
        unions: &[(u64, u64)],
    ) -> Result<()> {
        if epoch != pair.epoch {
            return Err("keeper epoch mismatch".into());
        }
        self.extend(parents, unions);
        let prefix = self.equal(key, old);
        let c = self.by_rule.entry(pair.rule).or_default();
        c.tested += 1;
        if key == old {
            c.at_insert += 1
        } else if prefix {
            c.pending_prefix += 1
        }
        self.queue.push(Keys {
            pair,
            left: key.to_vec(),
            right: old.to_vec(),
            prefix,
        });
        Ok(())
    }
    pub fn before_repair(&mut self, parents: &Parents, unions: &[(u64, u64)]) {
        if !self.queue.is_empty() {
            self.extend(parents, unions);
            for k in std::mem::take(&mut self.queue) {
                if !k.prefix {
                    let late = self.equal(&k.left, &k.right);
                    let c = self.by_rule.get_mut(&k.pair.rule).unwrap();
                    if late {
                        c.later_rhs += 1
                    } else {
                        c.needs_repair += 1
                    }
                }
            }
        }
        self.applied = 0;
        self.shadow.clear();
    }
    pub fn winner(&mut self, p: Pair) -> Result<()> {
        if let Some(m) = &mut self.expected {
            let prev = m.remove(&p.source).ok_or("unexpected winning keeper")?;
            if prev.epoch != p.epoch || prev.rule != p.rule || prev.keeper != p.keeper {
                return Err("winning keeper differs".into());
            }
        }
        self.observed.push(p);
        Ok(())
    }
    pub fn write(&self, g: &Graph, dir: &std::path::Path) -> Result<()> {
        if self.expected.as_ref().is_some_and(|m| {
            !m.is_empty()
                || self.by_rule.values().map(|c| c.tested).sum::<u64>()
                    != self.observed.len() as u64
        }) {
            return Err("incomplete keeper replay".into());
        }
        let j = serde_json::json!({"program":g.program.artifact["sha256"],"pairs":self.observed,"by_rule":self.by_rule});
        std::fs::write(
            dir.join("keeper-proof.json"),
            serde_json::to_vec(&j).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
