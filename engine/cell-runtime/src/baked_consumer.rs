//! A constructor terminal specialized to one executed composition in a frozen catalog.
use super::{Dependency, FactRef, Graph};
use crate::program::{Arg, Call, Op, Program, Result};
use crate::slots::Slots;
use crate::value::{Value, canon};
use rustc_hash::FxHashMap;
use serde::Serialize;
use smallvec::SmallVec;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub(crate) struct Plan {
    pub unit: usize,
    pub rule: usize,
    pub pivot: usize,
    pub op: usize,
    pub table: usize,
    dst: usize,
    key_ports: Vec<Arg>,
    retained_slots: Vec<usize>,
    pub computed: Vec<(usize, usize)>,
}

impl Plan {
    pub fn derive(p: &Program, unit: usize) -> Result<Self> {
        let (rid, pivot) = p
            .iterative
            .consumer_entry(unit)
            .ok_or("expected a composed Bake unit with an active switch")?;
        let rule = &p.rules[rid];
        let t = &p.templates[rule.template];
        let (tail, prefix) = t.ops.split_last().ok_or("empty consumer RHS")?;
        let Op::Let(dst, Call::Table(table), key) = tail else {
            return Err("expected a terminal constructor".into());
        };
        if !p.tables[*table].constructor
            || !p.tables[*table].virtual_fact
            || p.known_results.static_skip[rid][prefix.len()]
        {
            return Err("terminal must be an executed constructor".into());
        }
        let mut ports: BTreeMap<_, _> = rule
            .inputs
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, Arg::Slot(i)))
            .collect();
        let resolve = |a: &Arg, m: &BTreeMap<usize, Arg>| match a {
            Arg::Literal(_) => Some(a.clone()),
            Arg::Slot(s) => m.get(s).cloned(),
        };
        let mut computed = vec![];
        for ins in prefix {
            match ins {
                Op::Copy(dst, a) => {
                    let v = resolve(a, &ports).ok_or("unbound RHS copy")?;
                    ports.insert(*dst, v);
                }
                Op::Let(dst, Call::Primitive(op, sort), args)
                    if op == "+" && sort == "i64" && args.len() == 2 =>
                {
                    let mut operands = args
                        .iter()
                        .map(|a| resolve(a, &ports))
                        .collect::<Option<Vec<_>>>()
                        .ok_or("unbound RHS operands")?;
                    operands.sort();
                    let result = rule
                        .query
                        .iter()
                        .find_map(|atom| {
                            if !matches!(&atom.call,Call::Primitive(n,s) if n==op && s==sort) {
                                return None;
                            }
                            let (Arg::Slot(out), args) = atom.args.split_last()? else {
                                return None;
                            };
                            if args.len() != 2 {
                                return None;
                            }
                            let mut query = args
                                .iter()
                                .map(|a| resolve(a, &ports))
                                .collect::<Option<Vec<_>>>()?;
                            query.sort();
                            (operands == query)
                                .then(|| rule.inputs.binary_search(out).ok())
                                .flatten()
                        })
                        .ok_or("RHS addition has no matching successful LHS computation")?;
                    ports.insert(*dst, Arg::Slot(result));
                    computed.push((*dst, result));
                }
                _ => return Err("unproved RHS prefix".into()),
            }
        }
        if computed.is_empty() {
            return Err("consumer has no reusable checked arithmetic port".into());
        }
        let key_ports = key
            .iter()
            .map(|a| resolve(a, &ports))
            .collect::<Option<Vec<_>>>()
            .ok_or("unbound constructor key")?;
        let retained_slots = rule
            .inputs
            .iter()
            .copied()
            .chain(computed.iter().map(|(dst, _)| *dst))
            .collect::<Vec<_>>();
        if !retained_slots.windows(2).all(|w| w[0] < w[1]) {
            return Err("terminal ports require ascending SSA slot order".into());
        }
        Ok(Self {
            retained_slots,
            unit,
            rule: rid,
            pivot,
            op: prefix.len(),
            table: *table,
            dst: *dst,
            key_ports,
            computed,
        })
    }
}

#[derive(Default, Serialize)]
pub(crate) struct Counts {
    pub sealed: u64,
    pub applied: u64,
    pub sealed_values: u64,
    pub existing_results: u64,
    pub new_results: u64,
    pub constructor_requests: u64,
    pub direct: u64,
    pub binding_maps_at_match: u64,
    pub diagnostic_shadow_maps: u64,
    pub selected_rhs_ns: u64,
    pub frame_input_maps_at_birth: u64,
    pub compact_births: u64,
    pub checked_computations_reused: u64,
    pub receipt_hits: u64,
    pub keyed_hits: u64,
}

