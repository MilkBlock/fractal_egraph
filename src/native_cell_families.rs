//! Observed re-entry families. The only input is analyze's captured evidence.
//! No parsing a source file, EGraph queries, ripening, or speculative rule edges.
//! A family annotates observed dependency paths; it never authorizes a Cell union.
use super::super::*;

#[derive(Clone)]
struct Event {
    record: usize,
    event: u64,
    kind: usize,
    boundary: usize,
    scope: usize,
    values: Json,
    parents: Vec<(usize, usize)>, // producer record, binding-contract id
}

#[cfg(test)]
mod tests {
    use super::*;
    fn add(c: &mut Census, record: usize, kind: usize, parents: &[(usize, usize)]) {
        c.push(Event {
            record,
            event: record as u64,
            kind,
            boundary: record + 1,
            scope: 0,
            values: json!({"inputs":[],"outputs":[]}),
            parents: parents.to_vec(),
        });
    }
    #[test]
    fn dabcabc_requires_connected_observed_paths_and_keeps_rotations() {
        let mut c = Census::default();
        for (r, k, ps) in [
            (0, 0, vec![]),
            (1, 1, vec![(0, 9)]),
            (2, 2, vec![(1, 0)]),
            (3, 3, vec![(2, 1)]),
            (4, 1, vec![(3, 2)]),
            (5, 2, vec![(4, 0)]),
            (6, 3, vec![(5, 1)]),
        ] {
            add(&mut c, r, k, &ps);
        }
        assert_eq!(c.families.len(), 1);
        let f = c.families.values().next().unwrap();
        assert_eq!(f["binding_cycle"], json!([0, 1, 2]));
        assert_eq!(f["occurrences"].as_array().unwrap().len(), 3);
        assert_eq!(f["occurrences"][0]["start_record"], 1);
        assert_eq!(f["occurrences"][0]["end_record"], 4);
    }
    #[test]
    fn repeated_names_disconnected_edges_and_different_contracts_are_not_reentry() {
        let mut c = Census::default();
        add(&mut c, 0, 0, &[]);
        add(&mut c, 1, 1, &[(0, 0)]);
        add(&mut c, 2, 1, &[]);
        add(&mut c, 3, 0, &[(2, 1)]);
        assert!(
            c.families.is_empty(),
            "type-level cycle is not a witnessed path"
        );
        add(&mut c, 4, 2, &[(3, 2)]);
        assert!(
            c.families.is_empty(),
            "changed bindings/effects use distinct contracts"
        );
    }
    #[test]
    fn branches_retain_real_connections_without_cartesian_path_expansion() {
        let mut c = Census::default();
        add(&mut c, 0, 0, &[]);
        add(&mut c, 1, 0, &[(0, 0)]);
        add(&mut c, 2, 0, &[(0, 1)]);
        assert_eq!(
            c.families.len(),
            2,
            "different binding positions must not collapse"
        );
        add(&mut c, 3, 0, &[(1, 0), (2, 1)]);
        assert_eq!(c.events[&3].parents.len(), 2);
        assert_eq!(
            c.families
                .values()
                .map(|f| f["occurrences"].as_array().unwrap().len())
                .sum::<usize>(),
            4
        );
    }
    #[test]
    fn empty_history_yields_no_family_or_certificate() {
        let c = Census::default();
        let report = c.report(&[], &BTreeSet::new());
        assert_eq!(report["families"], json!([]));
        assert_eq!(report["processed_records"], 0);
    }
}

#[derive(Default)]
pub(super) struct Census {
    seen: usize,
    events: BTreeMap<usize, Event>,
    kinds: Vec<Json>,
    kind_keys: BTreeMap<String, usize>,
    bindings: Vec<Json>,
    binding_keys: BTreeMap<String, usize>,
    families: BTreeMap<String, Json>,
    unsupported: BTreeMap<String, usize>,
    visited: usize,
    units: BTreeMap<usize, BTreeSet<usize>>,
    seen_kinds: BTreeSet<usize>,
}

fn intern(value: Json, values: &mut Vec<Json>, keys: &mut BTreeMap<String, usize>) -> usize {
    let key = value.to_string();
    if let Some(&i) = keys.get(&key) {
        return i;
    }
    let i = values.len();
    values.push(value);
    keys.insert(key, i);
    i
}

