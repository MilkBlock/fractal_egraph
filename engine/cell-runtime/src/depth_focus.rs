//! Lower a profiled consumer port to copies of producer receipts. A later Run
//! reads the already flattened receipt and checks the live row's full key.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

type EdgeName = (String, usize, String, usize);
#[derive(Serialize, Deserialize)]
pub(super) struct Catalog {
    schema: String,
    paths: Vec<Vec<EdgeName>>,
}
const SCHEMA: &str = "eggfuse/structural-spine-ports/v1";
const PER_DEPTH: usize = 8;

fn name(p: &Program, edge: &crate::depth_plan::Edge) -> EdgeName {
    (
        p.tables[edge.0].name.clone(),
        edge.1,
        p.tables[edge.2].name.clone(),
        edge.3,
    )
}
impl Catalog {
    pub fn rank(s: &Depth, p: &Program) -> Self {
        let mut counts = BTreeMap::new();
        for (&(kind, lane, _, _, path), &count) in &s.shapes {
            if kind != 0 || lane != 1 || path.len < 2 {
                continue;
            }
            let names = path.edges[..path.len as usize]
                .iter()
                .map(|i| name(p, &s.plan.edges[*i as usize]))
                .collect::<Vec<_>>();
            *counts.entry(names).or_insert(0u64) += count;
        }
        let mut paths = vec![];
        for k in 2..=MAX_DEPTH {
            let mut rows = counts
                .iter()
                .filter(|(edges, _)| edges.len() == k)
                .collect::<Vec<_>>();
            rows.sort_by(|(a, n), (b, m)| m.cmp(n).then_with(|| a.cmp(b)));
            paths.extend(
                rows.into_iter()
                    .take(PER_DEPTH)
                    .map(|(path, _)| path.clone()),
            );
        }
        Self {
            schema: SCHEMA.into(),
            paths,
        }
    }
}
fn from_edges(edges: &[u32]) -> Path {
    let mut path = Path::default();
    path.len = edges.len() as u8;
    path.edges[..edges.len()].copy_from_slice(edges);
    path
}
struct CopyPort {
    src: Source,
    via: Vec<Source>,
    suffix: Option<Path>,
    target: Path,
    table: usize,
}
struct Emitter {
    op: usize,
    table: usize,
    copies: Vec<CopyPort>,
}
pub(super) struct Focus {
    rules: Vec<Vec<Emitter>>,
    sinks: Map<(usize, usize), BTreeSet<Path>>,
    pub matched: usize,
    pub requested: usize,
}
impl Focus {
    pub fn compile(p: &Program, c: Catalog, depth: u8) -> Result<Self> {
        if c.schema != SCHEMA
            || c.paths.len() > 2 * PER_DEPTH
            || c.paths.iter().any(|x| x.len() < 2 || x.len() > MAX_DEPTH)
        {
            return Err("invalid structural port catalog".into());
        }
        let edges = &p.depth_plan.edges;
        let ids: BTreeMap<_, _> = edges
            .iter()
            .enumerate()
            .map(|(i, e)| (name(p, e), i as u32))
            .collect();
        let mut sinks: Map<_, BTreeSet<_>> = Map::default();
        let mut tails = BTreeSet::new();
        let mut matched = 0;
        for names in &c.paths {
            if names.len() > depth as usize {
                continue;
            }
            let Some(ids) = names
                .iter()
                .map(|e| ids.get(e).copied())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if ids
                .windows(2)
                .any(|w| edges[w[0] as usize].2 != edges[w[1] as usize].0)
            {
                return Err("disconnected structural port path".into());
            }
            let pair = (
                edges[ids[0] as usize].0,
                edges[*ids.last().unwrap() as usize].2,
            );
            if sinks.entry(pair).or_default().insert(from_edges(&ids)) {
                matched += 1;
            }
            for start in 0..ids.len() {
                tails.insert(from_edges(&ids[start..]));
            }
        }
        let mut rules = vec![];
        for r in &p.depth_plan.recipes {
            let mut emitters = vec![];
            for o in &r.outputs {
                let mut copies = vec![];
                for &target in &tails {
                    let route = &target.edges[..target.len as usize];
                    for d in &o.donors {
                        let prefix = &d.path.edges[..d.path.len as usize];
                        if !route.starts_with(prefix) {
                            continue;
                        }
                        let rest = &route[prefix.len()..];
                        copies.push(CopyPort {
                            src: d.src,
                            via: d.via.clone(),
                            suffix: (!rest.is_empty()).then(|| from_edges(rest)),
                            target,
                            table: edges[*route.last().unwrap() as usize].2,
                        });
                    }
                }
                if !copies.is_empty() {
                    emitters.push(Emitter {
                        op: o.op,
                        table: o.table,
                        copies,
                    });
                }
            }
            rules.push(emitters);
        }
        Ok(Self {
            rules,
            sinks,
            matched,
            requested: c.paths.len(),
        })
    }
    pub fn has_rule(&self, r: usize) -> bool {
        !self.rules[r].is_empty()
    }
    pub fn capture<'a>(
        &self,
        g: &Graph,
        r: usize,
        known: &QueryFacts,
        effects: &[Option<Dependency>],
        read: impl Fn(Dependency) -> Option<&'a Packet>,
        mut emit: impl FnMut(Dependency, usize, Packet),
    ) {
        let receipt = |s| Depth::source(g, r, s, known, effects);
        for out in &self.rules[r] {
            let Some(anchor) = receipt(Source::Effect(out.op)) else {
                continue;
            };
            let mut packet = Packet::default();
            for c in &out.copies {
                let Some(src) = receipt(c.src) else { continue };
                if !c.via.iter().all(|s| receipt(*s).is_some()) {
                    continue;
                }
                let mut add = |w| {
                    if w != anchor && !packet.ports.iter().any(|x| x.path == c.target && x.w == w) {
                        packet.ports.push(Port {
                            path: c.target,
                            w,
                            t: c.table,
                        });
                    }
                    packet.ports.len() == BUDGET
                };
                if let Some(suffix) = c.suffix {
                    if let Some(prev) = read(src)
                        && prev
                            .ports
                            .iter()
                            .filter(|p| p.path == suffix)
                            .any(|p| add(p.w))
                    {
                        break;
                    }
                } else if add(src) {
                    break;
                }
            }
            emit(anchor, out.table, packet);
        }
    }
    pub fn ports(&self, from: usize, to: usize) -> Option<&BTreeSet<Path>> {
        self.sinks.get(&(from, to))
    }
}
