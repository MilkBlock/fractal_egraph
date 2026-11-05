use crate::{Result, surface::*};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
const H: &str = "cell-stable-keys";
const CHECK: &str = "cell-key-ready-check";
pub use eggfuse_bake::structural_key::source::HEADER;
fn fun(n: &str, inputs: &[String], out: &str, merge: &str) -> String {
    format!(
        "(function {n} ({}) {out} :merge {merge} :unextractable)",
        inputs.join(" ")
    )
}
fn helper(b: &[String], h: &[String], rs: &str) -> String {
    format!("(rule ({}) ({}) :ruleset {rs})", b.join(" "), h.join(" "))
}
fn need(i: usize, sort: &str, v: &str) -> Vec<String> {
    vec![
        format!("(CellChoiceNeed{i} {v})"),
        format!("(set (CellChoiceReady{i} {v}) false)"),
        format!("(CellKeyNeed-{sort} {v})"),
    ]
}
pub fn rewrite(name: &str, original: &str) -> Result<String> {
    if [
        "__sk_",
        "CellStructuralKey",
        "cell-key-",
        "CellKeyNeed-",
        "CellChoiceMap",
        "CellChoiceNeed",
        "CellChoiceReady",
        H,
    ]
    .iter()
    .any(|n| original.contains(n))
    {
        return Err("reserved structural-key namespace".into());
    }
    let (data, normalized) = normalize(name, original)?;
    let sig = data["signatures"].as_object().unwrap();
    let active = active(&data);
    let mut choices = BTreeMap::new();
    let mut selected = vec![];
    let mut consumers = BTreeSet::new();
    let mut live = BTreeSet::new();
    let mut declared = BTreeSet::new();
    for line in original
        .lines()
        .filter_map(|l| l.strip_prefix("; cell-choice/v1 "))
    {
        let mut c: J = serde_json::from_str(line)?;
        let n = c["function"]
            .as_str()
            .ok_or("choice function missing")?
            .to_owned();
        if !declared.insert(n.clone())
            || !c
                .as_object()
                .unwrap()
                .keys()
                .all(|k| matches!(k.as_str(), "function" | "score" | "order" | "tie_break"))
            || c["tie_break"] != "structural"
        {
            return Err("invalid or duplicated structural choice".into());
        }
        if c.get("order").is_none() {
            c["order"] = json!("max");
        }
        if !matches!(s(&c["order"]), "min" | "max") {
            return Err("invalid choice order".into());
        }
        let t = sig.get(&n).ok_or("unknown choice")?;
        let sort = s(&t["output"]);
        let merge = s(&t["merge"]["name"]);
        if !matches!(merge, "old" | "new")
            || t["kind"] != "function"
            || !sig
                .values()
                .any(|t| t["kind"] == "constructor" && t["output"] == sort)
        {
            return Err("choice requires an old/new reference selector".into());
        }
        if let Some(score) = c.get("score") {
            let score = score.as_str().ok_or("invalid score")?;
            let st = sig.get(score).ok_or("missing score")?;
            if st["input"] != json!([sort]) || st["output"] != "i64" {
                return Err("score signature differs".into());
            }
            let position = |name: &str| {
                a(&data["commands"])
                    .iter()
                    .position(|c| s(&c["text"]).starts_with(&format!("(function {name} ")))
            };
            if position(score) >= position(&n) {
                return Err("declare score before selector".into());
            }
            if a(&data["actions"])
                .iter()
                .map(|x| &x["action"])
                .chain(
                    a(&data["rules"])
                        .iter()
                        .filter(|r| active.contains(s(&r["ruleset"])))
                        .flat_map(|r| a(&r["head"])),
                )
                .any(|a| a["kind"] == "change" && a["head"] == score)
            {
                return Err("selected score can be deleted or subsumed".into());
            }
        }
        let mut used = a(&data["actions"])
            .iter()
            .any(|v| v["action"]["kind"] == "set" && v["action"]["head"] == n);
        for r in a(&data["rules"]) {
            let rs = s(&r["ruleset"]);
            if !active.contains(rs) {
                continue;
            }
            for act in a(&r["head"]) {
                if act["kind"] == "set" && act["head"] == n {
                    let mut bound = BTreeSet::new();
                    vars(&r["body"], &mut bound);
                    if act["value"]["kind"] != "var" || !bound.contains(s(&act["value"]["name"])) {
                        return Err("choice candidate must be LHS-bound".into());
                    }
                    used = true;
                    consumers.insert(rs.to_owned());
                }
            }
        }
        if used {
            c["sort"] = json!(sort);
            c["original_merge"] = json!(merge);
            choices.insert(n, selected.len());
            selected.push(c);
            live.insert(sort.to_owned());
        }
    }
    if !selected.is_empty()
        && a(&data["rules"])
            .iter()
            .any(|r| r["naive"] == true && active.contains(s(&r["ruleset"])))
    {
        return Err("key maintenance is not certified for active naive rules".into());
    }
    if !declared.is_empty()
        && a(&data["commands"]).iter().any(|c| {
            s(&c["text"]).starts_with("(fail ")
                && (s(&c["text"]).contains("(run") || s(&c["text"]).contains("(set "))
        })
    {
        return Err("nested fail schedule/write with structural choice".into());
    }
    if !selected.is_empty()
        && a(&data["rules"])
            .iter()
            .any(|r| r["naive"] == true && active.contains(s(&r["ruleset"])))
    {
        return Err("active naive rule with structural choice".into());
    }
    let mut bases = BTreeSet::new();
    loop {
        let n = live.len();
        for t in sig.values().filter(|t| t["kind"] == "constructor") {
            if live.contains(s(&t["output"])) {
                for v in a(&t["input"]) {
                    let v = s(v);
                    if matches!(v, "String" | "i64" | "Unit" | "f64" | "bool") {
                        bases.insert(v.to_owned());
                    } else if sig
                        .values()
                        .any(|c| c["kind"] == "constructor" && c["output"] == v)
                    {
                        live.insert(v.to_owned());
                    } else {
                        return Err(format!("unsupported structural key field {v}").into());
                    }
                }
            }
        }
        if n == live.len() {
            break;
        }
    }
    let req: BTreeMap<_, _> = consumers
        .into_iter()
        .enumerate()
        .map(|(i, k)| (k, format!("cell-key-request-{i}")))
        .collect();
    let mut request: BTreeMap<_, BTreeSet<_>> = req
        .iter()
        .map(|(k, v)| (k.clone(), BTreeSet::from([v.clone()])))
        .collect();
    loop {
        let mut changed = false;
        for (k, children) in data["combined_rulesets"].as_object().unwrap() {
            let sets: BTreeSet<_> = a(children)
                .iter()
                .flat_map(|c| request.get(s(c)).into_iter().flatten().cloned())
                .collect();
            if !sets.is_empty() && request.get(k) != Some(&sets) {
                request.insert(k.clone(), sets);
                changed = true
            }
        }
        if !changed {
            break;
        }
    }
    let sets = if live.is_empty() {
        vec![]
    } else {
        [H.to_owned(), CHECK.to_owned()]
            .into_iter()
            .chain(req.values().cloned())
            .collect()
    };
    let specs=sig.iter().filter(|(_,v)|v["kind"]=="constructor"&&live.contains(s(&v["output"]))).map(|(n,v)|json!({"primitive":format!("cell-key-node-{n}"),"label":format!("{}:{n}",s(&v["output"])),"arity":a(&v["input"]).len()})).collect::<Vec<_>>();
    let header = json!({"storage":"shared-native-dag","policy":"automatic","native_rulesets":sets,"choices":selected,"bases":bases,"nodes":specs,"key_semantics":"first complete constructor witness is anchored; union merges established keys by min; not an eclass canonical form"});
    if choices.is_empty() {
        let orig = eggfuse_bake::source_ast::export(name, original)?;
        let body = if a(&orig["rewrites"]).is_empty() {
            original
        } else {
            &normalized
        };
        return Ok(format!("{HEADER}{header}\n{body}"));
    }
    let mut lines = sets
        .iter()
        .map(|k| format!("(ruleset {k})"))
        .collect::<Vec<_>>();
    let mut defined = BTreeSet::new();
    let rules: BTreeMap<_, _> = a(&data["rules"])
        .iter()
        .map(|r| (r["command_ordinal"].as_u64().unwrap(), r))
        .collect();
    let actions: BTreeMap<_, _> = a(&data["actions"])
        .iter()
        .map(|r| (r["command_ordinal"].as_u64().unwrap(), &r["action"]))
        .collect();
    for cmd in a(&data["commands"]) {
        let id = cmd["ordinal"].as_u64().unwrap();
        let mut src = s(&cmd["text"]).to_owned();
        if let Some(r) = rules.get(&id) {
            lines.push(rule(r, a(&r["body"]), a(&r["head"]), s(&r["ruleset"])));
            if let Some(set) = req.get(s(&r["ruleset"])) {
                let mut h = vec![];
                for ac in a(&r["head"]) {
                    if ac["kind"] == "set"
                        && let Some(&i) = choices.get(s(&ac["head"]))
                    {
                        h.extend(need(i, s(&selected[i]["sort"]), &term(&ac["value"])));
                    }
                }
                if !h.is_empty() {
                    lines.push(helper(
                        &a(&r["body"]).iter().map(fact).collect::<Vec<_>>(),
                        &h,
                        set,
                    ));
                }
            }
            continue;
        }
        if cmd["kind"] == "schedule" {
            lines.push(form(
                "run-schedule",
                [schedule(
                    &cmd["schedule"],
                    &|rs, leaf| match request.get(rs) {
                        None => leaf.into(),
                        Some(xs) => form(
                            "seq",
                            xs.iter().map(|r| format!("(run {r})")).chain([
                                format!("(saturate {H})"),
                                format!("(run {CHECK})"),
                                leaf.into(),
                            ]),
                        ),
                    },
                )],
            ));
            continue;
        }
        if let Some(ac) = actions.get(&id)
            && ac["kind"] == "set"
            && let Some(&i) = choices.get(s(&ac["head"]))
        {
            let mut names = vec![];
            for (j, e) in a(&ac["args"]).iter().chain([&ac["value"]]).enumerate() {
                let v = format!("__sk_input{id}_{j}");
                lines.push(format!("(let {v} {})", term(e)));
                names.push(v)
            }
            let v = names.pop().unwrap();
            lines.extend(need(i, s(&selected[i]["sort"]), &v));
            lines.push(format!("(run-schedule (seq (saturate {H}) (run {CHECK})))"));
            lines.push(format!("(set {} {v})", form(s(&ac["head"]), names)));
            continue;
        }
        let mut words = src.split_whitespace();
        let kind = words.next().unwrap_or("");
        let decl = words.next().unwrap_or("").trim_end_matches(')').to_owned();
        if kind == "(function"
            && let Some(&i) = choices.get(&decl)
        {
            let c = &selected[i];
            let t = s(&c["sort"]);
            let (mut l, mut r) = ("0".into(), "0".into());
            if let Some(n) = c["score"].as_str() {
                l = format!("({n} old)");
                r = format!("({n} new)");
            }
            if c["order"] == "min" {
                std::mem::swap(&mut l, &mut r)
            }
            let old = format!(":merge {}", s(&c["original_merge"]));
            if src.matches(&old).count() != 1 {
                return Err("cannot locate source merge".into());
            }
            src=src.replacen(&old,&format!(":merge (cell-key-select old new (cell-key-prefer-left {l} {r} (CellStructuralKey-{t} old) (CellStructuralKey-{t} new)))"),1);
            lines.push(src);
            lines.push(format!("(relation CellChoiceNeed{i} ({t}))"));
            lines.push(fun(
                &format!("CellChoiceReady{i}"),
                &[t.into()],
                "bool",
                "(or old new)",
            ));
            let v = "__sk_choice";
            let ks = (0..a(&sig[&decl]["input"]).len()).map(|k| format!("__sk_arg{k}"));
            lines.push(helper(
                &[format!("(= {v} {})", form(&decl, ks))],
                &need(i, t, v),
                H,
            ));
            let mut b = vec![
                format!("(CellChoiceNeed{i} {v})"),
                format!("(= __sk_key (CellStructuralKey-{t} {v}))"),
            ];
            if let Some(n) = c["score"].as_str() {
                b.push(format!("(= __sk_score ({n} {v}))"))
            }
            lines.push(helper(
                &b,
                &[format!("(set (CellChoiceReady{i} {v}) true)")],
                H,
            ));
            lines.push(helper(
                &[
                    format!("(CellChoiceNeed{i} {v})"),
                    format!("(= (CellChoiceReady{i} {v}) false)"),
                ],
                &[format!(
                    "(panic \"unavailable structural key or score for {decl}\")"
                )],
                CHECK,
            ));
            continue;
        }
        lines.push(src.clone());
        let ctors = match kind {
            "(constructor" => vec![decl.clone()],
            "(datatype" => a(&data["declarations"])
                .iter()
                .find(|d| d["name"] == decl)
                .map(|d| {
                    a(&d["constructors"])
                        .iter()
                        .map(|v| s(&v["name"]).to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            "(datatypes" => {
                return Err("expand mutually recursive datatypes for structural choice".into());
            }
            _ => vec![],
        };
        if matches!(kind, "(sort" | "(datatype")
            && live.contains(&decl)
            && defined.insert(decl.clone())
        {
            let k = format!("CellStructuralKey-{decl}");
            lines.extend([
                fun(
                    &k,
                    std::slice::from_ref(&decl),
                    "CellStructuralKey",
                    "(cell-key-min old new)",
                ),
                format!("(relation CellKeyNeed-{decl} ({decl}))"),
                helper(
                    &[format!("(= __sk_k ({k} __sk_x))")],
                    &[format!("(set ({k} __sk_x) (cell-key-anchor __sk_k))")],
                    H,
                ),
            ]);
        }
        for n in ctors {
            let t = s(&sig[&n]["output"]);
            if !live.contains(t) {
                continue;
            }
            let xs = (0..a(&sig[&n]["input"]).len())
                .map(|i| format!("__sk_x{i}"))
                .collect::<Vec<_>>();
            let body = vec![
                format!("(CellKeyNeed-{t} __sk_out)"),
                format!("(= __sk_out {})", form(&n, xs.clone())),
            ];
            let mut b = body.clone();
            let (mut keys, mut children) = (vec![], vec![]);
            for (j, (sort, x)) in a(&sig[&n]["input"]).iter().zip(xs).enumerate() {
                let sort = s(sort);
                if live.contains(sort) {
                    b.push(format!("(= __sk_k{j} (CellStructuralKey-{sort} {x}))"));
                    keys.push(format!("__sk_k{j}"));
                    children.push(format!("(CellKeyNeed-{sort} {x})"));
                } else {
                    keys.push(format!("(cell-key-base-{sort} {x})"));
                }
            }
            if !children.is_empty() {
                lines.push(helper(&body, &children, H))
            }
            lines.push(helper(
                &b,
                &[format!(
                    "(set (CellStructuralKey-{t} __sk_out) {})",
                    form(&format!("cell-key-node-{n}"), keys)
                )],
                H,
            ));
        }
    }
    Ok(format!("{HEADER}{header}\n{}\n", lines.join("\n")))
}