pub(crate) struct Trial {
    pub plan: Plan,
    pub execute: bool,
    pub pending: FxHashMap<usize, Vec<Value>>,
    pub counts: Counts,
    #[cfg(feature = "slot-transfer-audit")]
    pub trace: sha2::Sha256,
}

impl Trial {
    pub fn new(plan: Plan, execute: bool) -> Self {
        Self {
            plan,
            execute,
            pending: Default::default(),
            counts: Default::default(),
            #[cfg(feature = "slot-transfer-audit")]
            trace: Default::default(),
        }
    }
}

#[cfg(feature = "slot-transfer-audit")]
#[derive(Default)]
pub(crate) struct EffectAudit {
    pub active: bool,
    pub count: u64,
    pub digest: sha2::Sha256,
}
#[cfg(feature = "slot-transfer-audit")]
impl EffectAudit {
    pub fn record(
        &mut self,
        cell: u32,
        op: usize,
        table: usize,
        key: &[Value],
        witness: Dependency,
    ) -> Result<()> {
        if self.active {
            serde_json::to_writer(&mut self.digest, &(cell, op, table, key, witness))
                .map_err(|e| e.to_string())?;
            self.count += 1;
        }
        Ok(())
    }
}
pub(crate) struct Commit {
    pub cell: u32,
    pub witness: Dependency,
    pub key: SmallVec<[Value; 4]>,
    pub born: bool,
    pub compact: bool,
    pub from_receipt: bool,
}

impl Graph {
    /// Read the target at the original RHS ordinal. On a miss its original
    /// template, destination slot and logical ID define the new result.
    pub(crate) fn commit_baked_consumer(
        &mut self,
        p: &Plan,
        ports: Vec<Value>,
        donor: Option<FactRef>,
    ) -> Result<Commit> {
        let key: SmallVec<[Value; 4]> = p
            .key_ports
            .iter()
            .map(|a| {
                canon(
                    match a {
                        Arg::Slot(i) => &ports[*i],
                        Arg::Literal(v) => v,
                    },
                    &self.parents,
                )
            })
            .collect();
        let cell = self.reserve_cell_id()?;
        let supplied = match donor {
            Some(f) => self
                .live_handoff(f, p.table)?
                .filter(|r| r.values[..r.values.len() - 1] == key[..]),
            None => None,
        };
        let from_receipt = supplied.is_some();
        let row = if from_receipt {
            supplied
        } else {
            self.stats.constructor_requests += 1;
            self.lookup_canonical(p.table, &key)?
        };
        if let Some(row) = row {
            if !from_receipt {
                self.stats.constructor_reused += 1;
            } else {
                self.stats.handoff_attempts += 1;
                self.stats.handoff_lets += 1;
                #[cfg(feature = "rhs-work-audit")]
                self.rhs_births.read(row.witness);
            }
            self.stats.rhs_frames_avoided += 1;
            return Ok(Commit {
                cell,
                key,
                witness: row.witness,
                born: false,
                compact: false,
                from_receipt,
            });
        }
        let rule = &self.program.rules[p.rule];
        let template = rule.template;
        let extra = p
            .computed
            .iter()
            .map(|&(_, pos)| ports[pos].clone())
            .collect();
        let ports = match self.port_frames.reserve_consumer(
            cell,
            template,
            self.epoch,
            &p.retained_slots,
            p.op as u32,
            ports,
            extra,
        ) {
            Ok(()) => {
                let f = FactRef {
                    cell,
                    op: p.op as u32,
                };
                let activation = self
                    .slot(cell, p.dst)
                    .and_then(|value| self.activate_into::<true>(f, p.table, &key, value, false));
                if let Err(error) = activation {
                    if self.port_frames.empty(cell) {
                        self.port_frames.remove(cell);
                    }
                    return Err(error);
                }
                return Ok(Commit {
                    cell,
                    key,
                    witness: Dependency::Fact(f),
                    born: true,
                    compact: true,
                    from_receipt: false,
                });
            }
            Err(values) => values,
        };
        let mut values: Slots = rule.inputs.iter().copied().zip(ports).collect();
        for &(dst, input) in &p.computed {
            let value = values[&rule.inputs[input]].clone();
            values.insert(dst, value);
        }
        self.trust_sealed_frame(cell);
        debug_assert!(self.active_rhs.is_none());
        self.active_origin = None;
        self.active_rhs = Some((cell, self.new_cell(template, values, None)));
        self.insert_constructor(cell, p.op, p.table, &key, p.dst)?;
        let frame = self.rhs_frame_mut(cell).unwrap();
        frame.completed = true;
        frame.completed_prefix = p.op + 1;
        Ok(Commit {
            cell,
            key,
            witness: Dependency::Fact(FactRef {
                cell,
                op: p.op as u32,
            }),
            born: true,
            compact: false,
            from_receipt: false,
        })
    }
}
