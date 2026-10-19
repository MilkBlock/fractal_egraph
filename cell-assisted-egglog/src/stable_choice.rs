//! Deterministic structural tie-breaking for eggcc's native `to-hoist` merge.
//! Keys are finite constructor DAGs, never e-class IDs or a hash-only ordering.
use crate::Result;
use egglog::{
    ArcSort, Core, EGraph, Primitive, PurePrim, PureState, TermDag, TermId, Value,
    ast::{Command, GenericSchedule, Literal, Schedule, Span},
    constraint::{SimpleTypeConstraint, TypeConstraint},
    prelude::{BaseSort, add_base_sort},
    sort::{BaseValues, Boxed, F, S},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    sync::Arc,
};
pub const RULESET: &str = "cell-stable-keys";
const SORT: &str = "CellStructuralKey";
const PREFIX: &str = "CellStructuralKey-";

#[derive(Clone, Debug)]
struct Key(Arc<Node>);
#[derive(Debug)]
struct Node {
    anchor: bool,
    height: usize,
    label: String,
    children: Vec<Key>,
    hash: u64,
}
impl Key {
    fn new(label: String, children: Vec<Key>) -> Self {
        let height = children.iter().map(|k| k.0.height + 1).max().unwrap_or(0);
        let mut hash = 0xcbf29ce484222325u64;
        for byte in label
            .as_bytes()
            .iter()
            .copied()
            .chain(children.iter().flat_map(|k| k.0.hash.to_le_bytes()))
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Self(Arc::new(Node {
            anchor: false,
            height,
            label,
            children,
            hash,
        }))
    }
    fn anchor(self) -> Self {
        if self.0.anchor {
            return self;
        }
        let mut key = Self::new("\0input-anchor".into(), vec![self]);
        let node = Arc::get_mut(&mut key.0).unwrap();
        node.anchor = true;
        node.height = 0;
        key
    }
}
impl Hash for Key {
    fn hash<H: Hasher>(&self, s: &mut H) {
        self.0.hash.hash(s)
    }
}
impl PartialEq for Key {
    fn eq(&self, b: &Self) -> bool {
        Arc::ptr_eq(&self.0, &b.0) || (self.0.hash == b.0.hash && self.cmp(b) == Ordering::Equal)
    }
}
impl Eq for Key {}
impl Ord for Key {
    fn cmp(&self, b: &Self) -> Ordering {
        if Arc::ptr_eq(&self.0, &b.0) {
            return Ordering::Equal;
        }
        b.0.anchor
            .cmp(&self.0.anchor)
            .then_with(|| self.0.height.cmp(&b.0.height))
            .then_with(|| self.0.label.cmp(&b.0.label))
            .then_with(|| self.0.children.cmp(&b.0.children))
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}
type Stored = Boxed<Key>;
#[derive(Debug)]
struct KeySort;
impl BaseSort for KeySort {
    type Base = Stored;
    fn name(&self) -> &str {
        SORT
    }
    fn reconstruct_termdag(&self, b: &BaseValues, v: Value, d: &mut TermDag) -> TermId {
        let k = b.unwrap::<Stored>(v);
        // Only a diagnostic rendering. Equality/order always uses the full DAG.
        d.lit(Literal::String(format!(
            "key:{}:{:016x}",
            k.0.0.height, k.0.0.hash
        )))
    }
}
#[derive(Clone)]
enum Op {
    Min,
    Choose,
    Anchor,
    Node(String),
    Literal(String),
}
#[derive(Clone)]
struct Prim {
    name: String,
    types: Vec<ArcSort>,
    op: Op,
}
impl Primitive for Prim {
    fn name(&self) -> &str {
        &self.name
    }
    fn get_type_constraints(&self, s: &Span) -> Box<dyn TypeConstraint> {
        SimpleTypeConstraint::new(self.name(), self.types.clone(), s.clone()).into_box()
    }
}
impl PurePrim for Prim {
    fn apply<'a, 'db>(&self, s: PureState<'a, 'db>, a: &[Value]) -> Option<Value> {
        let b = s.base_values();
        let key = |v| b.unwrap::<Stored>(v);
        match &self.op {
            Op::Min => Some(if key(a[0]) <= key(a[1]) { a[0] } else { a[1] }),
            Op::Anchor => Some(b.get(Stored::new((*key(a[0])).clone().anchor()))),
            Op::Choose => {
                let c0 = b.unwrap::<i64>(a[2]);
                let c1 = b.unwrap::<i64>(a[3]);
                Some(if c0 > c1 || (c0 == c1 && key(a[4]) <= key(a[5])) {
                    a[0]
                } else {
                    a[1]
                })
            }
            Op::Node(label) => Some(b.get(Stored::new(Key::new(
                label.clone(),
                a.iter().map(|v| (*key(*v)).clone()).collect(),
            )))),
            Op::Literal(sort) => {
                let text = match sort.as_str() {
                    "i64" => b.unwrap::<i64>(a[0]).to_string(),
                    "f64" => {
                        let value = b.unwrap::<F>(a[0]);
                        let bits = if value.is_nan() {
                            f64::NAN.to_bits()
                        } else if value.to_bits() & 0x7fff_ffff_ffff_ffff == 0 {
                            0
                        } else {
                            value.to_bits()
                        };
                        format!("{bits:016x}")
                    }
                    "String" => format!("{:?}", b.unwrap::<S>(a[0]).as_str()),
                    "bool" => b.unwrap::<bool>(a[0]).to_string(),
                    "Unit" => "()".into(),
                    _ => unreachable!(),
                };
                Some(b.get(Stored::new(Key::new(format!("{sort}:{text}"), vec![]))))
            }
        }
    }
}
fn private(eg: &mut EGraph, source: &str) -> Result<()> {
    let mut cmds = eg.parse_program(None, source)?;
    for c in &mut cmds {
        if let Command::Function {
            let_binding,
            hidden,
            ..
        } = c
        {
            *let_binding = true;
            *hidden = true;
        }
    }
    eg.run_program(cmds)?;
    Ok(())
}
fn key_function(sort: &str) -> String {
    format!("{PREFIX}{sort}")
}
fn declare_key(eg: &mut EGraph, sort: &str) -> Result<()> {
    private(
        eg,
        &format!(
            "(function {} ({sort}) {SORT} :merge (cell-key-min old new) :unextractable)",
            key_function(sort)
        ),
    )
}
/// Execute the original setup, replacing only the merge of `to-hoist`.
/// Adds the same auxiliary rules to all backends. No core engine modification.
pub fn setup(eg: &mut EGraph, commands: &[Command]) -> Result<()> {
    let mut found = false;
    for c in commands {
        let mut c = c.clone();
        if let Command::Function { name, merge, .. } = &mut c {
            if name == "to-hoist" {
                if found {
                    return Err("duplicate to-hoist declaration".into());
                }
                found = true;
                add_base_sort(eg, KeySort, Span::Panic)?;
                let key = eg.get_sort_by_name(SORT).unwrap().clone();
                let expr = eg.get_sort_by_name("Expr").ok_or("missing Expr")?.clone();
                let int = eg.get_sort_by_name("i64").unwrap().clone();
                eg.add_pure_primitive(
                    Prim {
                        name: "cell-key-min".into(),
                        types: vec![key.clone(); 3],
                        op: Op::Min,
                    },
                    None,
                );
                eg.add_pure_primitive(
                    Prim {
                        name: "cell-key-anchor".into(),
                        types: vec![key.clone(); 2],
                        op: Op::Anchor,
                    },
                    None,
                );
                eg.add_pure_primitive(
                    Prim {
                        name: "cell-hoist-best".into(),
                        types: vec![
                            expr.clone(),
                            expr.clone(),
                            int.clone(),
                            int,
                            key.clone(),
                            key,
                            expr,
                        ],
                        op: Op::Choose,
                    },
                    None,
                );
                declare_key(eg, "Expr")?;
                *merge=Some(egglog::ast::Parser::default().get_expr_from_string(None,"(cell-hoist-best old new (Expr-size old) (Expr-size new) (CellStructuralKey-Expr old) (CellStructuralKey-Expr new))")?);
            }
        }
        eg.run_program(vec![c])?;
    }
    if !found {
        return Err("missing to-hoist declaration".into());
    }
    install_rules(eg)?;
    // Freeze the current input's structural keys once. Subsequent alternatives
    // do not recursively rename the entire input; union still merges keys by min.
    eg.parse_and_run_program(
        None,
        &format!("(run-schedule (saturate {RULESET})) (run cell-stable-freeze 1)"),
    )?;
    Ok(())
}
fn install_rules(eg: &mut EGraph) -> Result<()> {
    let functions = eg
        .get_function_names()
        .into_iter()
        .filter_map(|n| {
            let f = eg.get_function(&n)?;
            f.is_constructor()
                .then(|| (n, (f.schema().input.clone(), f.schema().output.clone())))
        })
        .collect::<BTreeMap<_, _>>();
    let mut reachable = BTreeSet::from(["Expr".to_string()]);
    loop {
        let before = reachable.len();
        for (inputs, output) in functions.values() {
            if reachable.contains(output.name()) {
                for s in inputs {
                    if s.is_eq_sort() {
                        reachable.insert(s.name().into());
                    }
                }
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    for sort in &reachable {
        if sort != "Expr" {
            declare_key(eg, sort)?;
        }
    }
    eg.parse_and_run_program(None, &format!("(ruleset {RULESET})"))?;
    eg.parse_and_run_program(None, "(ruleset cell-stable-freeze)")?;
    for sort in &reachable {
        let f = key_function(sort);
        eg.parse_and_run_program(None,&format!("(rule ((= k ({f} x))) ((set ({f} x) (cell-key-anchor k))) :ruleset cell-stable-freeze)"))?;
    }
    let key = eg.get_sort_by_name(SORT).unwrap().clone();
    let mut bases = BTreeSet::new();
    for (name, (inputs, output)) in functions {
        if !reachable.contains(output.name()) {
            continue;
        }
        let mut conditions = Vec::new();
        let mut args = vec![];
        let mut keys = vec![];
        for (i, s) in inputs.iter().enumerate() {
            let x = format!("x{i}");
            args.push(x.clone());
            if s.is_eq_sort() {
                let k = format!("k{i}");
                conditions.push(format!("(= {k} ({} {x}))", key_function(s.name())));
                keys.push(k);
            } else {
                if !["i64", "f64", "String", "bool", "Unit"].contains(&s.name()) {
                    return Err(format!("stable key does not support {}", s.name()).into());
                }
                let p = format!("cell-key-base-{}", s.name());
                if bases.insert(s.name().to_string()) {
                    eg.add_pure_primitive(
                        Prim {
                            name: p.clone(),
                            types: vec![s.clone(), key.clone()],
                            op: Op::Literal(s.name().into()),
                        },
                        None,
                    );
                }
                keys.push(format!("({p} {x})"));
            }
        }
        let p = format!("cell-key-node-{name}");
        eg.add_pure_primitive(
            Prim {
                name: p.clone(),
                types: vec![key.clone(); inputs.len() + 1],
                op: Op::Node(format!("{}:{name}", output.name())),
            },
            None,
        );
        let source = format!(
            "(rule ((= out ({name} {})) {}) ((set ({} out) ({p} {}))) :ruleset {RULESET})",
            args.join(" "),
            conditions.join(" "),
            key_function(output.name()),
            keys.join(" ")
        );
        eg.parse_and_run_program(None, &source)?;
    }
    Ok(())
}
pub fn schedule(eg: &mut EGraph, commands: &[Command]) -> Result<Vec<Command>> {
    let helper = eg.parse_program(None, &format!("(run-schedule (saturate {RULESET}))"))?;
    let Command::RunSchedule(helper) = &helper[0] else {
        unreachable!()
    };
    fn walk(s: &Schedule, helper: &Schedule) -> Schedule {
        match s {
            GenericSchedule::Run(span, r) if r.ruleset == "boundary-analysis" => {
                GenericSchedule::Sequence(span.clone(), vec![helper.clone(), s.clone()])
            }
            GenericSchedule::Sequence(span, xs) => GenericSchedule::Sequence(
                span.clone(),
                xs.iter().map(|x| walk(x, helper)).collect(),
            ),
            GenericSchedule::Repeat(span, n, x) => {
                GenericSchedule::Repeat(span.clone(), *n, Box::new(walk(x, helper)))
            }
            GenericSchedule::Saturate(span, x) => {
                GenericSchedule::Saturate(span.clone(), Box::new(walk(x, helper)))
            }
            _ => s.clone(),
        }
    }
    Ok(commands
        .iter()
        .map(|c| {
            if let Command::RunSchedule(s) = c {
                Command::RunSchedule(walk(s, helper))
            } else {
                c.clone()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_hash_collision_does_not_make_different_structures_equal() {
        let mut a = Key::new("Expr:A".into(), vec![]);
        let mut b = Key::new("Expr:B".into(), vec![]);
        Arc::get_mut(&mut a.0).unwrap().hash = 0;
        Arc::get_mut(&mut b.0).unwrap().hash = 0;
        assert_ne!(a, b);
        assert!(a < b);
    }
}
