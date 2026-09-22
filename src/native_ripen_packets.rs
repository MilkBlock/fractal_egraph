//! Adapter from committed native writes/unions to flat facts, before trace drain.
use super::*;
use crate::{
    ripen_convergence::Packets,
    saturated_rule_composition::{Row, SaturatedRuleComposition as State, Vertex},
};
use std::collections::HashMap;
pub(super) struct Tracker {
    pub graph: Packets,
    pub packets: Vec<crate::packet_library::Packet>,
    ids: HashMap<(String, Value), usize>,
    equality: String,
}
impl Tracker {
    pub fn new(
        eg: &EGraph,
        c: &Captured,
        s: &State,
        ports: &[(String, egglog::ArcSort, Value)],
    ) -> Result<Self> {
        if !s.subsumed_rows.is_empty() || s.scope["semantics"] != "positive-constructor-equality/v1"
        {
            return Err("packet fast path requires positive constructor/union effects; tables or visibility use snapshot fallback".into());
        }
        let mut t = Self {
            graph: Packets::new(s),
            packets:vec![],
            ids: HashMap::new(),
            equality: c.datatype_name.clone(),
        };
        let by_id: BTreeMap<_, _> = s
            .local_ids
            .iter()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        let mut parser = EGraph::default();
        for cmd in parser.parse_program(None, &c.datatype)? {
            if let Command::Datatype { variants, .. } = cmd {
                for v in variants {
                    let mut raw = vec![];
                    eg.function_for_each(&v.name, |r| raw.push(r.vals.to_vec()))?;
                    let sorts: Vec<_> = v
                        .types
                        .iter()
                        .chain(std::iter::once(&t.equality))
                        .cloned()
                        .collect();
                    for row in raw {
                        for (ty, value) in sorts.iter().zip(row) {
                            let sort = eg.get_arcsort_by(|s| s.name() == ty);
                            let value = eg.get_canonical_value(value, &sort);
                            let key = eg.value_to_class_id(&sort, value).to_string();
                            t.ids.insert(
                                (ty.clone(), value),
                                *by_id.get(key.as_str()).ok_or("missing seed value")?,
                            );
                        }
                    }
                }
            }
        }
        for (_, sort, v) in ports {
            let value = eg.get_canonical_value(*v, sort);
            let key = eg.value_to_class_id(sort, value).to_string();
            t.ids.insert(
                (sort.name().into(), value),
                *by_id.get(key.as_str()).ok_or("missing seed port")?,
            );
        }
        Ok(t)
    }
    fn value(&mut self, eg: &EGraph, ty: &str, v: Value) -> Result<usize> {
        let key = (ty.to_string(), v);
        if let Some(&i) = self.ids.get(&key) {
            return Ok(i);
        }
        let sort = eg.get_arcsort_by(|s| s.name() == ty);
        let canonical = eg.get_canonical_value(v, &sort);
        if let Some(&i) = self.ids.get(&(ty.to_string(), canonical)) {
            self.ids.insert(key, i);
            return Ok(i);
        }
        if ty != self.equality {
            return Err("new primitive literal requires snapshot reseed".into());
        }
        let i = self.graph.vertex(Vertex {
            sort: ty.into(),
            literal: None,
        });
        self.ids.insert(key, i);
        self.ids.insert((ty.into(), canonical), i);
        Ok(i)
    }
    pub fn apply(&mut self, eg: &EGraph, trace: &TraceSession) -> Result<usize> {
        let before = self.graph.changes;
        use crate::packet_library::{Effect,Packet};
        let rules:HashMap<_,_>=trace.matches().into_iter().map(|m|(m.event_id,m.rule.to_string())).collect();
        let mut packets:BTreeMap<u64,Vec<(u64,Effect)>>=BTreeMap::new();
        let eq = self.equality.clone();
        let unions = trace.union_events();
        for u in &unions {
            if u.displaced.is_some() {
                let a = self.value(eg, &eq, u.lhs)?;
                let b = self.value(eg, &eq, u.rhs)?;
                packets.entry(u.match_event_id).or_default().push((u.event_id,Effect::Equate(a,b)));
                self.graph.union(a, b);
                let canonical = self.value(eg, &eq, u.canonical)?;
                self.graph.union(a, canonical);
            }
        }
        let names: HashMap<_, _> = trace.table_names().into_iter().collect();
        for w in trace.write_events() {
            let Some(name) = names.get(&w.table) else {
                if unions.iter().any(|u| u.table == w.table) {
                    continue;
                }
                return Err("unmapped committed write table".into());
            };
            let Some(f) = eg.get_function(name) else {
                if unions.iter().any(|u| u.table == w.table) {
                    continue;
                }
                return Err("unknown native write schema".into());
            };
            if matches!(w.outcome, egglog::WriteOutcome::Unsupported) {
                return Err("unsupported write".into());
            }
            let schema = f.schema();
            let types: Vec<String> = schema
                .input
                .iter()
                .chain(std::iter::once(&schema.output))
                .map(|s| s.name().to_string())
                .collect();
            if w.actual.len() < types.len() {
                return Err("incomplete committed row".into());
            }
            let mut values = vec![];
            for (ty, v) in types.iter().zip(&w.actual) {
                values.push(self.value(eg, ty, *v)?);
            }
            let result = values.pop().ok_or("empty write")?;
            packets.entry(w.match_event_id).or_default().push((w.event_id,Effect::Ensure{op:name.to_string(),args:values.clone(),result}));
            self.graph.ensure(Row {
                op: name.to_string(),
                args: values,
                result,
            });
        }
        for (id,mut effects) in packets {
            effects.sort_by_key(|(event,_)|*event);
            self.packets.push(Packet::normalize(rules.get(&id).cloned().unwrap_or_else(||"native-rebuild".into()),effects.into_iter().map(|(_,e)|e).collect()));
        }
        Ok(self.graph.changes - before)
    }
}
