//! Exact observed-prefix reuse; no template search or terminal-state cache.
use super::*;

pub(in crate::native_analyze) struct Plan {
    pub delta: Vec<Command>,
    pub skipped_stages: usize,
    pub skipped_commands: usize,
}
fn index(j: &Json, key: &str) -> Result<usize> {
    j[key]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| format!("missing replay {key}").into())
}
fn same(a: &[Command], b: &[Command]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_string() == b.to_string())
}
fn expression(e: &Expr, ops: &BTreeSet<String>) -> bool {
    match e {
        Expr::Call(_, op, args) => ops.contains(op) && args.iter().all(|e| expression(e, ops)),
        _ => true,
    }
}
fn facts(fs: &[Fact], ops: &BTreeSet<String>) -> bool {
    fs.iter().all(|f| match f {
        Fact::Eq(_, a, b) => expression(a, ops) && expression(b, ops),
        Fact::Fact(e) => expression(e, ops),
    })
}
fn action(a: &Action, ops: &BTreeSet<String>) -> bool {
    match a {
        Action::Expr(_, e) | Action::Let(_, _, e) => expression(e, ops),
        Action::Union(_, a, b) => expression(a, ops) && expression(b, ops),
        _ => false,
    }
}
fn positive(c: &Command, ops: &BTreeSet<String>) -> bool {
    match c {
        Command::Action(a) => action(a, ops),
        Command::Check(_, fs) => facts(fs, ops),
        Command::Fail(_, inner) => {
            matches!(inner.as_ref(),Command::Fail(_,check) if matches!(check.as_ref(),Command::Check(_,fs) if facts(fs,ops)))
        }
        _ => false,
    }
}
/// Saturating the parent early is valid for this positive constructor fragment.
/// Mutable tables, visibility changes, primitive guards and arbitrary merges
/// need a stronger schedule contract: they keep the original rebuild path.
pub(super) fn plan(parent: &[Command], po: &Json, child: &[Command], co: &Json) -> Result<Plan> {
    let p = &po["replay_plan"];
    let c = &co["replay_plan"];
    let pe = index(p, "environment_end")?;
    let ce = index(c, "environment_end")?;
    let pp = index(p, "parameter_end")?;
    let cp = index(c, "parameter_end")?;
    if pe > pp
        || pp > parent.len()
        || ce > cp
        || cp > child.len()
        || !same(&parent[..pe], &child[..ce])
    {
        return Err("environment differs".into());
    }
    let ops: BTreeSet<_> = sorts::constructors(&child[..ce])
        .into_iter()
        .map(|(_, v)| v.name)
        .chain(child[..ce].iter().filter_map(|c| match c {
            Command::Relation { name, .. } => Some(name.clone()),
            _ => None,
        }))
        .collect();
    for cmd in &child[..ce] {
        match cmd {
            Command::Datatype { .. }
            | Command::Datatypes { .. }
            | Command::Constructor { .. }
            | Command::Relation { .. }
            | Command::AddRuleset(..) => {}
            Command::Sort {
                presort_and_args: None,
                ..
            } => {}
            Command::Rule { rule }
                if facts(&rule.body, &ops) && rule.head.0.iter().all(|a| action(a, &ops)) => {}
            _ => return Err("nonpositive rule environment".into()),
        }
    }
    if !parent[pe..].iter().all(|c| positive(c, &ops))
        || !child[ce..].iter().all(|c| positive(c, &ops))
    {
        return Err("nonpositive replay effects or checks".into());
    }
    let ps = p["stages"].as_array().ok_or("missing parent stages")?;
    let cs = c["stages"].as_array().ok_or("missing child stages")?;
    if ps.is_empty() || ps.len() >= cs.len() {
        return Err("not a strict observed prefix".into());
    }
    let pparams = po["parameters"]
        .as_array()
        .ok_or("parent parameters missing")?;
    let cparams = co["parameters"]
        .as_array()
        .ok_or("child parameters missing")?;
    if pparams.len() > cparams.len()
        || !pparams
            .iter()
            .zip(cparams)
            .all(|(a, b)| a["token"] == b["token"] && a["sort"] == b["sort"])
    {
        return Err("boundary token coordinates differ".into());
    }
    if cp - ce < pp - pe || !same(&parent[pe..pp], &child[ce..ce + pp - pe]) {
        return Err("boundary terms differ".into());
    }
    let mut pend = pp;
    let mut cend = cp;
    for (a, b) in ps.iter().zip(cs) {
        let pa = index(a, "start")?;
        let pb = index(a, "end")?;
        let ca = index(b, "start")?;
        let cb = index(b, "end")?;
        if a["record"] != b["record"]
            || pa != pend
            || ca != cend
            || pa > pb
            || pb > parent.len()
            || ca > cb
            || cb > child.len()
            || !same(&parent[pa..pb], &child[ca..cb])
        {
            return Err("observed stages differ".into());
        }
        pend = pb;
        cend = cb;
    }
    let mut delta = child[ce + pp - pe..cp].to_vec();
    // Retain every suffix checkpoint in its original position.
    for s in &cs[ps.len()..] {
        let start = index(s, "start")?;
        let end = index(s, "end")?;
        if start != cend || start > end || end > child.len() {
            return Err("invalid suffix span".into());
        }
        delta.extend_from_slice(&child[start..end]);
        cend = end;
    }
    if !parent[pend..]
        .iter()
        .chain(&child[cend..])
        .all(|c| matches!(c, Command::Check(..)))
    {
        return Err("unaccounted replay actions after stage spans".into());
    }
    Ok(Plan {
        delta,
        skipped_stages: ps.len(),
        skipped_commands: pend - pe,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Vec<Command>, Json, Vec<Command>, Json) {
        let mut eg = EGraph::default();
        let env = "(datatype E (Input i64) (A E) (B E) (C E))";
        let p = eg
            .parse_program(
                None,
                &format!("{env} (Input 0) (A (Input 0)) (B (Input 0))"),
            )
            .unwrap();
        let c = eg
            .parse_program(
                None,
                &format!("{env} (Input 0) (Input 1) (A (Input 0)) (B (Input 0)) (C (Input 1))"),
            )
            .unwrap();
        let po = json!({"parameters":[{"token":10,"sort":"E"}],"replay_plan":{"environment_end":1,"parameter_end":2,"stages":[{"record":0,"start":2,"end":4}]}});
        let co = json!({"parameters":[{"token":10,"sort":"E"},{"token":20,"sort":"E"}],"replay_plan":{"environment_end":1,"parameter_end":3,"stages":[{"record":0,"start":3,"end":5},{"record":1,"start":5,"end":6}]}});
        (p, po, c, co)
    }
    #[test]
    fn exact_prefix_accepts_new_slots_but_not_rebinding_or_reordered_stages() {
        let (p, po, c, co) = fixture();
        let got = plan(&p, &po, &c, &co).unwrap();
        assert_eq!(
            got.delta
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["(Input 1)", "(C (Input 1))"]
        );
        let mut bad = co.clone();
        bad["parameters"][0]["token"] = json!(99);
        assert!(plan(&p, &po, &c, &bad).is_err());
        let mut bad = c.clone();
        bad.swap(3, 4);
        assert!(plan(&p, &po, &bad, &co).is_err());
        let mut bad = co.clone();
        bad["replay_plan"]["stages"][0]["record"] = json!(1);
        assert!(plan(&p, &po, &c, &bad).is_err());
    }
    #[test]
    fn negative_check_cannot_be_moved_after_parent_saturation() {
        let (p, po, mut c, mut co) = fixture();
        c.extend(
            EGraph::default()
                .parse_program(None, "(fail (check (= (C (Input 0)) (C (Input 0)))))")
                .unwrap(),
        );
        co["replay_plan"]["stages"][1]["end"] = json!(7);
        assert!(plan(&p, &po, &c, &co).is_err());
    }
}
