//! Exact isomorphism of complete exported local states and row visibility.
//! Cycles and sharing are preserved; matching histories and rule names are absent.
use crate::pipeline::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Vertex {
    pub sort: String,
    pub literal: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Row {
    pub op: String,
    pub args: Vec<usize>,
    pub result: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
/// A finite, exported local replay result.
///
/// This is deliberately a *state* record, not a rule macro: it contains the
/// rows and equality-shaped value graph observed after replaying one trigger
/// contract. The trigger, staged injections, and original history live beside
/// this value in the catalog. Keeping those objects separate is what prevents
/// two equal local states from being treated as interchangeable executions.
pub struct SaturatedRuleComposition {
    pub version: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_ids: Vec<String>,
    pub scope: Value,
    pub values: Vec<Vertex>,
    pub rows: Vec<Row>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subsumed_rows: Vec<usize>,
    /// Fixed named global ports; equal ports may point at the same e-class.
    pub ports: BTreeMap<String, usize>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Comparison {
    Equivalent {
        value_map: Vec<usize>,
        states: usize,
    },
    Different {
        reason: String,
        states: usize,
    },
    IncompatibleScope,
}
impl SaturatedRuleComposition {
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.local_ids.is_empty() && self.local_ids.len() != self.values.len() {
            return Err("invalid provenance ID count".into());
        }
        if !matches!(self.version, 1 | 2) || (self.version == 1 && !self.subsumed_rows.is_empty()) {
            return Err("unsupported saturated-rule-composition version".into());
        }
        if self.subsumed_rows.iter().any(|i| *i >= self.rows.len())
            || self.subsumed_rows.iter().collect::<BTreeSet<_>>().len() != self.subsumed_rows.len()
        {
            return Err("invalid visibility mask".into());
        }
        if self.ports.values().any(|i| *i >= self.values.len())
            || self.rows.iter().any(|r| {
                r.result >= self.values.len() || r.args.iter().any(|i| *i >= self.values.len())
            })
        {
            return Err("invalid saturated-rule-composition reference".into());
        }
        if self.rows.iter().collect::<BTreeSet<_>>().len() != self.rows.len() {
            return Err("duplicate fact rows".into());
        }
        Ok(())
    }
    fn labels(&self) -> Vec<String> {
        self.values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                serde_json::to_string(&(
                    v,
                    self.ports
                        .iter()
                        .filter_map(|(p, j)| (*j == i).then_some(p))
                        .collect::<Vec<_>>(),
                ))
                .unwrap()
            })
            .collect()
    }
    fn key(&self) -> String {
        let mut labels = self.labels();
        labels.sort();
        let mut ops: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| (&r.op, r.args.len(), self.subsumed_rows.contains(&i)))
            .collect();
        ops.sort();
        serde_json::to_string(&(&self.scope, labels, ops)).unwrap()
    }
    fn graph(&self) -> (Vec<String>, Vec<Vec<(String, usize)>>) {
        let mut labels = self.labels();
        labels.extend(self.rows.iter().enumerate().map(|(i, r)| {
            format!(
                "fact:{}:{}:{}",
                r.op,
                r.args.len(),
                self.subsumed_rows.contains(&i)
            )
        }));
        let mut edges = vec![vec![]; labels.len()];
        for (i, row) in self.rows.iter().enumerate() {
            let f = self.values.len() + i;
            for (slot, v) in row
                .args
                .iter()
                .chain(std::iter::once(&row.result))
                .enumerate()
            {
                edges[f].push((format!("to:{slot}"), *v));
                edges[*v].push((format!("from:{slot}"), f));
            }
        }
        (labels, edges)
    }
}
fn colors(a: &SaturatedRuleComposition, b: &SaturatedRuleComposition) -> (Vec<usize>, Vec<usize>) {
    let (la, ea) = a.graph();
    let (lb, eb) = b.graph();
    let mut labels = la;
    labels.extend(lb);
    let mut edges = ea;
    let split = edges.len();
    edges.extend(
        eb.into_iter()
            .map(|es| es.into_iter().map(|(r, n)| (r, n + split)).collect()),
    );
    fn number<T: Ord + Clone>(xs: &[T]) -> Vec<usize> {
        let ids: BTreeMap<_, _> = xs
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .enumerate()
            .map(|(i, x)| (x, i))
            .collect();
        xs.iter().map(|x| ids[x]).collect()
    }
    let mut c = number(&labels);
    loop {
        let sig: Vec<_> = edges
            .iter()
            .enumerate()
            .map(|(i, es)| {
                let mut neighbours: Vec<_> = es.iter().map(|(role, j)| (role, c[*j])).collect();
                neighbours.sort();
                (c[i], neighbours)
            })
            .collect();
        let next = number(&sig);
        let done =
            next.iter().collect::<BTreeSet<_>>().len() == c.iter().collect::<BTreeSet<_>>().len();
        c = next;
        if done {
            break;
        }
    }
    (c[..split].to_vec(), c[split..].to_vec())
}