fn expression(e: &Expr, slots: &BTreeMap<String, usize>) -> Json {
    match e {
        Expr::Var(_, n) => match slots.get(n) {
            Some(i) => json!({"input":i}),
            None => json!({"local_variable":n}),
        },
        Expr::Lit(_, v) => json!({"literal":v.to_string()}),
        Expr::Call(_, op, args) => {
            json!({"operator":op,"arguments":args.iter().map(|a|expression(a,slots)).collect::<Vec<_>>()})
        }
    }
}

fn fact(f: &Fact, slots: &BTreeMap<String, usize>) -> Json {
    match f {
        Fact::Eq(_, a, b) => json!({"equal":[expression(a,slots),expression(b,slots)]}),
        Fact::Fact(e) => expression(e, slots),
    }
}

fn role(r: &RuleInfo, o: &Output, slots: &BTreeMap<String, usize>) -> Json {
    match o {
        Output::Var(n) => {
            json!({"variable":n,"expression":slots.get(n).map(|i|json!({"input":i}))})
        }
        Output::Column(span, col) => {
            let call = r.calls.get(span);
            let expr = call.and_then(|(_, e)| match e {
                Expr::Call(_, _, args) if *col < args.len() => Some(expression(&args[*col], slots)),
                Expr::Call(_, _, args) if *col == args.len() => Some(expression(e, slots)),
                _ => None,
            });
            json!({"path":call.map(|x|&x.0),"column":col,"expression":expr})
        }
        Output::Row(span) => json!({"row":r.calls.get(span).map(|x|&x.0)}),
    }
}

