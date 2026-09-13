//! Symbolic endpoint/effect display for selected native occurrence DAGs.
//! Earlier LHS/actions are never rewritten using later unions.
use crate::native_analyze::{Input, Output, Port, Record, RuleInfo};
use egglog::ast::{Action, Expr, Fact, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Sym {
    Var(usize),
    Lit(String),
}
impl std::fmt::Display for Sym {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Var(i) => write!(f, "v{i}"),
            Self::Lit(s) => write!(f, "{s}"),
        }
    }
}
#[derive(Clone)]
enum Bound {
    Value(Sym),
    Row(usize),
    External,
}
struct Row {
    op: String,
    args: Vec<Sym>,
    out: Sym,
}
struct Lower<'a> {
    records: &'a [Record],
    rules: &'a [RuleInfo],
    uf: Vec<Sym>,
    entry: BTreeSet<usize>,
    bound: BTreeSet<usize>,
    rows: Vec<Row>,
    body: Vec<String>,
    head: Vec<String>,
    memo: BTreeMap<usize, Vec<Bound>>,
}
fn span_key(s: &Span) -> Result<Arc<str>> {
    if let Span::Egglog(s) = s {
        Ok(Arc::from(format!("{:?}:{}:{}", s.file.name, s.i, s.j)))
    } else {
        Err("unmapped source span".into())
    }
}
impl<'a> Lower<'a> {
    fn fresh(&mut self, entry: bool) -> Sym {
        let i = self.uf.len();
        let v = Sym::Var(i);
        self.uf.push(v.clone());
        if entry {
            self.entry.insert(i);
        }
        v
    }
    fn find(&self, v: &Sym) -> Sym {
        let mut v = v.clone();
        while let Sym::Var(i) = v {
            if self.uf[i] == v {
                break;
            }
            v = self.uf[i].clone();
        }
        v
    }
    fn is_entry(&self, s: &Sym) -> bool {
        match s {
            Sym::Lit(_) => true,
            Sym::Var(i) => self.entry.contains(i),
        }
    }
    fn union(&mut self, a: &Sym, b: &Sym) -> Result<()> {
        let (mut a, mut b) = (self.find(a), self.find(b));
        if a == b {
            return Ok(());
        }
        if matches!(a, Sym::Var(_))
            && (matches!(b, Sym::Lit(_)) || self.is_entry(&b) && !self.is_entry(&a))
        {
            std::mem::swap(&mut a, &mut b);
        }
        if let Sym::Var(i) = b {
            self.uf[i] = a;
            Ok(())
        } else {
            Err("incompatible literals".into())
        }
    }
    fn constrain(&mut self, a: &Sym, b: &Sym) -> Result<()> {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return Ok(());
        }
        for (x, y) in [(&a, &b), (&b, &a)] {
            if let Sym::Var(i) = x {
                if self.entry.contains(i) && !self.bound.contains(i) {
                    self.uf[*i] = y.clone();
                    return Ok(());
                }
            }
        }
        if self.is_entry(&a) && self.is_entry(&b) {
            self.body.push(format!("(= {a} {b})"));
            self.union(&a, &b)
        } else {
            Err("coarse join requires an intermediate result; staged matching required".into())
        }
    }
    fn expr(
        &mut self,
        e: &Expr,
        env: &mut BTreeMap<String, Sym>,
        reads: &BTreeMap<Arc<str>, Bound>,
        calls: &mut BTreeMap<Arc<str>, usize>,
        head: bool,
    ) -> Result<Sym> {
        match e {
            Expr::Lit(_, v) => Ok(Sym::Lit(v.to_string())),
            Expr::Var(_, n) => {
                if !env.contains_key(n) {
                    let v = self.fresh(!head);
                    env.insert(n.clone(), v);
                }
                Ok(self.find(&env[n]))
            }
            Expr::Call(span, op, args) => {
                if ![
                    "Add", "Mul", "Sub", "Div", "Pow", "Diff", "Integral", "Sin", "Cos", "Ln",
                    "Sqrt", "Const", "Var",
                ]
                .contains(&op.as_str())
                {
                    return Err(format!("unsupported endpoint operator {op}"));
                }
                let args = args
                    .iter()
                    .map(|a| self.expr(a, env, reads, calls, head))
                    .collect::<Result<Vec<_>>>()?;
                let span = span_key(span)?;
                let idx = if let Some(Bound::Row(row)) = reads.get(&span).filter(|_| !head) {
                    let row = *row;
                    if self.rows[row].op != *op || self.rows[row].args.len() != args.len() {
                        return Err("fact-port shape mismatch".into());
                    }
                    let old = self.rows[row].args.clone();
                    for (a, b) in args.iter().zip(&old) {
                        self.constrain(a, b)?;
                    }
                    row
                } else if let Some(idx) = self.rows.iter().position(|r| {
                    r.op == *op
                        && r.args.iter().map(|a| self.find(a)).collect::<Vec<_>>()
                            == args.iter().map(|a| self.find(a)).collect::<Vec<_>>()
                }) {
                    idx
                } else {
                    let args: Vec<_> = args.iter().map(|a| self.find(a)).collect();
                    if !head && args.iter().any(|a| !self.is_entry(a)) {
                        return Err(
                            "coarse read needs an intermediate value; staged matching required"
                                .into(),
                        );
                    }
                    let out = self.fresh(!head);
                    let term = format!(
                        "({op}{})",
                        args.iter().map(|a| format!(" {a}")).collect::<String>()
                    );
                    if head {
                        self.head.push(format!("(let {out} {term})"));
                    } else {
                        self.body.push(format!("(= {out} {term})"));
                        for v in args.iter().chain(std::iter::once(&out)) {
                            if let Sym::Var(i) = v {
                                self.bound.insert(*i);
                            }
                        }
                    }
                    let idx = self.rows.len();
                    self.rows.push(Row {
                        op: op.clone(),
                        args,
                        out,
                    });
                    idx
                };
                calls.insert(span, idx);
                Ok(self.find(&self.rows[idx].out))
            }
        }
    }
    fn apply(&mut self, index: usize) -> Result<Vec<Bound>> {
        if let Some(v) = self.memo.get(&index) {
            return Ok(v.clone());
        }
        let r = self.records[index].clone();
        let parents = r
            .parents
            .iter()
            .map(|p| self.apply(*p))
            .collect::<Result<Vec<_>>>()?;
        let mut env = BTreeMap::new();
        let mut reads = BTreeMap::new();
        let mut external = BTreeMap::new();
        let mut calls = BTreeMap::new();
        for (input, port) in r.inputs.iter().zip(&r.ports) {
            let value = match port {
                Port::Parent(p, j) => parents
                    .get(*p)
                    .and_then(|p| p.get(*j))
                    .cloned()
                    .ok_or("missing parent output")?,
                Port::External(k) => {
                    if !external.contains_key(k) {
                        let v = if matches!(input, Input::Read(_)) {
                            Bound::External
                        } else {
                            Bound::Value(self.fresh(true))
                        };
                        external.insert(*k, v);
                    }
                    external[k].clone()
                }
            };
            match input {
                Input::Var(n) => {
                    let Bound::Value(v) = value else {
                        return Err("value port is a fact".into());
                    };
                    env.insert(n.clone(), v);
                }
                Input::Read(s) => {
                    reads.insert(s.clone(), value);
                }
            }
        }
        let rule = self.rules[r.rule].rule.clone();
        for f in &rule.body {
            let Fact::Eq(_, a, b) = f else {
                return Err("source guard needs staged matching".into());
            };
            if let Expr::Var(_, n) = a {
                if !env.contains_key(n) {
                    let v = self.expr(b, &mut env, &reads, &mut calls, false)?;
                    env.insert(n.clone(), v);
                    continue;
                }
            }
            let a = self.expr(a, &mut env, &reads, &mut calls, false)?;
            let b = self.expr(b, &mut env, &reads, &mut calls, false)?;
            self.constrain(&a, &b)?;
        }
        if reads.keys().any(|s| !calls.contains_key(s)) {
            return Err("unmapped LHS witness".into());
        }
        for action in &rule.head.0 {
            let Action::Union(_, a, b) = action else {
                return Err("unsupported source effect".into());
            };
            let a = self.expr(a, &mut env, &reads, &mut calls, true)?;
            let b = self.expr(b, &mut env, &reads, &mut calls, true)?;
            if self.find(&a) != self.find(&b) {
                self.head.push(format!("(union {a} {b})"));
                self.union(&a, &b)?;
            }
        }
        let mut outputs = vec![];
        for role in &r.outputs {
            outputs.push(match role {
                Output::Var(n) => Bound::Value(env.get(n).ok_or("unbound output")?.clone()),
                Output::Row(span) => Bound::Row(*calls.get(span).ok_or("unmapped output row")?),
                Output::Column(span, col) => {
                    let row = &self.rows[*calls.get(span).ok_or("unmapped output column")?];
                    Bound::Value(if *col < row.args.len() {
                        row.args[*col].clone()
                    } else if *col == row.args.len() {
                        row.out.clone()
                    } else {
                        return Err("column out of bounds".into());
                    })
                }
            });
        }
        self.memo.insert(index, outputs.clone());
        Ok(outputs)
    }
}
pub struct Lowered {
    pub code: String,
    pub steps: Vec<usize>,
}
pub fn lower(records: &[Record], rules: &[RuleInfo], index: usize) -> Result<Lowered> {
    let mut l = Lower {
        records,
        rules,
        uf: vec![],
        entry: BTreeSet::new(),
        bound: BTreeSet::new(),
        rows: vec![],
        body: vec![],
        head: vec![],
        memo: BTreeMap::new(),
    };
    l.apply(index)?;
    let steps = l.memo.keys().copied().collect();
    if l.head.is_empty() {
        return Ok(Lowered {
            code: "; no new endpoint effect".into(),
            steps,
        });
    }
    let mut seen = BTreeSet::new();
    l.body.retain(|b| seen.insert(b.clone()));
    Ok(Lowered {
        code: format!(
            "(rule (\n  {}\n) (\n  {}\n) :name \"native_comb_{}\")",
            l.body.join("\n  "),
            l.head.join("\n  "),
            records[index].id
        ),
        steps,
    })
}
