//! Statistical reentry groups and minimal *observed abstract* starting conditions.
//! Cover uses native joins, never a similarity score or a termination claim.
use crate::{pipeline::Result, saturated_rule_composition::SaturatedRuleComposition as State};
use egglog::{EGraph, ast::Literal};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
fn symbol(prefix: &str, text: &str) -> String {
    format!(
        "{prefix}{}",
        text.as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
struct Condition {
    pattern: State,
    used: Vec<usize>,
    eg: EGraph,
    tables: BTreeSet<String>,
    links: BTreeMap<(usize, usize), Value>,
}
impl Condition {
    fn new(mut pattern: State, marker: &str) -> Result<Self> {
        let hidden: BTreeSet<_> = pattern.subsumed_rows.iter().copied().collect();
        pattern.rows = pattern
            .rows
            .into_iter()
            .enumerate()
            .filter(|(i, r)| !hidden.contains(i) && r.op != marker)
            .map(|(_, r)| r)
            .collect();
        pattern.subsumed_rows.clear();
        pattern.local_ids.clear();
        let used: Vec<_> = pattern
            .rows
            .iter()
            .flat_map(|r| r.args.iter().copied().chain(std::iter::once(r.result)))
            .chain(pattern.ports.values().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for v in &mut pattern.values {
            v.literal = None;
        }
        let mut eg = EGraph::default();
        let mut tables = BTreeSet::new();
        let mut declarations = BTreeMap::new();
        let mut facts = vec![];
        for &v in &used {
            let op = symbol("Type_", &pattern.values[v].sort);
            declarations.insert(op.clone(), 1);
            facts.push(format!("({op} {v})"));
        }
        for r in &pattern.rows {
            let op = symbol("Fact_", &format!("{}:{}", r.op, r.args.len()));
            declarations.insert(op.clone(), r.args.len() + 1);
            facts.push(format!(
                "({op}{})",
                r.args
                    .iter()
                    .chain(std::iter::once(&r.result))
                    .map(|v| format!(" {v}"))
                    .collect::<String>()
            ));
        }
        let mut program = String::new();
        for (op, n) in declarations {
            program += &format!("(relation {op} ({}))\n", vec!["i64"; n].join(" "));
            tables.insert(op);
        }
        program += "(relation NamedPort (String i64))\n";
        tables.insert("NamedPort".into());
        for (name, v) in &pattern.ports {
            facts.push(format!("(NamedPort {} {v})", serde_json::to_string(name)?));
        }
        program += &facts.join("\n");
        eg.parse_and_run_program(None, &program)?;
        Ok(Self {
            pattern,
            used,
            eg,
            tables,
            links: BTreeMap::new(),
        })
    }
    /// Does the stronger condition contain an instantiation of the weaker one?
    fn cover(&mut self, small: &Self) -> Result<Option<Value>> {
        let mut values = vec![("i64".to_string(), None); small.pattern.values.len()];
        let mut atoms = vec![];
        for &v in &small.used {
            atoms.push((symbol("Type_", &small.pattern.values[v].sort), vec![v]));
        }
        for r in &small.pattern.rows {
            atoms.push((
                symbol("Fact_", &format!("{}:{}", r.op, r.args.len())),
                r.args
                    .iter()
                    .copied()
                    .chain(std::iter::once(r.result))
                    .collect(),
            ));
        }
        for (name, v) in &small.pattern.ports {
            let i = values.len();
            values.push(("String".into(), Some(Literal::String(name.clone()))));
            atoms.push(("NamedPort".into(), vec![i, *v]));
        }
        if atoms.iter().any(|(op, _)| !self.tables.contains(op)) {
            return Ok(None);
        }
        let query = self.eg.compile_fact_query(&values, &atoms, &small.used)?;
        // Cover needs one mapping, not every automorphism of the condition.
        let matches = self
            .eg
            .run_fact_query_difference(&query, Default::default());
        self.eg.free_fact_query(query);
        Ok(matches?.0.as_ref().map(|row| {
            json!(
                small
                    .used
                    .iter()
                    .zip(row)
                    .map(|(a, b)| json!([a, self.eg.value_to_base::<i64>(*b)]))
                    .collect::<Vec<_>>()
            )
        }))
    }
}
#[derive(Default)]
pub struct Buckets {
    conditions: Vec<Condition>,
    groups: Vec<Group>,
    keys: HashMap<String, usize>,
    group_keys: HashMap<String, usize>,
    original: BTreeMap<usize, State>,
    comparisons: usize,
}
struct Group {
    operators: Vec<String>,
    union_effect: bool,
    ids: Vec<usize>,
    covers: Vec<Value>,
}
impl Buckets {
    pub fn observe(
        &mut self,
        source: usize,
        target: usize,
        round: u64,
        trigger: &State,
        marker: &str,
        components: &BTreeMap<String, u64>,
        union_effect: bool,
        evidence: &Value,
    ) -> Result {
        let growth = evidence["admission"] == "growth_observation"
            && evidence["unfinished"] == true
            && evidence["growth"]["after_facts"].as_u64().unwrap_or(0)
                > evidence["growth"]["before_facts"]
                    .as_u64()
                    .unwrap_or(u64::MAX)
            && evidence["growth"]["reasons"]
                .as_array()
                .is_some_and(|r| !r.is_empty());
        let fresh = evidence["new_effects"] == true
            && evidence["new_bindings"]
                .as_array()
                .is_some_and(|r| !r.is_empty());
        if !growth && !fresh {
            return Ok(());
        }
        let rows: Vec<_> = trigger
            .rows
            .iter()
            .enumerate()
            .filter(|(i, r)| r.op != marker && !trigger.subsumed_rows.contains(i))
            .map(|(_, r)| r.clone())
            .collect();
        if rows.is_empty() {
            return Ok(());
        }
        let operators: Vec<_> = components
            .keys()
            .filter(|op| op.as_str() != marker)
            .cloned()
            .collect();
        let group_key = serde_json::to_string(&(&trigger.scope, &operators, union_effect))?;
        let group = if let Some(i) = self.group_keys.get(&group_key) {
            *i
        } else {
            let i = self.groups.len();
            self.groups.push(Group {
                operators,
                union_effect,
                ids: vec![],
                covers: vec![],
            });
            self.group_keys.insert(group_key.clone(), i);
            i
        };
        // Literal payload is erased for bucketing only; original conditions remain intact.
        let rawkey = serde_json::to_string(&(
            &group_key,
            rows,
            trigger.values.iter().map(|v| &v.sort).collect::<Vec<_>>(),
            &trigger.ports,
        ))?;
        let id = if let Some(id) = self.keys.get(&rawkey) {
            *id
        } else {
            let mut fresh = Condition::new(trigger.clone(), marker)?;
            let mut relations = vec![];
            let mut equivalent = None;
            for &old in &self.groups[group].ids {
                self.comparisons += 2;
                let forward = fresh.cover(&self.conditions[old])?;
                let reverse = self.conditions[old].cover(&fresh)?;
                if forward.is_some() && reverse.is_some() {
                    equivalent = Some(old);
                    break;
                }
                relations.push((old, forward, reverse));
            }
            let id = if let Some(old) = equivalent {
                old
            } else {
                let id = self.conditions.len();
                self.conditions.push(fresh);
                self.groups[group].ids.push(id);
                for (old, forward, reverse) in relations {
                    if let Some(map) = forward {
                        self.groups[group].covers.push(
                            json!({"stronger":id,"weaker":old,"mapping_weaker_to_stronger":map}),
                        );
                    }
                    if let Some(map) = reverse {
                        self.groups[group].covers.push(
                            json!({"stronger":old,"weaker":id,"mapping_weaker_to_stronger":map}),
                        );
                    }
                }
                id
            };
            self.keys.insert(rawkey, id);
            id
        };
        self.original
            .entry(target)
            .or_insert_with(|| trigger.clone());
        let prior_growth = self.conditions[id]
            .links
            .get(&(source, target))
            .map(|r| r["growth"].clone())
            .unwrap_or(Value::Null);
        let growth_evidence = if growth {
            evidence["growth"].clone()
        } else {
            prior_growth
        };
        self.conditions[id].links.insert((source,target),json!({"source":source,"target":target,"round":round,"components":components,"union_effect":union_effect,"admission":if !growth_evidence.is_null(){"growth_observation"}else{"fresh_binding"},"starting_condition_resolved":false,"growth":growth_evidence,"new_bindings":evidence["new_bindings"],"scope":evidence["scope"]}));
        Ok(())
    }
    pub fn known_targets(&self) -> BTreeSet<usize> {
        self.original.keys().copied().collect()
    }
    pub fn report(&self) -> Value {
        let groups:Vec<_>=self.groups.iter().enumerate().map(|(id,g)|{
   let stronger:BTreeSet<_>=g.covers.iter().filter_map(|e|e["stronger"].as_u64()).collect();
   json!({"id":id,"new_node_operators":g.operators,"union_effect":g.union_effect,"conditions":g.ids,"minimal_starting_conditions":g.ids.iter().filter(|i|!stronger.contains(&(**i as u64))).collect::<Vec<_>>(),"cover":g.covers})
  }).collect();
        json!({"schema":"reentry-buckets/v1","scope":"observed non-trivial reentry suspicion only; grouping by new-effect operator support; typed literals erased, alias and named-port constraints retained; minimal among observed abstract conditions, not proven loops or infi","comparisons":self.comparisons,"groups":groups,"conditions":self.conditions.iter().enumerate().map(|(id,c)|json!({"id":id,"abstract_condition":c.pattern,"observations":c.links.values().collect::<Vec<_>>()})).collect::<Vec<_>>(),"original_conditions":self.original})
    }
}
