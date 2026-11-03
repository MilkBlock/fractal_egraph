//! Select correlated constructor fragments and materialize exact source views.
use crate::{Result, surface::*};
use eggfuse_cell_runtime::checksum::canonical;
use microlp::{ComparisonOp, OptimizationDirection, Problem, Variable};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
const PREFIX: &str = "CellSpineView";
const REPAIR: &str = "cell-spine-complete";
const SLOT: &str = "cell_spine_slot";
const SHADOW: &str = "cell-spine-original";
fn names(j: &J) -> BTreeSet<String> {
    let mut v = BTreeSet::new();
    vars(j, &mut v);
    v
}
#[derive(Clone)]
struct Flat {
    body: Vec<J>,
    head: Vec<J>,
    types: BTreeMap<String, String>,
    atoms: usize,
}
fn flatten(r: &J, sig: &J, globals: &BTreeSet<String>) -> Option<Flat> {
    let mut used = names(&r["body"]);
    if used.contains("_") {
        return None;
    }
    struct State<'a> {
        used: &'a mut BTreeSet<String>,
        sig: &'a J,
        atoms: Vec<J>,
        types: BTreeMap<String, String>,
        memo: BTreeMap<String, J>,
    }
    impl State<'_> {
        fn visit(&mut self, v: &J) -> J {
            if v["kind"] != "call" {
                return v.clone();
            }
            let n = s(&v["head"]);
            let args = a(&v["args"])
                .iter()
                .map(|x| self.visit(x))
                .collect::<Vec<_>>();
            let expr = call(n, args.clone());
            let st = &self.sig[n];
            if st["kind"] != "constructor" || args.iter().any(|a| a["kind"] == "call") {
                return expr;
            }
            let key = term(&expr);
            if let Some(v) = self.memo.get(&key) {
                return v.clone();
            }
            let mut name = format!("cell_spine_v{}", self.used.len());
            while !self.used.insert(name.clone()) {
                name.push('x')
            }
            let v = var(&name);
            for (x, t) in args
                .iter()
                .chain([&v])
                .zip(a(&st["input"]).iter().chain([&st["output"]]))
            {
                if x["kind"] == "var" {
                    self.types.insert(s(&x["name"]).into(), s(t).into());
                }
            }
            self.atoms.push(json!({"kind":"eq","args":[v,expr]}));
            self.memo.insert(key, v.clone());
            v
        }
    }
    let mut st = State {
        used: &mut used,
        sig,
        atoms: vec![],
        types: BTreeMap::new(),
        memo: BTreeMap::new(),
    };
    let rest=a(&r["body"]).iter().map(|f|if f["kind"]=="eq"{json!({"kind":"eq","args":a(&f["args"]).iter().map(|e|st.visit(e)).collect::<Vec<_>>()})}else{json!({"kind":"fact","expr":st.visit(&f["expr"])})}).collect::<Vec<_>>();
    let mut links = BTreeMap::new();
    fn root<'a>(mut v: &'a str, m: &'a BTreeMap<String, String>) -> &'a str {
        while let Some(x) = m.get(v) {
            v = x
        }
        v
    }
    for f in &rest {
        let xs = a(&f["args"]);
        if xs.len() == 2 && xs.iter().all(|x| x["kind"] == "var") {
            let mut k = [
                root(s(&xs[0]["name"]), &links).to_owned(),
                root(s(&xs[1]["name"]), &links).to_owned(),
            ];
            k.sort();
            if k[0] != k[1] {
                links.insert(k[1].clone(), k[0].clone());
            }
        }
    }
    let mut groups = BTreeMap::<String, BTreeSet<String>>::new();
    for x in links.keys().chain(links.values()) {
        groups
            .entry(root(x, &links).into())
            .or_default()
            .insert(x.clone());
    }
    let mut m = BTreeMap::new();
    for (k, g) in groups {
        let rep = g.intersection(globals).next().unwrap_or(&k);
        for n in g.difference(globals) {
            if n != rep {
                m.insert(n.clone(), var(rep));
            }
        }
    }
    let atoms = st.atoms.len();
    let mut body = st.atoms.iter().map(|x| subst(x, &m)).collect::<Vec<_>>();
    body.extend(rest.iter().map(|x| subst(x, &m)).filter(|x| {
        if x["kind"] == "eq" {
            x["args"][0] != x["args"][1]
        } else {
            x["expr"]["kind"] != "var"
        }
    }));
    let types = st
        .types
        .into_iter()
        .map(|(n, t)| (m.get(&n).map(|v| s(&v["name"]).into()).unwrap_or(n), t))
        .collect();
    Some(Flat {
        body,
        types,
        atoms,
        head: a(&r["head"])
            .iter()
            .map(|x| {
                let mut h = subst(x, &m);
                if h["kind"] == "let"
                    && let Some(v) = m.get(s(&h["name"]))
                {
                    h["name"] = v["name"].clone()
                }
                h
            })
            .collect(),
    })
}
#[derive(Clone)]
struct Occ {
    rid: usize,
    atoms: Vec<usize>,
    args: Vec<J>,
}
#[derive(Clone)]
struct View {
    body: Vec<J>,
    sorts: Vec<String>,
    uses: Vec<Occ>,
    name: String,
    completion: Vec<J>,
}
fn fields(
    j: &J,
    seen: &mut BTreeMap<String, J>,
    actual: &mut Vec<J>,
    sorts: &mut Vec<String>,
    types: &BTreeMap<String, String>,
) {
    if j["kind"] == "var" {
        let n = s(&j["name"]);
        if !seen.contains_key(n) {
            seen.insert(n.into(), var(&format!("{SLOT}{}", actual.len())));
            actual.push(j.clone());
            sorts.push(types[n].clone());
        }
    } else {
        for x in a(&j["args"]) {
            fields(x, seen, actual, sorts, types)
        }
    }
}
fn discover(
    flat: &Flat,
    eq: &BTreeSet<String>,
    rid: usize,
    identity: bool,
    views: &mut Vec<View>,
    ids: &mut BTreeMap<String, usize>,
) {
    let n = flat.atoms;
    let atoms = &flat.body[..n];
    let vertices = atoms
        .iter()
        .map(|f| {
            names(f)
                .into_iter()
                .filter(|x| !identity || eq.contains(&flat.types[x]))
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut dist = vec![vec![usize::MAX / 3; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                dist[i][j] = 0
            } else if !vertices[i].is_disjoint(&vertices[j]) {
                dist[i][j] = 1
            }
        }
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                dist[i][j] = dist[i][j].min(dist[i][k] + dist[k][j]);
            }
        }
    }
    let mut fs = BTreeSet::new();
    for i in 0..n {
        for j in i + 1..n {
            fs.insert(vec![i, j]);
        }
        for end in i + 3..=n {
            fs.insert((i..end).collect());
        }
    }
    let mut connected = BTreeSet::new();
    for fragment in fs {
        if !identity {
            if !vertices[fragment[0]].is_disjoint(&vertices[fragment[1]]) {
                connected.insert(fragment);
            }
            continue;
        }
        let mut chosen = fragment.iter().copied().collect::<BTreeSet<_>>();
        let mut reachable = BTreeSet::from([fragment[0]]);
        loop {
            let k = reachable.len();
            for &i in &fragment {
                if reachable.iter().any(|&other| dist[i][other] == 1) {
                    reachable.insert(i);
                }
            }
            if reachable.len() == k {
                break;
            }
        }
        if reachable != chosen {
            if fragment
                .iter()
                .any(|&i| dist[i][fragment[0]] >= usize::MAX / 3)
            {
                continue;
            }
            for (i, &l) in fragment.iter().enumerate() {
                for &r in &fragment[i + 1..] {
                    for v in 0..n {
                        if dist[l][v] + dist[v][r] == dist[l][r] {
                            chosen.insert(v);
                        }
                    }
                }
            }
        }
        connected.insert(chosen.into_iter().collect::<Vec<_>>());
    }
    for f in connected {
        let mut best = None;
        for &first in &f {
            let mut order = vec![first];
            let mut known = names(&atoms[first]);
            let mut rest = f
                .iter()
                .copied()
                .filter(|&i| i != first)
                .collect::<Vec<_>>();
            while !rest.is_empty() {
                let choice = rest
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| !vertices[**i].is_disjoint(&known))
                    .min_by_key(|(_, i)| {
                        let e = &atoms[**i]["args"][1];
                        (!names(&e["args"]).is_subset(&known), term(e))
                    })
                    .map(|(j, _)| j);
                let Some(j) = choice else { break };
                let i = rest.remove(j);
                order.push(i);
                known.extend(names(&atoms[i]));
            }
            if !rest.is_empty() {
                continue;
            }
            let (mut m, mut args, mut sorts) = (BTreeMap::new(), vec![], vec![]);
            for &i in &order {
                for e in a(&atoms[i]["args"]) {
                    fields(e, &mut m, &mut args, &mut sorts, &flat.types)
                }
            }
            let body = order
                .iter()
                .map(|&i| subst(&atoms[i], &m))
                .collect::<Vec<_>>();
            let key = eggfuse_cell_runtime::checksum::canonical(&json!([body, sorts]), true);
            if best.as_ref().is_none_or(|(k, _, _, _)| &key < k) {
                best = Some((key, body, sorts, args));
            }
        }
        if let Some((k, body, sorts, args)) = best {
            let id = *ids.entry(k).or_insert(views.len());
            if id == views.len() {
                views.push(View {
                    body,
                    sorts,
                    uses: vec![],
                    name: String::new(),
                    completion: vec![],
                });
            }
            views[id].uses.push(Occ {
                rid,
                atoms: f,
                args,
            });
        }
    }
}
fn costs(view: &View, rules: &[J], profile: Option<&J>) -> Result<(f64, Vec<f64>)> {
    let Some(p) = profile else {
        return Ok((
            view.body.len() as f64,
            vec![(view.body.len() - 1) as f64; view.uses.len()],
        ));
    };
    let key = fragment_key(view);
    let data = &p["markers"][&key];
    let maintenance = data["maintenance_ns"]
        .as_f64()
        .filter(|x| x.is_finite() && *x >= 0.)
        .ok_or_else(|| format!("invalid maintenance cost for {key}"))?;
    let saved = view
        .uses
        .iter()
        .map(|o| {
            let r = &rules[o.rid];
            let name = canonical(&json!([r["ruleset"], r["name"]]), false);
            data["saved_search_ns_by_rule"][name]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or("invalid measured consumer cost")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok((maintenance, saved))
}
fn fragment_key(view: &View) -> String {
    crate::sha(canonical(&json!([view.body, view.sorts]), true).as_bytes())
}
fn select(views: &[View], rules: &[J], profile: Option<&J>) -> Result<Vec<View>> {
    let mut groups = vec![];
    for v in views {
        let (c, s) = costs(v, rules, profile)?;
        if s.iter().copied().map(|x| x.max(0.)).sum::<f64>() >= c {
            groups.push((v, c, s));
        }
    }
    if groups.is_empty() {
        return Ok(vec![]);
    }
    // Solve measured fractional costs without perturbing the primary optimum.
    let build = |primary: Option<f64>| {
        let mut model = Problem::new(OptimizationDirection::Minimize);
        let mut objective = vec![];
        let mut overlaps = BTreeMap::<(usize, usize), Vec<(Variable, f64)>>::new();
        let mut variables = vec![];
        for (view, c, saved) in &groups {
            let v = model.add_binary_var(if primary.is_some() { 0. } else { *c });
            objective.push((v, *c));
            let mut gate = vec![(
                v,
                -(view
                    .uses
                    .iter()
                    .map(|o| o.rid)
                    .collect::<BTreeSet<_>>()
                    .len()
                    .min(2) as f64),
            )];
            let mut uses = vec![];
            for (o, &cost) in view.uses.iter().zip(saved) {
                let u = model.add_binary_var(if primary.is_some() { -1. } else { -cost });
                objective.push((u, -cost));
                model.add_constraint(&[(u, 1.), (v, -1.)], ComparisonOp::Le, 0.);
                gate.push((u, 1.));
                uses.push(u);
                for &a in &o.atoms {
                    overlaps.entry((o.rid, a)).or_default().push((u, 1.));
                }
            }
            model.add_constraint(gate, ComparisonOp::Ge, 0.);
            variables.push((v, uses));
        }
        for row in overlaps.values() {
            model.add_constraint(row, ComparisonOp::Le, 1.);
        }
        if let Some(best) = primary {
            model.add_constraint(objective, ComparisonOp::Eq, best)
        }
        (model, variables)
    };
    let solve = |model: Problem| -> Result<microlp::Solution> {
        let sol = model
            .solve()?
            .into_solution()
            .map_err(|_| "marker solve interrupted")?;
        if sol.status() != microlp::SolutionStatus::Optimal {
            return Err("marker optimum not proven".into());
        }
        Ok(sol)
    };
    let best = solve(build(None).0)?.objective();
    let (model, vars) = build(Some(best));
    let sol = solve(model)?;
    let mut out = vec![];
    for ((v, _, _), (chosen, uses)) in groups.into_iter().zip(vars) {
        if sol.var_value(chosen) > 0.5 {
            let mut view = v.clone();
            view.name = format!("{PREFIX}{}", out.len());
            view.uses = v
                .uses
                .iter()
                .zip(uses)
                .filter(|(_, x)| sol.var_value(*x) > 0.5)
                .map(|(o, _)| o.clone())
                .collect();
            out.push(view)
        }
    }
    Ok(out)
}
fn marker_fact(v: &View) -> J {
    json!({"kind":"fact","expr":call(&v.name,(0..v.sorts.len()).map(|i|var(&format!("{SLOT}{i}"))).collect())})
}
pub fn rewrite(
    name: &str,
    source: &str,
    cells: &[crate::witness::Cell],
    profile: Option<&J>,
    identity: bool,
) -> Result<(String, J, BTreeSet<String>)> {
    if let Some(p) = profile
        && (p["schema"] != "marker-cost-profile/v1"
            || p["source_sha256"] != crate::sha(source.as_bytes()))
    {
        return Err("marker profile does not describe the structural-key source".into());
    }
    let data = egg_layout::source_ast::export(name, source)?;
    let sig = &data["signatures"];
    let active = active(&data);
    let rules = a(&data["rules"]);
    let commands = a(&data["commands"]);
    if commands.iter().any(|c| {
        [PREFIX, SHADOW, SLOT, REPAIR, "CellSpineApplied"]
            .iter()
            .any(|n| s(&c["text"]).contains(n))
    }) {
        return Err("reserved marker namespace".into());
    }
    let mut changed = BTreeSet::new();
    for ac in rules
        .iter()
        .flat_map(|r| a(&r["head"]))
        .chain(a(&data["actions"]).iter().map(|x| &x["action"]))
    {
        if ac["kind"] == "change" {
            changed.insert(s(&ac["head"]));
        }
    }
    let eq = sig
        .as_object()
        .unwrap()
        .values()
        .filter(|t| t["kind"] == "constructor")
        .map(|t| s(&t["output"]).to_owned())
        .collect();
    let (mut views, mut ids, mut flats) = (vec![], BTreeMap::new(), BTreeMap::new());
    for (rid, r) in rules.iter().enumerate() {
        if !active.contains(s(&r["ruleset"])) {
            continue;
        }
        let globals = a(&data["actions"])
            .iter()
            .filter(|ac| {
                ac["command_ordinal"].as_u64() < r["command_ordinal"].as_u64()
                    && ac["action"]["kind"] == "let"
            })
            .map(|ac| s(&ac["action"]["name"]).into())
            .collect();
        if let Some(f) = flatten(r, sig, &globals) {
            discover(&f, &eq, rid, identity, &mut views, &mut ids);
            flats.insert(rid, f);
        }
    }
    views.retain(|v| {
        !v.body
            .iter()
            .any(|f| changed.contains(s(&f["args"][1]["head"])))
    });
    let offered = views.len();
    let offers=views.iter().map(|v|json!({"key":fragment_key(v),"atoms":v.body.len(),"occurrences":v.uses.len(),"consumers":v.uses.iter().map(|o|canonical(&json!([rules[o.rid]["ruleset"],rules[o.rid]["name"]]),false)).collect::<BTreeSet<_>>() })).collect::<Vec<_>>();
    let mut views = select(&views, rules, profile)?;
    let selected = views.len();
    // Prefixes use the same field numbering as their children.
    let mut i = 0;
    while i < views.len() {
        let body = views[i].body.clone();
        if body.len() > 2 {
            let prefix = &body[..body.len() - 1];
            let idx = if let Some(j) = views.iter().position(|v| v.body == prefix) {
                j
            } else {
                let j = views.len();
                let count = names(&json!(prefix)).len();
                views.push(View {
                    name: format!("{PREFIX}{j}"),
                    body: prefix.to_vec(),
                    sorts: views[i].sorts[..count].to_vec(),
                    uses: vec![],
                    completion: vec![],
                });
                j
            };
            views[i].completion = vec![marker_fact(&views[idx]), body.last().unwrap().clone()];
        } else {
            views[i].completion = body;
        }
        i += 1;
    }
    let mut used = BTreeMap::<usize, Vec<(&View, &Occ)>>::new();
    for v in &views {
        for o in &v.uses {
            used.entry(o.rid).or_default().push((v, o));
        }
    }
    let mut replacements = BTreeMap::new();
    let mut sets = BTreeSet::new();
    let mut applied = vec![];
    let mut pubs = 0;
    for (rid, r) in rules.iter().enumerate() {
        let selected = used.get(&rid);
        let mut body = a(&r["body"]).to_vec();
        let mut head = a(&r["head"]).to_vec();
        let mut receipt = None;
        if let Some(selected) = selected {
            let f = &flats[&rid];
            let removed = selected
                .iter()
                .flat_map(|(_, o)| o.atoms.iter().copied())
                .collect::<BTreeSet<_>>();
            body = f
                .body
                .iter()
                .enumerate()
                .filter(|(i, _)| !removed.contains(i))
                .map(|(_, f)| f.clone())
                .collect();
            body.extend(
                selected
                    .iter()
                    .map(|(v, o)| json!({"kind":"fact","expr":call(&v.name,o.args.clone())})),
            );
            head = f.head.clone();
            sets.insert(s(&r["ruleset"]).to_owned());
            let mut types = f.types.clone();
            infer_types(&f.body, sig, &mut types);
            let inputs = names(&json!(head));
            let bound = names(&json!(f.body));
            let vars = inputs.intersection(&bound).collect::<Vec<_>>();
            if vars.iter().all(|v| types.contains_key(*v)) {
                let n = format!("CellSpineApplied{rid}");
                applied.push((
                    n.clone(),
                    vars.iter().map(|v| types[*v].clone()).collect::<Vec<_>>(),
                ));
                receipt = Some(write(&n, vars.iter().map(|v| var(v)).collect()));
            }
        }
        let f = flats.get(&rid);
        let original_body = if selected.is_some() {
            f.unwrap().body.as_slice()
        } else {
            a(&r["body"])
        };
        let original_head = if selected.is_some() {
            f.unwrap().head.as_slice()
        } else {
            a(&r["head"])
        };
        let extra = if active.contains(s(&r["ruleset"])) {
            publish(original_body, original_head, &views, sig)
        } else {
            vec![]
        };
        pubs += extra.len();
        if selected.is_some() || !extra.is_empty() {
            head.extend(extra);
            head.extend(receipt);
            replacements.insert(
                r["command_ordinal"].as_u64().unwrap() as usize,
                (
                    rid,
                    rule(r, &body, &head, s(&r["ruleset"])),
                    names(&json!(body)),
                ),
            );
        }
    }
    loop {
        let old = sets.len();
        for (k, xs) in data["combined_rulesets"].as_object().unwrap() {
            if a(xs).iter().any(|x| sets.contains(s(x))) {
                sets.insert(k.clone());
            }
        }
        if old == sets.len() {
            break;
        }
    }
    let install = rules
        .iter()
        .filter_map(|r| r["command_ordinal"].as_u64())
        .max()
        .map_or(0, |i| i as usize + 1);
    if !views.is_empty()
        && commands
            .iter()
            .take(install)
            .any(|c| c["kind"] == "schedule")
    {
        return Err("define rules before schedules for source markers".into());
    }
    let mut lines = source
        .lines()
        .filter(|x| x.starts_with(crate::keys::HEADER))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for (i, c) in commands.iter().enumerate() {
        if replacements.keys().next() == Some(&i) {
            lines.extend(
                replacements
                    .keys()
                    .map(|i| format!("(ruleset {SHADOW}{i})")),
            );
        }
        if i == install && !replacements.is_empty() {
            lines.push(format!("(ruleset {REPAIR})"));
            for v in &views {
                lines.push(decl(&v.name, &v.sorts));
            }
            for (n, ss) in &applied {
                lines.push(decl(n, ss));
            }
            for v in &views {
                let args = (0..v.sorts.len())
                    .map(|i| var(&format!("{SLOT}{i}")))
                    .collect();
                lines.push(format!(
                    "(rule ({}) ({}) :ruleset {REPAIR} :name \"complete-{}\")",
                    v.completion.iter().map(fact).collect::<Vec<_>>().join(" "),
                    action(&write(&v.name, args)),
                    v.name
                ));
            }
            lines.extend(replacements.values().map(|(_, t, _)| t.clone()));
        }
        if let Some((rid, _, locals)) = replacements.get(&i) {
            for ac in a(&data["actions"]) {
                let j = ac["command_ordinal"].as_u64().unwrap() as usize;
                if j > i
                    && j < install
                    && ac["action"]["kind"] == "let"
                    && locals.contains(s(&ac["action"]["name"]))
                {
                    return Err("late marker installation would capture a global".into());
                }
            }
            let r = &rules[*rid];
            lines.push(rule(
                r,
                a(&r["body"]),
                a(&r["head"]),
                &format!("{SHADOW}{i}"),
            ));
        } else if c["kind"] == "schedule" && !views.is_empty() {
            lines.push(form(
                "run-schedule",
                [schedule(&c["schedule"], &|rs, t| {
                    if sets.contains(rs) {
                        format!("(seq (saturate (run {REPAIR})) {t})")
                    } else {
                        t.into()
                    }
                })],
            ));
        } else {
            lines.push(s(&c["text"]).into())
        }
    }
    let report = json!({"offered":offered,"offered_fragments":offers,"locality":if identity{"identity"}else{"all"},"selection_scope":"exact positive source views; training observations provide witnesses","cost_model":if profile.is_some(){"profile-supplied lifecycle nanoseconds"}else{"structural baseline; not a runtime performance estimate"},"receipt_tables":applied.iter().map(|v|&v.0).collect::<Vec<_>>(),"selected":selected,"publish_actions":pubs,"markers":views.iter().map(|v|json!({"name":v.name,"body":v.body,"completion_body":v.completion,"sorts":v.sorts,"cells":crate::witness::matches(cells,&v.body,&v.sorts),"consumers":v.uses.iter().map(|o|o.rid).collect::<Vec<_>>() })).collect::<Vec<_>>()});
    Ok((
        lines.join("\n") + "\n",
        report,
        views.iter().map(|v| v.name.clone()).collect(),
    ))
}
fn decl(n: &str, sorts: &[String]) -> String {
    format!("(function {n} ({}) Unit :merge old)", sorts.join(" "))
}
fn write(n: &str, args: Vec<J>) -> J {
    json!({"kind":"set","head":n,"args":args,"value":{"kind":"literal","sort":"Unit","text":"()"}})
}
fn infer_types(body: &[J], sig: &J, types: &mut BTreeMap<String, String>) {
    fn infer(
        e: &J,
        t: Option<&str>,
        sig: &J,
        types: &mut BTreeMap<String, String>,
    ) -> Option<String> {
        match s(&e["kind"]) {
            "var" => {
                if let Some(t) = t {
                    types.insert(s(&e["name"]).into(), t.into());
                }
                types.get(s(&e["name"])).cloned()
            }
            "literal" => Some(s(&e["sort"]).into()),
            _ => {
                let st = &sig[s(&e["head"])];
                let ret = st["output"].as_str()?;
                for (a, t) in a(&e["args"]).iter().zip(a(&st["input"])) {
                    infer(a, t.as_str(), sig, types);
                }
                Some(ret.into())
            }
        }
    }
    for f in body {
        if f["kind"] == "eq" {
            let (a, b) = (&f["args"][0], &f["args"][1]);
            let l = infer(a, None, sig, types);
            let r = infer(b, None, sig, types);
            infer(a, r.as_deref(), sig, types);
            infer(b, l.as_deref(), sig, types);
        } else {
            infer(&f["expr"], None, sig, types);
        }
    }
}

#[derive(Default)]
struct Congruence {
    ids: BTreeMap<String, usize>,
    expr: Vec<J>,
    parents: Vec<usize>,
}
impl Congruence {
    fn add(&mut self, e: &J) -> usize {
        let k = term(e);
        if let Some(i) = self.ids.get(&k) {
            return *i;
        }
        for a in a(&e["args"]) {
            self.add(a);
        }
        let id = self.expr.len();
        self.expr.push(e.clone());
        self.parents.push(id);
        self.ids.insert(k, id);
        id
    }
    fn root(&self, mut i: usize) -> usize {
        while self.parents[i] != i {
            i = self.parents[i]
        }
        i
    }
    fn join(&mut self, a: usize, b: usize) -> bool {
        let (a, b) = (self.root(a), self.root(b));
        if a == b {
            return false;
        }
        self.parents[a.max(b)] = a.min(b);
        true
    }
    fn equal(&mut self, a: &J, b: &J) {
        let x = self.add(a);
        let y = self.add(b);
        self.join(x, y);
    }
    fn close(&mut self) {
        loop {
            let mut keys = BTreeMap::new();
            let mut change = false;
            for i in 0..self.expr.len() {
                let e = &self.expr[i];
                if e["kind"] != "call" {
                    continue;
                }
                let k = (
                    s(&e["head"]).to_owned(),
                    a(&e["args"])
                        .iter()
                        .map(|v| self.root(self.ids[&term(v)]))
                        .collect::<Vec<_>>(),
                );
                if let Some(old) = keys.insert(k, i) {
                    change |= self.join(i, old);
                }
            }
            if !change {
                break;
            }
        }
    }
}
fn pure(v: &J, sig: &J) -> bool {
    v["kind"] != "call"
        || (sig[s(&v["head"])]["kind"] == "constructor"
            && a(&v["args"]).iter().all(|a| pure(a, sig)))
}
fn publish(body: &[J], head: &[J], views: &[View], sig: &J) -> Vec<J> {
    if views.is_empty() || names(&json!([body, head])).contains("_") {
        return vec![];
    }
    let mut cg = Congruence::default();
    let mut observed = vec![];
    let mut created = BTreeSet::new();
    fn observe(
        e: &J,
        sig: &J,
        cg: &mut Congruence,
        obs: &mut Vec<usize>,
        made: &mut BTreeSet<String>,
        rhs: bool,
    ) {
        if e["kind"] == "call" {
            if pure(e, sig) {
                obs.push(cg.add(e));
                if rhs {
                    made.insert(s(&e["head"]).into());
                }
            }
            for x in a(&e["args"]) {
                observe(x, sig, cg, obs, made, rhs);
            }
        } else {
            cg.add(e);
        }
    }
    for f in body {
        if f["kind"] == "eq" {
            for e in a(&f["args"]) {
                observe(e, sig, &mut cg, &mut observed, &mut created, false)
            }
            cg.equal(&f["args"][0], &f["args"][1]);
        } else {
            observe(&f["expr"], sig, &mut cg, &mut observed, &mut created, false)
        }
    }
    for act in head {
        let expr = &act["expr"];
        if !expr.is_null() {
            observe(expr, sig, &mut cg, &mut observed, &mut created, true);
            if act["kind"] == "let" && pure(expr, sig) {
                cg.equal(&var(s(&act["name"])), expr)
            }
        }
        for e in a(&act["args"]) {
            observe(e, sig, &mut cg, &mut observed, &mut created, true);
        }
        if let Some(v) = act.get("value") {
            observe(v, sig, &mut cg, &mut observed, &mut created, true)
        }
        if act["kind"] == "union" && a(&act["args"]).iter().all(|x| pure(x, sig)) {
            cg.equal(&act["args"][0], &act["args"][1]);
        }
    }
    cg.close();
    let mut reps = BTreeMap::new();
    for (i, e) in cg.expr.iter().enumerate() {
        if e["kind"] == "var" {
            reps.entry(cg.root(i)).or_insert(i);
        }
    }
    let mut rows = BTreeMap::<String, BTreeSet<Vec<usize>>>::new();
    for i in observed {
        let e = &cg.expr[i];
        let mut row = a(&e["args"])
            .iter()
            .map(|v| cg.ids[&term(v)])
            .collect::<Vec<_>>();
        row.push(i);
        let row = row
            .into_iter()
            .map(|i| *reps.get(&cg.root(i)).unwrap_or(&i))
            .collect();
        rows.entry(s(&e["head"]).into()).or_default().insert(row);
    }
    let mut out = vec![];
    let mut emitted = BTreeSet::new();
    for view in views {
        if !view
            .body
            .iter()
            .any(|f| created.contains(s(&f["args"][1]["head"])))
            || !view
                .body
                .iter()
                .all(|f| rows.contains_key(s(&f["args"][1]["head"])))
        {
            continue;
        }
        let mut partial = vec![BTreeMap::<String, usize>::new()];
        for (depth, f) in view.body.iter().enumerate() {
            let c = &f["args"][1];
            let mut next = BTreeSet::new();
            for m in partial {
                let mut matched = BTreeSet::new();
                for row in &rows[s(&c["head"])] {
                    let mut ext = m.clone();
                    if a(&c["args"])
                        .iter()
                        .chain([&f["args"][0]])
                        .zip(row)
                        .all(|(pat, &i)| {
                            if pat["kind"] == "var" {
                                let old = ext.entry(s(&pat["name"]).into()).or_insert(i);
                                cg.root(*old) == cg.root(i)
                            } else {
                                cg.ids
                                    .get(&term(pat))
                                    .is_some_and(|p| cg.root(*p) == cg.root(i))
                            }
                        })
                    {
                        matched.insert(ext);
                    }
                }
                if depth == 0 || matched.len() == 1 {
                    next.extend(matched);
                }
            }
            partial = next.into_iter().collect();
            if partial.is_empty() {
                break;
            }
        }
        for m in partial {
            let params = (0..view.sorts.len())
                .map(|i| cg.expr[m[&format!("{SLOT}{i}")]].clone())
                .collect::<Vec<_>>();
            if params.iter().all(|x| pure(x, sig)) {
                let w = write(&view.name, params);
                if emitted.insert(action(&w)) {
                    out.push(w);
                }
            }
        }
    }
    out
}
