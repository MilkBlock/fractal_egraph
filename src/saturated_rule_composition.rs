//! Exact, bounded isomorphism of complete exported local states and row visibility.
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
    UnknownBudget {
        states: usize,
    },
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

/// Compare two exported local states under a finite search budget.
///
/// The inexpensive key and colour-refinement passes reject obvious mismatches;
/// the final backtracking search is still required because refinement is only a
/// necessary condition for graph isomorphism. `UnknownBudget` is therefore a
/// real result: exhausting the checker does not mean the states differ.
pub fn compare(a: &SaturatedRuleComposition, b: &SaturatedRuleComposition, budget: usize) -> Result<Comparison> {
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
    if a.values.len() > 512 || a.rows.len() > 4096 {
        return Ok(Comparison::UnknownBudget { states: 0 });
    }
    let (ca, cb) = colors(a, b);
    let mut ha = ca.clone();
    ha.sort();
    let mut hb = cb.clone();
    hb.sort();
    if ha != hb {
        return Ok(different("refined incidence classes differ", 0));
    }
    let candidates: Vec<Vec<_>> = (0..a.values.len())
        .map(|i| (0..b.values.len()).filter(|j| ca[i] == cb[*j]).collect())
        .collect();
    let mut map = vec![None; a.values.len()];
    let mut used = vec![false; b.values.len()];
    let rows: BTreeMap<_, _> = b
        .rows
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, r)| (r, b.subsumed_rows.contains(&i)))
        .collect();
    let mut states = 0;
    let mut exhausted = false;
    fn search(
        a: &SaturatedRuleComposition,
        rows: &BTreeMap<Row, bool>,
        cs: &[Vec<usize>],
        map: &mut [Option<usize>],
        used: &mut [bool],
        states: &mut usize,
        budget: usize,
        exhausted: &mut bool,
    ) -> bool {
        let next = (0..map.len())
            .filter(|i| map[*i].is_none())
            .min_by_key(|i| cs[*i].iter().filter(|j| !used[**j]).count());
        let Some(i) = next else {
            return a.rows.iter().enumerate().all(|(row_id, r)| {
                rows.get(&Row {
                    op: r.op.clone(),
                    args: r.args.iter().map(|x| map[*x].unwrap()).collect(),
                    result: map[r.result].unwrap(),
                }) == Some(&a.subsumed_rows.contains(&row_id))
            });
        };
        for &j in &cs[i] {
            if used[j] {
                continue;
            }
            if *states >= budget {
                *exhausted = true;
                return false;
            }
            *states += 1;
            map[i] = Some(j);
            used[j] = true;
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
            if valid && search(a, rows, cs, map, used, states, budget, exhausted) {
                return true;
            }
            map[i] = None;
            used[j] = false;
            if *exhausted {
                return false;
            }
        }
        false
    }
    if search(
        a,
        &rows,
        &candidates,
        &mut map,
        &mut used,
        &mut states,
        budget,
        &mut exhausted,
    ) {
        Ok(Comparison::Equivalent {
            value_map: map.into_iter().map(Option::unwrap).collect(),
            states,
        })
    } else if exhausted {
        Ok(Comparison::UnknownBudget { states })
    } else {
        Ok(different("no complete typed fact/port bijection", states))
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
pub fn catalog(inputs: &[std::path::PathBuf], out: &Path, budget: usize) -> Result<Value> {
    if inputs.is_empty() || out.exists() {
        return Err("catalog needs input ripen directories and a new output directory".into());
    }
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
    let mut states: Vec<SaturatedRuleComposition> = vec![];
    let mut buckets = BTreeMap::<String, Vec<usize>>::new();
    let mut triggers = vec![];
    let mut comparisons = vec![];
    for (path, report, state) in entries {
        let source_value_ids = state.local_ids.clone();
        let key = state.key();
        let mut hit = None;
        for &i in buckets.get(&key).into_iter().flatten() {
            let result = compare(&state, &states[i], budget)?;
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
        std::fs::write(
            out.join(format!("states/state-{i:04}.json")),
            serde_json::to_vec_pretty(state)?,
        )?;
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
    let report = json!({"schema":"saturated-rule-composition-catalog/v1","scope":"exact complete constructor-state sharing with fixed named ports; triggers remain separate; no tier0 substitution", "unresolved_comparisons":comparisons.iter().filter(|c|c["result"]["status"]=="unknown_budget").count(),"saturated_rule_compositions":states.len(),"state_groups":state_groups,"comb_groups":comb_groups,"triggers":triggers,"comparisons":comparisons});
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
    std::fs::write(out.join("catalog.dot"), dot)?;
    std::fs::write(
        out.join("catalog.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
