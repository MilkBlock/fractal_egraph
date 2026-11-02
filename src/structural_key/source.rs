//! Explicit native DAG extension for generated source. Registers primitives only:
//! rules, choices, anchoring and maintenance schedules remain in the .egg file.
use super::key::*;
use crate::pipeline::Result;
use egglog::{Core, EGraph, Value};
use serde_json::{Value as Json, json};
use std::collections::HashMap;
use std::{collections::BTreeSet, sync::Arc};

pub const HEADER: &str = "; cell-structural-key-runtime/v1 ";

/// Native sort/primitive registrations are not AST commands. Keep their
/// declaration when extracting or serializing a local replay program.
pub fn declaration(text: &str) -> Result<Option<&str>> {
    let mut headers = text.lines().filter(|l| l.starts_with(HEADER));
    let header = headers.next();
    if headers.next().is_some() {
        return Err("duplicate structural-key runtime declaration".into());
    }
    Ok(header)
}

pub fn inherit_runtime(source: &str, generated: &str) -> Result<String> {
    let header = declaration(source)?;
    match (header, declaration(generated)?) {
        (Some(a), Some(b)) if a != b => Err("structural-key replay environment differs".into()),
        // Append so symbolic-size replay keeps its own first-line dispatch tag.
        (Some(header), None) => Ok(format!("{generated}\n{header}\n")),
        _ => Ok(generated.to_owned()),
    }
}

/// Exact, compact DAG payload. Diagnostic height/hash labels cannot be replayed.
pub fn value_literal(eg: &EGraph, value: Value) -> Result<String> {
    let key = eg.value_to_base::<Stored>(value);
    // AST Display does not preserve JSON string escapes. An ASCII payload also
    // survives command serialization, history replay and symbolic lowering.
    let payload = serde_json::to_vec(&*key)?;
    let hex: String = payload.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("(cell-key-decode \"dag-v1:{hex}\")"))
}
// A typed conditional avoids constructing a Map for every merge, and supports
// any result sort without registering one application-specific primitive each.
#[derive(Clone)]
struct Select;
struct SelectTypes(egglog::ArcSort, egglog::ast::Span);
impl egglog::constraint::TypeConstraint for SelectTypes {
    fn get(
        &self,
        a: &[egglog::AtomTerm],
        info: &egglog::TypeInfo,
    ) -> Vec<Box<dyn egglog::constraint::Constraint<egglog::AtomTerm, egglog::ArcSort>>> {
        use egglog::constraint::{AllEqualTypeConstraint, assign};
        if a.len() != 4 {
            return AllEqualTypeConstraint::new("cell-key-select", self.1.clone())
                .with_exact_length(4)
                .get(a, info);
        }
        let mut c = AllEqualTypeConstraint::new("cell-key-select", self.1.clone())
            .get(&[a[0].clone(), a[1].clone(), a[3].clone()], info);
        c.push(assign(a[2].clone(), self.0.clone()));
        c
    }
}
impl egglog::Primitive for Select {
    fn name(&self) -> &str {
        "cell-key-select"
    }
    fn get_type_constraints(
        &self,
        span: &egglog::ast::Span,
    ) -> Box<dyn egglog::constraint::TypeConstraint> {
        use egglog::prelude::BaseSort;
        Box::new(SelectTypes(
            egglog::sort::BoolSort.to_arcsort(),
            span.clone(),
        ))
    }
}
impl egglog::PurePrim for Select {
    fn apply<'a, 'db>(&self, s: egglog::PureState<'a, 'db>, a: &[Value]) -> Option<Value> {
        Some(if s.base_values().unwrap::<bool>(a[2]) {
            a[0]
        } else {
            a[1]
        })
    }
}