impl Census {
    pub fn observe(&mut self, c: &Captured, store: &cs::Store) {
        // Anchor to the construction's final apply, not every larger unit that
        // happens to contain some earlier member of the observed path.
        for (u, unit) in store.units.iter().enumerate() {
            if let Some(comp) = unit.source_composition {
                self.units
                    .entry(store.compositions[comp].consumer)
                    .or_default()
                    .insert(u);
            }
        }
        for w in &store.windows {
            self.units.entry(w.consumer).or_default().insert(w.unit);
        }
        for i in self.seen..c.records.len() {
            let r = &c.records[i];
            let rule = &c.rules[r.rule];
            if rule
                .rule
                .head
                .0
                .iter()
                .any(|a| matches!(a, Action::Change(..)))
            {
                *self
                    .unsupported
                    .entry("visibility effects lack family lineage".into())
                    .or_default() += 1;
                continue;
            }
            let boundary = c.boundaries.partition_point(|b| b.end <= i) + 1;
            let scope = c
                .changes
                .iter()
                .filter(|x| x.reset && x.boundary <= boundary)
                .count();
            if r.produced.is_empty() && r.unions.is_empty() && !r.updated_rows {
                *self
                    .unsupported
                    .entry("no supported insertion/union/update evidence".into())
                    .or_default() += 1;
                continue;
            }
            let slots: BTreeMap<_, _> = r
                .inputs
                .iter()
                .enumerate()
                .filter_map(|(i, x)| match x {
                    Input::Var(n) => Some((n.clone(), i)),
                    _ => None,
                })
                .collect();
            let outputs: Vec<_> = r.outputs.iter().map(|o| role(rule, o, &slots)).collect();
            let inputs: Vec<_> = r
                .inputs
                .iter()
                .map(|o| match o {
                    Input::Var(n) => json!({"variable":n}),
                    Input::Read(span) => json!({"read":rule.calls.get(span).map(|x|&x.0)}),
                })
                .collect();
            let mut aliases = BTreeMap::new();
            let mut name = |v: usize| {
                let next = aliases.len();
                *aliases.entry(v).or_insert(next)
            };
            let input_aliases: Vec<_> = r.wanted.iter().map(|v| name(*v)).collect();
            let output_aliases: Vec<_> = r.values.iter().map(|v| name(*v)).collect();
            let produced: Vec<_> = r.produced.iter().map(|v| name(*v)).collect();
            let unions: Vec<_> = r.unions.iter().map(|(a, b)| (name(*a), name(*b))).collect();
            let parent_rules: Vec<_> = r.parents.iter().map(|p| c.records[*p].rule).collect();
            let signature = json!({"rule":r.rule,"rule_name":rule.rule.name,"ruleset":rule.rule.ruleset,
                "body":rule.rule.body.iter().map(|f|fact(f,&slots)).collect::<Vec<_>>(),
                "inputs":inputs,"outputs":outputs,
                "input_sorts":r.wanted.iter().map(|v|&c.pool.values[*v].sort).collect::<Vec<_>>(),
                "output_sorts":r.values.iter().map(|v|&c.pool.values[*v].sort).collect::<Vec<_>>(),
                "input_aliases":input_aliases,"output_aliases":output_aliases,
                "bindings":r.ports,"parent_rules":parent_rules,"new_facts":produced,"unions":unions,
                // Updates carry concrete before/after evidence; do not erase it.
                "updates":r.sets,"updated_rows":r.updated_rows,
                "actions":rule.rule.head.0.iter().map(ToString::to_string).collect::<Vec<_>>()});
            let kind = intern(signature, &mut self.kinds, &mut self.kind_keys);
            let values = json!({"inputs":r.wanted.iter().map(|v|json!({"token":v,"sort":c.pool.values[*v].sort,"literal":c.pool.literals.get(v)})).collect::<Vec<_>>(),
                "outputs":r.values.iter().map(|v|json!({"token":v,"sort":c.pool.values[*v].sort,"literal":c.pool.literals.get(v)})).collect::<Vec<_>>()});
            let mut parents = vec![];
            for (parent_slot, &p) in r.parents.iter().enumerate() {
                let Some(prior) = self.events.get(&p) else {
                    continue;
                };
                if prior.scope != scope {
                    *self
                        .unsupported
                        .entry("scope/reset boundary".into())
                        .or_default() += 1;
                    continue;
                }
                let pr = &c.records[p];
                let links: Vec<_> = r
                    .ports
                    .iter()
                    .enumerate()
                    .filter_map(|(to, port)| match port {
                        Port::Parent(which, from) if *which == parent_slot => Some((*from, to)),
                        _ => None,
                    })
                    .collect();
                // A numeric value match alone is not a producer certificate.
                let witnessed = links.iter().any(|(from, to)| {
                    matches!(r.inputs[*to], Input::Read(_))
                        && matches!(pr.outputs[*from], Output::Row(_))
                        && pr.produced.contains(&r.wanted[*to])
                        && pr.values[*from] == r.wanted[*to]
                });
                if !witnessed {
                    *self
                        .unsupported
                        .entry("missing versioned row link".into())
                        .or_default() += 1;
                    continue;
                }
                let key = json!({"source_contract":prior.kind,"target_contract":kind,"parent_slot":parent_slot,
                    "links":links.iter().map(|(from,to)|json!({"from":from,"to":to,
                        "source":self.kinds[prior.kind]["outputs"][*from],"target":inputs[*to]})).collect::<Vec<_>>(),
                    "scope":"exact recorded row producer; all other parent and external ports retained in target contract"});
                let binding = intern(key, &mut self.bindings, &mut self.binding_keys);
                parents.push((p, binding));
            }
            self.push(Event {
                record: i,
                event: r.id,
                kind,
                boundary,
                scope,
                values,
                parents,
            });
        }
        self.seen = c.records.len();
    }

