//! Explicit native DAG extension for generated source. Registers primitives only:
//! rules, choices, anchoring and maintenance schedules remain in the .egg file.
use super::key::*;
use crate::Result;
use egglog::{EGraph, Value};
use serde_json::{Value as Json, json};
use std::collections::HashMap;
use std::{collections::BTreeSet, sync::Arc};

pub const HEADER: &str = "; cell-structural-key-runtime/v1 ";

pub fn install(eg: &mut EGraph, text: &str) -> Result<Option<Json>> {
    let headers = text
        .lines()
        .filter_map(|l| l.strip_prefix(HEADER))
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Ok(None);
    }
    if headers.len() != 1 {
        return Err("duplicate structural-key runtime declaration".into());
    }
    let spec: Json = serde_json::from_str(headers[0])?;
    let nodes = spec["nodes"]
        .as_array()
        .ok_or("key runtime declaration needs nodes")?;
    let bases = spec["bases"]
        .as_array()
        .ok_or("key runtime declaration needs bases")?;
    if eg.get_sort_by_name(SORT).is_some() {
        return Err("structural-key sort already installed".into());
    }
    super::key::install_base(eg)?;
    let key = eg.get_sort_by_name(SORT).unwrap().clone();
    let int = eg.get_sort_by_name("i64").unwrap().clone();
    let boolean = eg.get_sort_by_name("bool").unwrap().clone();
    eg.add_pure_primitive(
        Prim {
            name: "cell-key-prefer-left".into(),
            types: vec![int.clone(), int, key.clone(), key.clone(), boolean],
            op: Op::PreferLeft,
        },
        None,
    );
    let mut names = BTreeSet::new();
    for base in bases {
        let name = base.as_str().ok_or("invalid key literal sort")?;
        if !["i64", "f64", "String", "bool", "Unit"].contains(&name)
            || !names.insert(name.to_owned())
        {
            return Err("unsupported or duplicate structural-key literal sort".into());
        }
        let input = eg.get_sort_by_name(name).unwrap().clone();
        eg.add_pure_primitive(
            Prim {
                name: format!("cell-key-base-{name}"),
                types: vec![input, key.clone()],
                op: Op::Literal(name.into()),
            },
            None,
        );
    }
    names.clear();
    for n in nodes {
        let name = n["primitive"]
            .as_str()
            .ok_or("missing key primitive name")?;
        let label = n["label"].as_str().ok_or("missing typed key label")?;
        let arity = usize::try_from(n["arity"].as_u64().ok_or("missing key arity")?)?;
        if !name.starts_with("cell-key-node-") || !names.insert(name.to_owned()) {
            return Err("invalid or duplicate key node primitive".into());
        }
        eg.add_pure_primitive(
            Prim {
                name: name.into(),
                types: vec![key.clone(); arity.checked_add(1).ok_or("key arity overflow")?],
                op: Op::Node(label.into()),
            },
            None,
        );
    }
    Ok(Some(
        json!({"name":"cell-structural-key-runtime/v1","nodes":nodes.len(),"bases":bases,
        "storage":"shared immutable Arc DAG; cached hash with exact structural equality",
        "source_controls_rules_and_schedule":true}),
    ))
}

/// Export a shared, topologically ordered DAG, never one expanded tree per row.
/// Pointer addresses are used only for local traversal, not as persisted identity.
#[derive(Default)]
pub struct KeyGraph {
    ids: HashMap<usize, usize>,
    pub nodes: Vec<Json>,
}
impl KeyGraph {
    pub fn intern(&mut self, eg: &EGraph, value: Value) -> usize {
        let root = (*eg.value_to_base::<Stored>(value)).clone();
        let address = |k: &Key| Arc::as_ptr(&k.0) as usize;
        let mut stack = vec![(root.clone(), false)];
        while let Some((k, expanded)) = stack.pop() {
            let ptr = address(&k);
            if self.ids.contains_key(&ptr) {
                continue;
            }
            if !expanded {
                stack.push((k.clone(), true));
                for child in k.0.children.iter().rev() {
                    if !self.ids.contains_key(&address(child)) {
                        stack.push((child.clone(), false));
                    }
                }
            } else {
                let children =
                    k.0.children
                        .iter()
                        .map(|c| self.ids[&address(c)])
                        .collect::<Vec<_>>();
                let id = self.nodes.len();
                self.nodes.push(
                    json!({"anchor":k.0.anchor,"label":k.0.label.as_ref(),"children":children}),
                );
                self.ids.insert(ptr, id);
            }
        }
        self.ids[&address(&root)]
    }
}