/// Compare exported local states exactly, with no search cutoff.
///
/// The inexpensive key and colour-refinement passes reject obvious mismatches;
/// the final backtracking search is still required because refinement is only a
/// necessary condition for graph isomorphism. The explicit DFS has no cutoff;
/// this whole-state bijection check is distinct from native weak-dominate joins.
pub fn compare(a: &SaturatedRuleComposition, b: &SaturatedRuleComposition) -> Result<Comparison> {
    a.validate()?;
    b.validate()?;
    if a.scope != b.scope {
        return Ok(Comparison::IncompatibleScope);
    }
    let different = |reason: &str, states| Comparison::Different {
        reason: reason.into(),
        states,
    };
    if a.key() != b.key() {
        return Ok(different("typed facts or fixed port inventory differs", 0));
    }
    let (ca, cb) = colors(a, b);
    let mut ha = ca.clone();
    ha.sort();
    let mut hb = cb.clone();
    hb.sort();
    if ha != hb {
        return Ok(different("refined incidence classes differ", 0));
    }
    // One shared domain per refined class, rather than a quadratic copy for
    // every symmetric value. Explicit DFS state avoids a call-stack limit.
    let mut domains = BTreeMap::<usize, Vec<usize>>::new();
    for (i, color) in cb.iter().take(b.values.len()).enumerate() {
        domains.entry(*color).or_default().push(i);
    }
    let mut map = vec![None; a.values.len()];
    let mut used = vec![false; b.values.len()];
    let rows: BTreeMap<_, _> = b
        .rows
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, r)| (r, b.subsumed_rows.contains(&i)))
        .collect();
    let mut next = vec![0; a.values.len()];
    let mut depth = 0;
    let mut states = 0;
    loop {
        if depth == map.len() {
            return Ok(Comparison::Equivalent {
                value_map: map.into_iter().map(Option::unwrap).collect(),
                states,
            });
        }
        let domain = &domains[&ca[depth]];
        let mut choice = None;
        while next[depth] < domain.len() {
            let j = domain[next[depth]];
            next[depth] += 1;
            if !used[j] {
                choice = Some(j);
                break;
            }
        }
        let Some(j) = choice else {
            next[depth] = 0;
            if depth == 0 {
                return Ok(different("no complete typed fact/port bijection", states));
            }
            depth -= 1;
            let previous = map[depth].take().unwrap();
            used[previous] = false;
            continue;
        };
        map[depth] = Some(j);
        used[j] = true;
        states += 1;
        let valid = a.rows.iter().enumerate().all(|(row_id, r)| {
            if let Some(result) = map[r.result] {
                if let Some(args) = r.args.iter().map(|x| map[*x]).collect::<Option<Vec<_>>>() {
                    return rows.get(&Row {
                        op: r.op.clone(),
                        args,
                        result,
                    }) == Some(&a.subsumed_rows.contains(&row_id));
                }
            }
            true
        });
        if valid {
            depth += 1;
        } else {
            map[depth] = None;
            used[j] = false;
        }
    }
}

