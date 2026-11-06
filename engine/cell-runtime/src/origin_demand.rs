//! A Unit receipt with an old-value merge needs its key and row identity
//! through rebuild. Its creating frame is needed only if executable code
//! can consume that table. No generated table names participate in the proof.
use crate::program::{Call, Op, Program};
pub(crate) fn required(p: &Program) -> Vec<bool> {
    let mut readers = vec![false; p.tables.len()];
    for r in &p.rules {
        for a in r.query.iter() {
            if let Call::Table(t) = a.call {
                readers[t] = true;
            }
        }
    }
    for rule in &p.rules {
        for op in p.templates[rule.template].ops.iter() {
            if let Op::Let(_, Call::Table(t), _) = op {
                readers[*t] = true;
            }
        }
    }
    fn merge_reads(j: &serde_json::Value, p: &Program, readers: &mut [bool]) {
        if let Some(name) = j["call"]["table"].as_str()
            && let Some(t) = p.tables.iter().position(|t| t.name == name)
        {
            readers[t] = true;
        }
        match j {
            serde_json::Value::Array(xs) => {
                for x in xs {
                    merge_reads(x, p, readers)
                }
            }
            serde_json::Value::Object(xs) => {
                for x in xs.values() {
                    merge_reads(x, p, readers)
                }
            }
            _ => (),
        }
    }
    for t in &p.tables {
        merge_reads(&t.merge, p, &mut readers);
    }
    p.tables
        .iter()
        .enumerate()
        .map(|(i, t)| {
            readers[i]
                || t.virtual_fact
                || t.schema.last().is_none_or(|s| s["kind"] != "Unit")
                || t.merge["var"] != "old"
        })
        .collect()
}
