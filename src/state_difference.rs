//! Explain one supplied pair of local states. No rule synthesis or pair search.
use crate::saturated_rule_composition::{Row, SaturatedRuleComposition as State};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

struct Alignment<'a> {
    a: &'a State,
    b: &'a State,
    ab: Vec<Option<usize>>,
    ba: Vec<Option<usize>>,
    users_a: Vec<Vec<usize>>,
    users_b: Vec<Vec<usize>>,
    queue: VecDeque<(bool, usize)>,
}
impl Alignment<'_> {
    fn bind(&mut self, a: usize, b: usize) -> Result<(), String> {
        if a >= self.ab.len() || b >= self.ba.len() {
            return Err("anchor outside state".into());
        }
        if self.a.values[a] != self.b.values[b] {
            return Err("anchor type/literal conflict".into());
        }
        if self.ab[a].is_some_and(|v| v != b) || self.ba[b].is_some_and(|v| v != a) {
            return Err("anchor equality/alias conflict".into());
        }
        if self.ab[a].is_none() {
            self.ab[a] = Some(b);
            self.ba[b] = Some(a);
            self.queue
                .extend(self.users_a[a].iter().map(|r| (true, *r)));
            self.queue
                .extend(self.users_b[b].iter().map(|r| (false, *r)));
        }
        Ok(())
    }
}
fn users(s: &State) -> Vec<Vec<usize>> {
    let mut out = vec![vec![]; s.values.len()];
    for (i, r) in s.rows.iter().enumerate() {
        for &v in &r.args {
            out[v].push(i);
        }
    }
    out
}
fn keys(s: &State) -> HashMap<(String, Vec<usize>), Option<usize>> {
    let mut out = HashMap::new();
    for r in &s.rows {
        out.entry((r.op.clone(), r.args.clone()))
            .and_modify(|v: &mut Option<usize>| {
                if *v != Some(r.result) {
                    *v = None;
                }
            })
            .or_insert(Some(r.result));
    }
    out
}
/// Equalities are an explicit/derived alignment, not extra unions applied to
/// either graph. Unknown bindings remain unknown. No saturation claim is made.
pub fn explain(a: &State, b: &State, anchors: &[(usize, usize)]) -> Result<Value, String> {
    a.validate().map_err(|e| e.to_string())?;
    b.validate().map_err(|e| e.to_string())?;
    if a.scope != b.scope {
        return Ok(json!({"status":"incompatible_scope"}));
    }
    if a.ports.keys().collect::<Vec<_>>() != b.ports.keys().collect::<Vec<_>>() {
        return Ok(json!({"status":"incompatible_ports"}));
    }
    let mut alignment = Alignment {
        a,
        b,
        ab: vec![None; a.values.len()],
        ba: vec![None; b.values.len()],
        users_a: users(a),
        users_b: users(b),
        queue: (0..a.rows.len())
            .map(|i| (true, i))
            .chain((0..b.rows.len()).map(|i| (false, i)))
            .collect(),
    };
    for (name, &av) in &a.ports {
        let bv = b.ports[name];
        if let Err(reason) = alignment.bind(av, bv) {
            return Ok(
                json!({"status":"interface_conflict","port":name,"left":av,"right":bv,"reason":reason}),
            );
        }
    }
    for &(av, bv) in anchors {
        alignment.bind(av, bv)?;
    }
    // Only unique, identical typed scalar literals act as automatic anchors.
    let mut literals_a = BTreeMap::<_, Vec<_>>::new();
    let mut literals_b = BTreeMap::<_, Vec<_>>::new();
    for (i, v) in a.values.iter().enumerate() {
        if let Some(l) = &v.literal {
            literals_a.entry((&v.sort, l)).or_default().push(i);
        }
    }
    for (i, v) in b.values.iter().enumerate() {
        if let Some(l) = &v.literal {
            literals_b.entry((&v.sort, l)).or_default().push(i);
        }
    }
    for (label, vs) in literals_a {
        if vs.len() == 1 {
            if let Some(ws) = literals_b.get(&label) {
                if ws.len() == 1 {
                    alignment.bind(vs[0], ws[0])?;
                }
            }
        }
    }
    let ka = keys(a);
    let kb = keys(b);
    let mut examined = 0;
    let mut conflicts = BTreeSet::new();
    while let Some((left, i)) = alignment.queue.pop_front() {
        examined += 1;
        let (row, map, target) = if left {
            (&a.rows[i], &alignment.ab, &kb)
        } else {
            (&b.rows[i], &alignment.ba, &ka)
        };
        let Some(args) = row.args.iter().map(|v| map[*v]).collect::<Option<Vec<_>>>() else {
            continue;
        };
        if let Some(Some(result)) = target.get(&(row.op.clone(), args)) {
            let (av, bv) = if left {
                (row.result, *result)
            } else {
                (*result, row.result)
            };
            if let Err(reason) = alignment.bind(av, bv) {
                conflicts.insert((av, bv, reason));
            }
        }
    }
    let hidden_a: BTreeSet<_> = a.subsumed_rows.iter().copied().collect();
    let hidden_b: BTreeSet<_> = b.subsumed_rows.iter().copied().collect();
    let rows_b: BTreeMap<_, _> = b
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.clone(), i))
        .collect();
    let mut seen = BTreeSet::new();
    let mut left_only = vec![];
    let mut unresolved_left = vec![];
    let mut visibility = vec![];
    for (i, r) in a.rows.iter().enumerate() {
        let mapped = r
            .args
            .iter()
            .map(|v| alignment.ab[*v])
            .collect::<Option<Vec<_>>>()
            .zip(alignment.ab[r.result]);
        if let Some((args, result)) = mapped {
            let row = Row {
                op: r.op.clone(),
                args,
                result,
            };
            if let Some(&j) = rows_b.get(&row) {
                seen.insert(j);
                if hidden_a.contains(&i) != hidden_b.contains(&j) {
                    visibility.push(json!({"left_row":i,"right_row":j,"left_hidden":hidden_a.contains(&i),"right_hidden":hidden_b.contains(&j)}));
                }
            } else {
                left_only.push(json!({"source_row":i,"row_in_right_variables":row,"hidden":hidden_a.contains(&i)}));
            }
        } else {
            unresolved_left.push(i);
        }
    }
    let mut right_only = vec![];
    let mut unresolved_right = vec![];
    for (i, r) in b.rows.iter().enumerate().filter(|(i, _)| !seen.contains(i)) {
        if r.args
            .iter()
            .chain(std::iter::once(&r.result))
            .all(|v| alignment.ba[*v].is_some())
        {
            right_only.push(json!({"source_row":i,"row":r,"hidden":hidden_b.contains(&i)}));
        } else {
            unresolved_right.push(i);
        }
    }
    let complete = alignment.ab.iter().all(Option::is_some)
        && alignment.ba.iter().all(Option::is_some)
        && conflicts.is_empty();
    let same = complete && left_only.is_empty() && right_only.is_empty() && visibility.is_empty();
    Ok(
        json!({"status":if same{"same_contents"}else if complete{"different_contents"}else{"partial_alignment"},"left_to_right":alignment.ab,"result_alias_conflicts":conflicts,"left_only":left_only,"right_only":right_only,"visibility_changes":visibility,"unresolved_left_rows":unresolved_left,"unresolved_right_rows":unresolved_right,"rows_examined":examined,"closure_equivalence":"not_certified_by_difference_analysis","obligation":"left must derive right_only; right must derive left_only; visibility/equality changes need separate justification"}),
    )
}
