use crate::Result;
use egg_layout::egg_source::quote;
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
pub fn a(j: &J) -> &[J] {
    j.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn s(j: &J) -> &str {
    j.as_str().unwrap_or("")
}
pub fn var(n: &str) -> J {
    json!({"kind":"var","name":n})
}
pub fn call(n: &str, args: Vec<J>) -> J {
    json!({"kind":"call","head":n,"args":args})
}
pub fn term(v: &J) -> String {
    match s(&v["kind"]) {
        "var" => s(&v["name"]).into(),
        "literal" if v["literal_kind"] == "string" => quote(s(&v["value"])),
        "literal" => s(&v["text"]).into(),
        "call" => form(s(&v["head"]), a(&v["args"]).iter().map(term)),
        k => panic!("invalid native AST expression {k}"),
    }
}
pub fn form(n: &str, args: impl IntoIterator<Item = String>) -> String {
    let mut out = format!("({n}");
    for a in args {
        out.push(' ');
        out.push_str(&a);
    }
    out.push(')');
    out
}
pub fn fact(f: &J) -> String {
    if f["kind"] == "eq" {
        form("=", a(&f["args"]).iter().map(term))
    } else {
        term(&f["expr"])
    }
}
pub fn action(v: &J) -> String {
    let h = s(&v["head"]);
    let args = || a(&v["args"]).iter().map(term);
    match s(&v["kind"]) {
        "expr" => term(&v["expr"]),
        "let" => format!("(let {} {})", s(&v["name"]), term(&v["expr"])),
        "set" => format!("(set {} {})", form(h, args()), term(&v["value"])),
        "union" => form("union", args()),
        "panic" => form("panic", [quote(s(&v["message"]))]),
        "change" => form(&s(&v["change"]).to_lowercase(), [form(h, args())]),
        k => panic!("unknown native action {k}"),
    }
}
pub fn rule(r: &J, body: &[J], head: &[J], set: &str) -> String {
    format!(
        "(rule ({}) ({}){} :name {}{})",
        body.iter().map(fact).collect::<Vec<_>>().join(" "),
        head.iter().map(action).collect::<Vec<_>>().join(" "),
        if set.is_empty() {
            String::new()
        } else {
            format!(" :ruleset {set}")
        },
        quote(s(&r["name"])),
        if r["naive"] == true { " :naive" } else { "" }
    )
}
pub fn vars(j: &J, out: &mut BTreeSet<String>) {
    if j["kind"] == "var" {
        out.insert(s(&j["name"]).to_owned());
    } else if let Some(xs) = j.as_array() {
        for x in xs {
            vars(x, out)
        }
    } else if let Some(xs) = j.as_object() {
        for x in xs.values() {
            vars(x, out)
        }
    }
}
pub fn subst(j: &J, m: &BTreeMap<String, J>) -> J {
    if j["kind"] == "var" {
        if let Some(v) = m.get(s(&j["name"])) {
            return v.clone();
        }
    }
    match j {
        J::Array(xs) => J::Array(xs.iter().map(|x| subst(x, m)).collect()),
        J::Object(xs) => J::Object(xs.iter().map(|(k, x)| (k.clone(), subst(x, m))).collect()),
        _ => j.clone(),
    }
}
pub fn active(data: &J) -> BTreeSet<String> {
    fn scan(j: &J, set: &mut BTreeSet<String>) {
        if j["kind"] == "repeat" && j["count"] == 0 {
            return;
        }
        if j["kind"] == "run" {
            set.insert(s(&j["ruleset"]).into());
        }
        for c in a(&j["children"]) {
            scan(c, set)
        }
        if !j["child"].is_null() {
            scan(&j["child"], set)
        }
    }
    let mut r = BTreeSet::new();
    for c in a(&data["commands"]) {
        scan(&c["schedule"], &mut r)
    }
    loop {
        let n = r.len();
        for k in r.clone() {
            r.extend(
                a(&data["combined_rulesets"][k])
                    .iter()
                    .map(|s| s.as_str().unwrap().to_owned()),
            );
        }
        if r.len() == n {
            return r;
        }
    }
}
pub fn schedule(j: &J, wrap: &impl Fn(&str, &str) -> String) -> String {
    match s(&j["kind"]) {
        "run" => wrap(s(&j["ruleset"]), s(&j["text"])),
        "seq" => form("seq", a(&j["children"]).iter().map(|c| schedule(c, wrap))),
        "repeat" => format!("(repeat {} {})", j["count"], schedule(&j["child"], wrap)),
        "saturate" => form("saturate", [schedule(&j["child"], wrap)]),
        v => panic!("invalid schedule {v}"),
    }
}
pub fn normalize(name: &str, text: &str) -> Result<(J, String)> {
    let mut e = egglog::EGraph::default();
    let mut lines = vec![];
    let mut i = 0;
    for c in egg_layout::visual_rule::surface_program(e.parse_program(Some(name.into()), text)?) {
        if matches!(
            &c,
            egglog::ast::Command::Rewrite(_, _, true)
                | egglog::ast::Command::BiRewrite(..)
                | egglog::ast::Command::Include(..)
                | egglog::ast::Command::Push(_)
                | egglog::ast::Command::Pop(_, _)
                | egglog::ast::Command::Input { .. }
        ) {
            return Err(
                "CPU compiler requires a self-contained flat program with supported rewrites"
                    .into(),
            );
        }
        let c = egg_layout::visual_rule::normalize(c, i);
        if matches!(&c, egglog::ast::Command::Rule { .. }) {
            i += 1
        }
        lines.push(egg_layout::egg_source::printed(&c));
    }
    let normalized = lines.join("\n");
    Ok((
        egg_layout::source_ast::export(name, &normalized)?,
        normalized,
    ))
}