pub fn install(eg: &mut EGraph, text: &str) -> Result<Option<Json>> {
    let Some(header) = declaration(text)? else {
        return Ok(None);
    };
    let spec: Json = serde_json::from_str(&header[HEADER.len()..])?;
    let nodes = spec["nodes"]
        .as_array()
        .ok_or("key runtime declaration needs nodes")?;
    let bases = spec["bases"]
        .as_array()
        .ok_or("key runtime declaration needs bases")?;
    if nodes.is_empty() && bases.is_empty() {
        return Ok(Some(
            json!({"name":"cell-structural-key-runtime/v1","nodes":0,
            "bases":[],"registration_skipped":true,"source_controls_rules_and_schedule":true}),
        ));
    }
    if eg.get_sort_by_name(SORT).is_some() {
        return Err("structural-key sort already installed".into());
    }
    super::key::install_base(eg)?;
    eg.add_pure_primitive(Select, None);
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

#[cfg(test)]
mod tests {
    use super::*;
    const SPEC: &str = r#"; cell-structural-key-runtime/v1 {"nodes":[{"primitive":"cell-key-node-leaf","label":"E:leaf","arity":0}],"bases":[]}"#;
    #[test]
    fn replay_runtime_keeps_dispatch_tag_and_rejects_different_environment() {
        let body = "; ripsat-symbolic-size/v1\n(datatype E (V))\n";
        let replay = inherit_runtime(SPEC, body).unwrap();
        assert!(replay.starts_with("; ripsat-symbolic-size/v1"));
        assert_eq!(declaration(&replay).unwrap(), Some(SPEC));
        assert_eq!(inherit_runtime(SPEC, &replay).unwrap(), replay);
        assert!(inherit_runtime(&SPEC.replace("E:leaf", "Other:leaf"), &replay).is_err());
        assert!(declaration(&format!("{SPEC}\n{SPEC}")).is_err());
        assert_eq!(inherit_runtime("", body).unwrap(), body);
    }
    #[test]
    fn replay_key_payload_is_exact_and_does_not_expand_shared_dag() {
        let mut key = Key::new("leaf".into(), vec![]);
        for _ in 0..40 {
            key = Key::new("pair".into(), vec![key.clone(), key]);
        }
        key = key.anchor();
        let payload = serde_json::to_string(&key).unwrap();
        assert!(payload.len() < 10_000);
        assert_eq!(Key::decode_legacy(&payload).unwrap(), key);
        assert!(Key::decode_legacy("key:40:1234").is_none());
        for bad in ["dag-v1:0", "dag-v1:gg", "dag-v1:7b7d"] {
            assert!(Key::decode_legacy(bad).is_none());
        }
        let mut eg = EGraph::default();
        install(&mut eg, SPEC).unwrap();
        eg.parse_and_run_program(
            None,
            &format!(
                "(let k (cell-key-decode {}))",
                serde_json::to_string(&payload).unwrap()
            ),
        )
        .unwrap();
        let (_, v) = eg
            .eval_expr(&egglog::ast::Expr::Var(
                egglog::ast::Span::Panic,
                "k".into(),
            ))
            .unwrap();
        let literal = value_literal(&eg, v).unwrap();
        let mut fresh = EGraph::default();
        install(&mut fresh, SPEC).unwrap();
        fresh
            .parse_and_run_program(None, &format!("(let k {literal})"))
            .unwrap();
        let (_, v) = fresh
            .eval_expr(&egglog::ast::Expr::Var(
                egglog::ast::Span::Panic,
                "k".into(),
            ))
            .unwrap();
        assert_eq!(&*fresh.value_to_base::<Stored>(v), &key);
    }
    #[test]
    fn select_has_one_sort_for_both_values_and_result() {
        let mut eg = EGraph::default();
        install(&mut eg, SPEC).unwrap();
        eg.parse_and_run_program(
            None,
            r#"
            (datatype E (N i64)) (N 1) (N 2)
            (check (= (cell-key-select 1 2 true) 1))
            (check (= (cell-key-select (N 1) (N 2) false) (N 2)))
        "#,
        )
        .unwrap();
        for text in [
            r#"(cell-key-select 1 "wrong" true)"#,
            "(cell-key-select 1 2 3)",
            "(cell-key-select 1 2)",
        ] {
            assert!(eg.parse_and_run_program(None, text).is_err());
        }
    }
    #[test]
    fn empty_key_declaration_does_not_register_a_sort() {
        let mut eg = EGraph::default();
        let r = install(
            &mut eg,
            r#"; cell-structural-key-runtime/v1 {"nodes":[],"bases":[]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(r["registration_skipped"], true);
        assert!(eg.get_sort_by_name(SORT).is_none());
    }
}
