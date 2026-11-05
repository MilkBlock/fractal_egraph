use super::*;
use crate::local_rhs::{Block, Counts, ResultUse};

pub(crate) enum Start {
    Complete { cell: u32, frame: Cell },
    Promoted { cell: u32, next_op: usize },
}
impl Graph {
    pub(crate) fn local_key(&self, id: u32, frame: &Cell, args: &[Arg]) -> Result<Vec<Value>> {
        args.iter()
            .map(|a| self.argument_in(id, frame, a))
            .collect()
    }
    pub(crate) fn local_rhs(
        &mut self,
        template: usize,
        values: crate::slots::Slots,
        parent: Option<FactRef>,
        plan: &Block,
        mut receipts: Option<&mut [Option<Dependency>]>,
        stats: &mut Counts,
    ) -> Result<Start> {
        let cell = self.reserve_cell_id()?;
        let mut frame = self.new_cell(template, values, parent);
        let mut local_lets = 0;
        stats.enter += 1;
        for step in &plan.steps {
            self.known_stats.static_eliminated += step.skip_before;
            self.known_stats.avoided_lookups += step.skip_before;
            let key = step
                .key
                .iter()
                .map(|a| self.argument_in(cell, &frame, a))
                .collect::<Result<smallvec::SmallVec<[Value; 4]>>>()?;
            if matches!(step.result, ResultUse::Let(_)) {
                self.stats.constructor_requests += 1;
            }
            let assigned = if let ResultUse::Set(v) = &step.result {
                Some(self.argument_in(cell, &frame, v)?)
            } else {
                None
            };
            if let Some(row) = self.lookup_canonical(step.table, &key)? {
                let value = row.values.last().unwrap();
                match step.result {
                    ResultUse::Let(s) => {
                        frame.values.insert(s, value.clone());
                        self.stats.constructor_reused += 1;
                        local_lets += 1;
                        stats.local_let_results += 1;
                    }
                    ResultUse::Set(_) => {
                        let rhs = assigned.unwrap();
                        if *value == rhs {
                            stats.no_op_unions += 1;
                        } else {
                            self.enqueue_union(value.clone(), rhs)?;
                            stats.ordered_unions += 1;
                        }
                    }
                }
                if let Some(rs) = receipts.as_mut() {
                    rs[step.op] = Some(row.witness);
                }
            } else {
                self.store_cell(cell, frame);
                let fact = FactRef {
                    cell,
                    op: step.op as u32,
                };
                match step.result {
                    ResultUse::Let(s) => {
                        self.insert_constructor(cell, step.op, step.table, &key, s)?;
                    }
                    ResultUse::Set(_) => {
                        self.activate(fact, step.table, &key, assigned.unwrap(), false)?;
                    }
                }
                if let Some(rs) = receipts.as_mut() {
                    rs[step.op] = Some(Dependency::Fact(fact));
                }
                stats.promoted += 1;
                stats.prefix_lets_on_promotion += local_lets;
                return Ok(Start::Promoted {
                    cell,
                    next_op: step.op + 1,
                });
            }
        }
        self.known_stats.static_eliminated += plan.trailing_aliases;
        self.known_stats.avoided_lookups += plan.trailing_aliases;
        stats.local_complete += 1;
        stats.block_local_intermediates += plan.intermediate_ops.len() as u64;
        self.stats.rhs_frames_avoided += 1;
        Ok(Start::Complete { cell, frame })
    }
}