    fn push(&mut self, event: Event) {
        let target = event.record;
        self.events.insert(target, event.clone());
        if self.seen_kinds.insert(event.kind) {
            return;
        }
        // Search recorded ancestors, never source-rule call graphs or Cell pairs.
        // One path per reached start/end is evidence, not all-path enumeration.
        let mut queue = std::collections::VecDeque::from([target]);
        let mut visited = BTreeSet::from([target]);
        let mut next = BTreeMap::<usize, (usize, usize)>::new();
        while let Some(current) = queue.pop_front() {
            self.visited += 1;
            let e = &self.events[&current];
            if current != target && e.kind == event.kind {
                let mut path = vec![];
                let mut at = current;
                while at != target {
                    let (child, binding) = next[&at];
                    path.push((at, child, binding));
                    at = child;
                }
                let steps: Vec<_> = path.iter().map(|(_, _, b)| *b).collect();
                // Rotate a witnessed loop; never reorder its binding operations.
                let canonical = (0..steps.len())
                    .map(|k| {
                        steps[k..]
                            .iter()
                            .chain(&steps[..k])
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .min()
                    .unwrap();
                let key = json!(canonical).to_string();
                let next = self.families.len();
                let family=self.families.entry(key).or_insert_with(||json!({"id":next,"binding_cycle":canonical,
                    "status":"observed-reentry","occurrences":[],"guarantee":"finite observed dependency paths only; no universal recursion, termination, saturation or Cell equivalence proof"}));
                family["occurrences"].as_array_mut().unwrap().push(json!({"start_record":current,"end_record":target,
                    "start_event":e.event,"end_event":event.event,"boundary":event.boundary,
                    "steps":path.iter().map(|(a,b,k)|json!({"source_record":a,"target_record":b,"binding":k})).collect::<Vec<_>>() }));
                continue;
            }
            for &(p, binding) in &e.parents {
                if p >= current || !visited.insert(p) {
                    continue;
                }
                next.insert(p, (current, binding));
                queue.push_back(p);
            }
        }
    }

    pub fn memberships(&self) -> BTreeSet<(usize, usize)> {
        self.families
            .values()
            .flat_map(|f| {
                let id = f["id"].as_u64().unwrap() as usize;
                f["occurrences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(move |o| {
                        o["steps"].as_array().unwrap().iter().flat_map(move |s| {
                            [
                                s["source_record"].as_u64().unwrap() as usize,
                                s["target_record"].as_u64().unwrap() as usize,
                            ]
                            .into_iter()
                            .flat_map(move |r| {
                                self.units
                                    .get(&r)
                                    .into_iter()
                                    .flatten()
                                    .map(move |u| (id, *u))
                            })
                        })
                    })
            })
            .collect()
    }

    pub fn report(&self, units: &[Json], completed: &BTreeSet<usize>) -> Json {
        let classes: BTreeMap<_, _> = units
            .iter()
            .filter_map(|u| Some((u["unit"].as_u64()? as usize, u["eclass"].clone())))
            .collect();
        let mut families: Vec<_> = self.families.values().cloned().collect();
        families.sort_by_key(|f| f["id"].as_u64());
        let mut records = BTreeSet::new();
        for f in &mut families {
            let members: BTreeSet<_> = f["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|o| {
                    o["steps"].as_array().unwrap().iter().flat_map(|s| {
                        [
                            s["source_record"].as_u64().unwrap() as usize,
                            s["target_record"].as_u64().unwrap() as usize,
                        ]
                    })
                })
                .collect();
            records.extend(&members);
            let us: BTreeSet<_> = members
                .iter()
                .flat_map(|r| self.units.get(r).into_iter().flatten().copied())
                .filter(|u| classes.contains_key(u))
                .collect();
            f["units"] = json!(us);
            f["eclasses"] = json!(us.iter().map(|u| classes[u].clone()).collect::<Vec<_>>());
            f["certified_saturated_units"] =
                json!(us.intersection(completed).copied().collect::<Vec<_>>());
            f["records"] = json!(members);
            f["rules"] = json!(
                members
                    .iter()
                    .map(|r| self.kinds[self.events[r].kind]["rule_name"]
                        .as_str()
                        .unwrap_or(""))
                    .collect::<BTreeSet<_>>()
            );
        }
        let evidence:Vec<_>=records.iter().map(|r| {
            let e=&self.events[r];json!({"record":r,"event":e.event,"contract":e.kind,"boundary":e.boundary,
                "scope_epoch":e.scope,"values":e.values,"producer_links":e.parents,
                "units":self.units.get(r).into_iter().flatten().filter(|u|classes.contains_key(u)).collect::<Vec<_>>()})
        }).collect();
        json!({"schema":"analyze-cell-reentry/v1","source":"analyze captured committed applications",
            "allowed_inputs":["recorded bindings","versioned row producer links","committed effects","semantics of executed rules","registered Cell membership"],
            "forbidden_inputs":["whole-source candidate scanning","fresh match queries","future snapshots","unobserved rule connections"],
            "families":families,"events":evidence,"contracts":self.kinds,"binding_contracts":self.bindings,
            "processed_records":self.seen,"ancestor_visits":self.visited,"unsupported":self.unsupported,
            "membership_scope":"constructions rooted at an apply on an observed path; no whole-Cell equivalence implied",
            "scope":"observation annotation only; no Cell unions or runtime admission; one causal path per start/end, not exhaustive motif mining"})
    }
}