pub fn read(path: &Path) -> Result<SaturatedRuleComposition> {
    let s: SaturatedRuleComposition = serde_json::from_slice(&std::fs::read(path)?)?;
    s.validate()?;
    Ok(s)
}

/// Group successful replays by exact exported-state equivalence.
///
/// The catalog shares descriptions of equal states while retaining one trigger
/// record per input. It measures conditional symbolic interfaces, not memory
/// saved inside the live egglog matcher.
pub fn catalog(inputs: &[std::path::PathBuf], out: &Path) -> Result<Value> {
    if inputs.is_empty() || out.exists() {
        return Err("catalog needs input ripen directories and a new output directory".into());
    }
    refresh_catalog(inputs, out)
}

// Pipeline inputs are append-only: existing state ids stay stable. Publish each
// complete file atomically and publish catalog.json last, retaining the previous
// catalog if cancellation interrupts construction.
pub(crate) fn refresh_catalog(inputs: &[std::path::PathBuf], out: &Path) -> Result<Value> {
    let read_started = std::time::Instant::now();
    // Validate every input before creating output. Suspended runs have no state.
    let entries: Vec<_> = inputs
        .iter()
        .map(|p| -> Result<_> {
            let report: Value = serde_json::from_slice(&std::fs::read(p.join("ripen.json"))?)?;
            if report["ripen"]["state"] != "Saturated" {
                return Err("catalog input is not Saturated".into());
            }
            if report["saturated_rule_composition"]["status"] != "exported" {
                return Err(
                    format!("SaturatedRuleComposition was not exported: {}", report["saturated_rule_composition"]).into(),
                );
            }
            let report=json!({"ripen":report["ripen"],"source_text":report["source_text"],"origin":report["origin"]});
            Ok((p, report, read(&p.join("saturated-rule-composition.json"))?))
        })
        .collect::<Result<_>>()?;
    let read_seconds = read_started.elapsed().as_secs_f64();
    let mut compare_seconds = 0.;
    let mut key_seconds = 0.;
    let mut states: Vec<SaturatedRuleComposition> = vec![];
    let mut buckets = BTreeMap::<String, Vec<usize>>::new();
    let mut triggers = vec![];
    let mut comparisons = vec![];
    for (path, report, state) in entries {
        let source_value_ids = state.local_ids.clone();
        let stage = std::time::Instant::now();
        let key = state.key();
        key_seconds += stage.elapsed().as_secs_f64();
        let mut hit = None;
        for &i in buckets.get(&key).into_iter().flatten() {
            let stage = std::time::Instant::now();
            let result = compare(&state, &states[i])?;
            compare_seconds += stage.elapsed().as_secs_f64();
            if let Comparison::Equivalent { value_map, .. } = &result {
                hit = Some((i, value_map.clone()));
            }
            comparisons.push(json!({"trigger":triggers.len(),"state":i,"result":result}));
            if hit.is_some() {
                break;
            }
        }
        let (id, mapping) = hit.unwrap_or_else(|| {
            let id = states.len();
            let identity = (0..state.values.len()).collect();
            states.push(state);
            buckets.entry(key).or_default().push(id);
            (id, identity)
        });
        triggers.push(json!({"source":path.canonicalize()?,"entry":report["source_text"],"origin":report["ripen"]["origin"],"binding_origin":report["origin"],"saturated_rule_composition":id,"value_map":mapping,"source_value_ids":source_value_ids}));
    }
    std::fs::create_dir_all(out.join("states"))?;
    for (i, state) in states.iter().enumerate() {
        crate::pipeline::write_json(&out.join(format!("states/state-{i:04}.json")), state)?;
    }
    let mut group_keys = BTreeMap::new();
    let mut comb_groups: Vec<Value> = vec![];
    for (i, t) in triggers.iter().enumerate() {
        let members = t["binding_origin"]["comb_members"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let signature: Vec<_> = members
            .iter()
            .map(|m| {
                let mut parents = m["parents"].as_array().cloned().unwrap_or_default();
                parents.sort_by_key(|p| p.as_u64());
                json!([m["rule"], m["use_kind"], parents])
            })
            .collect();
        let key = serde_json::to_string(&json!([
            t["saturated_rule_composition"],
            t["origin"]["history"],
            t["origin"]["template"],
            signature,
            if members.is_empty() {
                json!(i)
            } else {
                Value::Null
            }
        ]))?;
        let n = comb_groups.len();
        let g=*group_keys.entry(key).or_insert_with(||{
            comb_groups.push(json!({"id":n,"saturated_rule_composition":t["saturated_rule_composition"],"template":t["origin"]["template"],"history":t["origin"]["history"],
                "members":members,"root_member":t["binding_origin"]["root_member"],"triggers":[]}));n
        });
        comb_groups[g]["triggers"]
            .as_array_mut()
            .unwrap()
            .push(json!(i));
    }
    let state_groups:Vec<_>=(0..states.len()).map(|id|{
        let members:Vec<_>=triggers.iter().enumerate().filter(|(_,t)|t["saturated_rule_composition"]==id).collect();
        let templates:BTreeSet<_>=members.iter().filter(|(_,t)|t["origin"]["template"].is_number()).map(|(_,t)|json!([t["origin"]["history"],t["origin"]["template"]]).to_string()).collect();
        json!({"saturated_rule_composition":id,"trigger_count":members.len(),"template_count":templates.len(),
            "triggers":members.iter().map(|(i,_)|*i).collect::<Vec<_>>(),
            "comb_groups":comb_groups.iter().filter(|g|g["saturated_rule_composition"]==id).map(|g|g["id"].clone()).collect::<Vec<_>>()})
    }).collect();
    let report = json!({"timings":{"read_seconds":read_seconds,"key_seconds":key_seconds,"compare_seconds":compare_seconds},"schema":"saturated-rule-composition-catalog/v1","scope":"exact complete constructor-state sharing with fixed named ports; triggers remain separate; no tier0 substitution", "unresolved_comparisons":0,"saturated_rule_compositions":states.len(),"state_groups":state_groups,"comb_groups":comb_groups,"triggers":triggers,"comparisons":comparisons});
    if crate::graph_render::graphs_enabled() {
        let mut dot = String::from("digraph ClosedCatalog { rankdir=LR; node [shape=box];\n");
        for (i, state) in states.iter().enumerate() {
            let label = format!(
                "SaturatedRuleComposition C{i}\n{} values / {} facts",
                state.values.len(),
                state.rows.len()
            );
            dot += &format!("c{i} [label={}];\n", serde_json::to_string(&label)?);
        }
        for (i, t) in report["triggers"].as_array().unwrap().iter().enumerate() {
            let label = if t["origin"]["use_id"].is_number() {
                format!(
                    "Trigger {i}\nU{} / T{}",
                    t["origin"]["use_id"], t["origin"]["template"]
                )
            } else {
                format!("Trigger {i}")
            };
            dot += &format!(
                "t{i} [label={}];\nt{i} -> c{} [label=\"exact value mapping\"];\n",
                serde_json::to_string(&label)?,
                t["saturated_rule_composition"]
            );
        }
        dot += "}\n";
        std::fs::write(out.join("catalog.dot.partial"), dot)?;
        std::fs::rename(out.join("catalog.dot.partial"), out.join("catalog.dot"))?;
    }
    crate::pipeline::write_json(&out.join("catalog.json"), &report)?;
    Ok(report)
}
