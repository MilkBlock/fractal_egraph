//! Profile selection proposes entries. Loading independently reconstructs their
//! exact dataflow from the target query and its complete stage entry.
use crate::{
    graph::{Graph, Row},
    program::*,
    value::{Value, canon},
};
use serde::Serialize;
use serde_json::{Value as J, json};
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Debug, Serialize)]
pub(super) enum Field {
    Anchor(usize),
    PortResult,
    Literal(Value),
}
impl Field {
    pub(super) fn get(&self, g: &Graph, row: &Row, port: Option<&Row>) -> Value {
        match self {
            Self::Anchor(col) => row.values[*col].clone(),
            Self::PortResult => port.expect("validated port").values.last().unwrap().clone(),
            Self::Literal(v) => canon(v, &g.parents),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Port {
    pub atom: usize,
    pub table: usize,
    pub key: Vec<Field>,
    pub check_result: Option<Field>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Block {
    pub port: Option<Port>,
    pub inputs: Vec<Field>,
}
impl Block {
    pub(crate) fn lower(p: &Program, r: usize, pivot: usize) -> Option<Self> {
        let (_, _, order) = p
            .multi_stage
            .stage_entries
            .iter()
            .find(|(i, a, _)| (*i, *a) == (r, pivot))?;
        Self::lower_entry(&p.rules[r], pivot, order)
    }
    fn lower_entry(rule: &Rule, pivot: usize, order: &[usize]) -> Option<Self> {
        let mut sources = BTreeMap::new();
        for (col, arg) in rule.query[pivot].args.iter().enumerate() {
            if let Arg::Slot(s) = arg {
                sources.entry(*s).or_insert(Field::Anchor(col));
            }
        }
        let field = |a: &Arg, map: &BTreeMap<usize, Field>| match a {
            Arg::Slot(s) => map.get(s).cloned(),
            Arg::Literal(v) => Some(Field::Literal(v.clone())),
        };
        let port = match order {
            [] => None,
            &[atom] => {
                let a = &rule.query[atom];
                let Call::Table(table) = a.call else {
                    return None;
                };
                let (out, keys) = a.args.split_last()?;
                let key = keys
                    .iter()
                    .map(|a| field(a, &sources))
                    .collect::<Option<_>>()?;
                let check_result = field(out, &sources);
                if let Arg::Slot(s) = out {
                    sources.entry(*s).or_insert(Field::PortResult);
                }
                Some(Port {
                    atom,
                    table,
                    key,
                    check_result,
                })
            }
            _ => return None,
        };
        Some(Self {
            port,
            inputs: rule
                .inputs
                .iter()
                .map(|s| sources.get(s).cloned())
                .collect::<Option<_>>()?,
        })
    }
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    entries: Vec<Vec<Option<Rc<Block>>>>,
}
impl Catalog {
    pub(super) fn get(&self, r: usize, pivot: usize) -> Option<Rc<Block>> {
        self.entries.get(r)?.get(pivot)?.clone()
    }
    pub(crate) fn parse(p: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            return Ok(Self::default());
        }
        if raw["schema"] != "eggfuse-spine-blocks/v1"
            || raw["recipe_sha256"] != p.artifact["recipe"]["sha256"]
        {
            return Err("spine block recipe mismatch".into());
        }
        let mut entries: Vec<Vec<_>> = p.rules.iter().map(|r| vec![None; r.query.len()]).collect();
        for e in raw["entries"].as_array().ok_or("missing spine blocks")? {
            let r = e["consumer"].as_u64().ok_or("block consumer")? as usize;
            let a = e["pivot"].as_u64().ok_or("block pivot")? as usize;
            let block = Block::lower(p, r, a).ok_or("uncertified spine entry")?;
            if e["rhs_template"] != p.rules[r].template
                || e["block"] != json!(block)
                || entries[r][a].replace(Rc::new(block)).is_some()
            {
                return Err("invalid spine block wiring or duplicate entry".into());
            }
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyed_port_preserves_bound_result_constraints_and_slot_order() {
        use Arg::{Literal, Slot};
        let rule = Rule {
            constructor_only: false,
            template: 3,
            seminaive: true,
            direct: None,
            inputs: Rc::new(vec![1, 0]),
            query: Rc::new(vec![
                Atom {
                    call: Call::Table(1),
                    args: vec![Slot(0), Slot(1), Slot(0)],
                },
                Atom {
                    call: Call::Table(2),
                    args: vec![Slot(1), Literal(Value::Int(7)), Slot(0)],
                },
            ]),
        };
        let b = Block::lower_entry(&rule, 0, &[1]).unwrap();
        let p = b.port.unwrap();
        assert_eq!(json!(p.key), json!([{"Anchor":1},{"Literal":{"Int":7}}]));
        assert_eq!(json!(p.check_result), json!({"Anchor":0}));
        assert_eq!(json!(b.inputs), json!([{"Anchor":1},{"Anchor":0}]));
        assert!(Block::lower_entry(&rule, 0, &[1, 1]).is_none());
        let mut rule = rule;
        Rc::make_mut(&mut rule.query)[1].args[0] = Slot(9);
        assert!(Block::lower_entry(&rule, 0, &[1]).is_none());
    }
}
