//! Exact storage-only absorption. No fact becomes hidden or newly derived.
use super::*;
impl Graph {
    pub fn resolve_reference(&self, f: FactRef) -> FactRef {
        crate::views::resolve(&self.fact_forwards, f)
    }
    pub(super) fn carrier(&self, f: FactRef) -> Option<&crate::views::Carrier> {
        let c = self.cells.get(&f.cell)?;
        self.program.views.carrier(
            c.template,
            f.op as usize,
            self.program.templates[c.template].ops.len(),
        )
    }
    pub(super) fn retain_reference(&mut self, f: FactRef) {
        let f = self.resolve_reference(f);
        *self.reference_users.entry(f).or_default() += 1;
        *self.parent_users.entry(f.cell).or_default() += 1;
    }
    pub(super) fn release_reference(&mut self, f: FactRef) {
        let f = self.resolve_reference(f);
        let n = self
            .reference_users
            .get_mut(&f)
            .expect("retained fact reference");
        *n -= 1;
        if *n == 0 {
            self.reference_users.remove(&f);
        }
        *self.parent_users.get_mut(&f.cell).expect("retained Cell") -= 1;
        self.garbage.insert(f.cell);
    }
    pub fn reference_frame(
        &mut self,
        template: usize,
        values: BTreeMap<usize, Value>,
        parent: Option<FactRef>,
        witnesses: &BTreeMap<usize, Dependency>,
    ) -> Result<u32> {
        if !self.views_enabled {
            return self.linked_frame(template, values, parent);
        }
        if !self
            .program
            .views
            .layouts
            .get(&template)
            .is_some_and(|l| l.reference_only)
        {
            return self.linked_frame(template, values, parent);
        }
        // Keep the matched, correlated input tuple locally. Fact identity still
        // uses FactRef; only field storage changes. slot() canonicalizes copied
        // equality IDs, and publishing the marker subscribes its full row to
        // union/repair just like every other virtual fact. No provider pins are
        // needed to keep a snapshot of immutable matched parameters alive.
        if self.marker_fields_copied {
            return self.frame(template, values);
        }
        let id = self.frame(template, values)?;
        let Some(layout) = self.program.views.layouts.get(&template).cloned() else {
            return Ok(id);
        };
        for carrier in &layout.carriers {
            let Some(Dependency::Fact(f)) = witnesses.get(&carrier.atom) else {
                continue;
            };
            let f = self.resolve_reference(*f);
            // A match was gathered at this Run's snapshot. Parameters are immutable;
            // a later RHS may hide the row, but must not rewrite the captured binding.
            let expected = self.arguments(id, &carrier.columns)?;
            let actual = (0..expected.len())
                .map(|c| self.fact_column(f, c))
                .collect::<Result<Vec<_>>>()?;
            if actual != expected {
                return Err("matched reference projection differs".into());
            }
            self.cells
                .get_mut(&id)
                .unwrap()
                .providers
                .insert(carrier.atom, f);
            self.retain_reference(f);
        }
        for (slot, (atom, _)) in layout.slots {
            if self.cells[&id].providers.contains_key(&atom)
                && self
                    .cells
                    .get_mut(&id)
                    .unwrap()
                    .values
                    .remove(&slot)
                    .is_some()
            {
                self.view_stats.projected_values += 1;
            }
        }
        Ok(id)
    }
    pub fn queue_absorption(&mut self, target: u32, witnesses: &BTreeMap<usize, Dependency>) {
        if !self.views_enabled || std::env::var("CELL_CPU_ABSORB").as_deref() == Ok("0") {
            return;
        }
        let Some(c) = self.cells.get(&target) else {
            return;
        };
        if c.facts.is_empty() {
            return;
        }
        let Some(layout) = self.program.views.layouts.get(&c.template) else {
            return;
        };
        for carrier in &layout.carriers {
            if let Some(Dependency::Fact(f)) = witnesses.get(&carrier.atom) {
                let f = self.resolve_reference(*f);
                if f.cell < target {
                    self.absorption_candidates.insert((target, f.cell));
                }
            }
        }
    }
    fn detach_parameters(&mut self, target: u32) -> Result<()> {
        let c = &self.cells[&target];
        let template = c.template;
        let mut values = BTreeMap::new();
        if let Some(layout) = self.program.views.layouts.get(&template) {
            for slot in layout.slots.keys() {
                if let Ok(v) = self.slot(target, *slot) {
                    values.insert(*slot, v);
                }
            }
        }
        let c = self.cells.get_mut(&target).unwrap();
        c.values.extend(values);
        let parent = c.parent.take();
        let providers = std::mem::take(&mut c.providers);
        if let Some(f) = parent {
            self.release_reference(f);
        }
        for f in providers.values() {
            self.release_reference(*f);
        }
        Ok(())
    }
    /// Returns old/new physical fact references to retarget queued work. Query
    /// indexes receive removals/additions, while logical timestamps stay unchanged.
    pub fn absorb_ready(&mut self) -> Result<BTreeMap<FactRef, FactRef>> {
        let mut moved = BTreeMap::new();
        for (target, source) in std::mem::take(&mut self.absorption_candidates) {
            if target == source
                || !self.cells.contains_key(&target)
                || !self.cells.contains_key(&source)
            {
                continue;
            }
            if self.cells[&target].facts.is_empty() {
                continue;
            }
            self.view_stats.absorption_attempts += 1;
            // Mutable function ownership cannot be redirected to a constructor carrier.
            let refs: usize = self
                .reference_users
                .range(
                    FactRef {
                        cell: source,
                        op: 0,
                    }..=FactRef {
                        cell: source,
                        op: u32::MAX,
                    },
                )
                .map(|(_, n)| *n)
                .sum();
            if self.parent_users.get(&source).copied().unwrap_or(0) > refs {
                self.view_stats.retained_function_owners += 1;
                continue;
            }
            let template = self.cells[&target].template;
            let base = self.program.templates[template].ops.len();
            let Some(layout) = self.program.views.layouts.get(&template).cloned() else {
                continue;
            };
            let mut targets = BTreeMap::new();
            for (i, c) in layout.carriers.iter().enumerate() {
                let vals = self.arguments(target, &c.columns)?;
                targets.entry((c.table, vals)).or_insert(FactRef {
                    cell: target,
                    op: (base + i) as u32,
                });
            }
            let mut needed: BTreeSet<FactRef> = self.cells[&source]
                .facts
                .keys()
                .map(|op| FactRef {
                    cell: source,
                    op: *op,
                })
                .collect();
            needed.extend(
                self.reference_users
                    .range(
                        FactRef {
                            cell: source,
                            op: 0,
                        }..=FactRef {
                            cell: source,
                            op: u32::MAX,
                        },
                    )
                    .map(|(f, _)| *f),
            );
            let mut map = BTreeMap::new();
            let mut valid = true;
            for old in needed {
                let Some(table) = self.dependency_table(Dependency::Fact(old)) else {
                    valid = false;
                    break;
                };
                let vals = (0..self.program.tables[table].schema.len())
                    .map(|c| self.fact_column(old, c))
                    .collect::<Result<Vec<_>>>()?;
                let Some(&new) = targets.get(&(table, vals.clone())) else {
                    valid = false;
                    break;
                };
                // Retained parameter-only aliases need no visible fact. A live fact must
                // still own its key; never resurrect a deleted/subsumed source.
                if self.cells[&source].facts.contains_key(&old.op) {
                    if self
                        .lookup(table, &vals[..vals.len() - 1])?
                        .is_none_or(|r| r.witness != Dependency::Fact(old))
                    {
                        valid = false;
                        break;
                    }
                }
                map.insert(old, new);
            }
            if !valid {
                self.view_stats.uncovered_sources += 1;
                continue;
            }
            // Break target -> source edges before redirecting source -> target. This
            // copies one owning tuple, not a clone of the source Cell or its closure.
            self.detach_parameters(target)?;
            for (&old, &new) in &map {
                if let Some(mut state) = self.cells.get_mut(&source).unwrap().facts.remove(&old.op)
                {
                    if state.entry.is_none() {
                        state.entry = Some(old);
                    }
                    let table = self
                        .dependency_table(Dependency::Fact(old))
                        .ok_or("absorption table")?;
                    self.operators[table].remove(&old);
                    self.operators[table].insert(new);
                    self.intern
                        .get_mut(&state.hash)
                        .ok_or("absorption hash")?
                        .remove(&old);
                    self.intern.entry(state.hash).or_default().insert(new);
                    if self.cells[&target].facts.contains_key(&new.op) {
                        return Err("absorption overwrites live carrier".into());
                    }
                    self.cells
                        .get_mut(&target)
                        .unwrap()
                        .facts
                        .insert(new.op, state);
                    self.subscribe(Dependency::Fact(old), &[]);
                    let row = self.fact(new)?.ok_or("missing transferred row")?;
                    self.subscribe(Dependency::Fact(new), &row.values);
                    moved.insert(old, new);
                    self.view_stats.relocated_facts += 1;
                }
                // Move pin counts too; users retain stable old handles and resolve at read.
                let users = self.reference_users.remove(&old).unwrap_or(0);
                if users > 0 {
                    *self.parent_users.get_mut(&source).unwrap() -= users;
                    *self.parent_users.entry(target).or_default() += users;
                    *self.reference_users.entry(new).or_default() += users;
                }
                self.fact_forwards.insert(old, new);
            }
            let c = &self.cells[&source];
            self.retired_contexts.insert(
                source,
                crate::views::RetiredContext {
                    template: c.template,
                    depth: c.recipe_depth,
                },
            );
            self.retire_empty(source);
            if self.cells.contains_key(&source) {
                return Err("absorbed source remained pinned".into());
            }
            self.view_stats.absorbed_cells += 1;
        }
        Ok(moved)
    }
    pub fn continuation_context(&self, w: Dependency) -> Option<crate::views::Context> {
        let actual = self.origin(w)?;
        let f = if let Dependency::Fact(f) = w {
            self.cells
                .get(&f.cell)?
                .facts
                .get(&f.op)?
                .entry
                .unwrap_or(f)
        } else {
            actual
        };
        if let Some(c) = self.cells.get(&f.cell) {
            return Some(crate::views::Context {
                cell: f.cell,
                template: c.template,
                depth: c.recipe_depth,
                op: f.op as usize,
            });
        }
        let c = self.retired_contexts.get(&f.cell)?;
        Some(crate::views::Context {
            cell: f.cell,
            template: c.template,
            depth: c.depth,
            op: f.op as usize,
        })
    }
    pub fn context_argument(&self, c: crate::views::Context, a: &Arg) -> Result<Value> {
        let Arg::Slot(s) = a else {
            return self.argument_literal(a);
        };
        if self.cells.contains_key(&c.cell) {
            return self.slot(c.cell, *s);
        }
        if let Some(sources) = self
            .program
            .views
            .exports
            .get(&c.template)
            .and_then(|m| m.get(s))
        {
            for (op, col) in sources {
                let f = FactRef {
                    cell: c.cell,
                    op: *op as u32,
                };
                if self.fact_forwards.contains_key(&f) {
                    let to = self.resolve_reference(f);
                    if self.cells.contains_key(&to.cell) {
                        return self.fact_column(to, *col);
                    }
                }
            }
        }
        let t = &self.program.templates[c.template];
        if let Some(Op::Copy(_, a)) = t.defs.get(s).and_then(|op| t.ops.get(*op)) {
            return self.context_argument(c, a);
        }
        Err("unavailable retired context slot".into())
    }
    fn argument_literal(&self, a: &Arg) -> Result<Value> {
        match a {
            Arg::Literal(v) => Ok(canon(v, &self.parents)),
            _ => Err("literal expected".into()),
        }
    }
    pub fn context_arguments(&self, c: crate::views::Context, aa: &[Arg]) -> Result<Vec<Value>> {
        aa.iter().map(|a| self.context_argument(c, a)).collect()
    }
    pub fn context_fact(&self, c: crate::views::Context, op: usize) -> FactRef {
        self.resolve_reference(FactRef {
            cell: c.cell,
            op: op as u32,
        })
    }
    pub fn reference_report(&self) -> J {
        json!({"enabled":self.views_enabled,"marker_fields":if self.marker_fields_copied{"copy"}else{"reference"},"stats":self.view_stats,"marker_tables":self.program.views.markers.len(),"live_marker_views":if self.views_enabled{self.program.views.markers.iter().map(|t|self.operators[*t].len()).sum::<usize>()}else{0},"provider_handles":self.cells.values().map(|c|c.providers.len()).sum::<usize>(),"forwarded_fact_handles":self.fact_forwards.len(),"retired_context_views":self.retired_contexts.len(),"absorption":"exact matched-carrier containment; mutable function owners retained"})
    }
}
