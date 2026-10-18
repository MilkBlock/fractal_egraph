//! Same native admission/check/commit boundary as NumericSession. The host sends
//! rows, not row combinations. Device receipts survive errors and table changes.
use super::*;

impl NumericSession {
    pub(super) fn init_tables(&mut self) -> Result<()> {
        let schema = self
            .source
            .eg
            .get_function(&self.function)
            .unwrap()
            .schema()
            .clone();
        let mut keys = vec![];
        for (k, sort) in self.keys.iter().zip(&schema.input) {
            keys.push(match k {
                CellOperand::Slot(i) => (0, *i as u32),
                CellOperand::Constant { value, .. } => {
                    let next = u32::try_from(self.identities.len() + 1)?;
                    let id = *self
                        .identities
                        .entry((sort.name().into(), value.rep()))
                        .or_insert(next);
                    (1, id)
                }
            });
        }
        let config = crate::table_program::compile(
            self.markers.columns(),
            self.source.sink.slots().len(),
            &self.numeric_slots,
            &self.program.numeric.as_ref().unwrap().exprs,
            &self.guards,
            self.value_expr,
            &keys,
            self.merge,
        )?;
        let initialized = self.device.table_init(config)?;
        self.stats.physical_layout = json!({"plan":{"method":"resident typed fragment tables"},"solver_calls":0,"cache_hits":0});
        self.stats.install_breakdown = initialized;
        self.table_mode = true;
        Ok(())
    }
    fn table_binding(&self, ids: &[u32]) -> Result<Vec<Value>> {
        if ids.len() != self.source.parts.len() {
            return Err("GPU table source arity".into());
        }
        let mut values = vec![None; self.source.sink.slots().len()];
        for (part, id) in ids.iter().enumerate() {
            let id = *id as usize;
            let frame = self.frames.get(&id).ok_or("GPU table unknown source")?;
            if frame.part != part || !self.active.contains(&id) {
                return Err("GPU table stale/wrong-part source".into());
            }
            for (slot, v) in frame.binding.iter().enumerate() {
                if let Some(v) = v {
                    if values[slot].is_some_and(|a| a != *v) {
                        return Err("GPU table incompatible binding".into());
                    }
                    values[slot] = Some(*v);
                }
            }
        }
        values
            .into_iter()
            .map(|v| v.ok_or_else(|| "GPU table incomplete binding".into()))
            .collect()
    }
    fn table_key(&self, binding: &[Value]) -> Vec<Value> {
        let schema = self
            .source
            .eg
            .get_function(&self.function)
            .unwrap()
            .schema();
        self.keys
            .iter()
            .zip(&schema.input)
            .map(|(k, s)| {
                self.source.eg.get_canonical_value(
                    match k {
                        CellOperand::Slot(i) => binding[*i],
                        CellOperand::Constant { value, .. } => *value,
                    },
                    s,
                )
            })
            .collect()
    }
    pub(super) fn advance_tables(&mut self, native_updated: bool) -> Result<bool> {
        let tick = Instant::now();
        let total = self.program.cells.len();
        let count = total - self.table_uploaded;
        let mut words = vec![
            u32::try_from(self.epoch)?,
            u32::try_from(count)?,
            u32::try_from(self.retired.len())?,
            u32::try_from(total)?,
        ];
        for id in self.table_uploaded..total {
            words.push(self.frames[&id].part as u32);
            words.extend(self.program.cells[id].binding.as_ref().unwrap());
            for slot in &self.numeric_slots {
                let v = self.frames[&id].binding[*slot]
                    .map(|v| self.source.eg.value_to_base::<i64>(v))
                    .unwrap_or(0) as u64;
                words.extend([v as u32, (v >> 32) as u32]);
            }
        }
        words.extend(self.retired.iter().map(|i| *i as u32));
        self.stats.phase_seconds.program_seconds += tick.elapsed().as_secs_f64();
        let tick = Instant::now();
        let audit = self.checker.is_some() || self.source.audit;
        // A transport failure may follow a device-side update. Without a receipt
        // we cannot safely infer which delta has been consumed.
        let (report, body) = self.device.table_delta(&words, audit).inspect_err(|_| {
            self.check_failed = true;
        })?;
        self.table_uploaded = total;
        self.stats.table_uploaded_bytes += words.len() * 4;
        self.stats.table_downloaded_bytes += body.len() * 4;
        self.stats.phase_seconds.dispatch_seconds += tick.elapsed().as_secs_f64();
        self.stats.phase_seconds.device_seconds += report["gpu_seconds"].as_f64().unwrap_or(0.0);
        self.stats.table_runtime = report.clone();
        self.stats.table_batches += 1;
        self.stats.table_index_lookups += report["index_lookups"]
            .as_u64()
            .ok_or("missing table lookup count")?;
        self.stats.table_candidate_rows += report["candidate_row_visits"]
            .as_u64()
            .ok_or("missing table visit count")?;
        self.stats.table_match_records = report["match_records"]
            .as_u64()
            .ok_or("missing table match count")?;
        self.stats.table_retries += report["retry_count"]
            .as_u64()
            .ok_or("missing table retry count")?;
        self.stats.device = json!({"device":report["device"],"matrix_bytes":report["matrix_bytes"],"numeric":{"arithmetic_error":report["arithmetic_error"]},"table":report});
        if report["arithmetic_error"] == true {
            return Err(
                "checked i64 RHS failed; no numeric group from this batch committed".into(),
            );
        }
        let tick = Instant::now();
        let before_checks = self.stats.phase_seconds.verification_seconds;
        let outcome = self.check_table_packet(&report, &body, audit);
        let writes = match outcome {
            Ok(w) => w,
            Err(e) => {
                self.check_failed = true;
                if self.checker.is_some() {
                    self.stats.verification.failures += 1;
                }
                return Err(e);
            }
        };
        self.stats.phase_seconds.result_seconds += tick.elapsed().as_secs_f64()
            - (self.stats.phase_seconds.verification_seconds - before_checks);
        let tick = Instant::now();
        let mut rows = vec![];
        for (key, value) in &writes {
            let numeric = self.source.eg.base_to_value(*value);
            let unit = self.source.eg.base_to_value(());
            rows.push(
                self.writer
                    .slots()
                    .iter()
                    .map(|(name, sort)| {
                        if *name == "value" {
                            Ok(numeric)
                        } else if let Some(k) =
                            name.strip_prefix('k').and_then(|k| k.parse::<usize>().ok())
                        {
                            key.get(k).copied().ok_or_else(|| "table writer key".into())
                        } else if *sort == "Unit" {
                            Ok(unit)
                        } else {
                            Err("table writer operand".into())
                        }
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        if !rows.is_empty() {
            self.source
                .eg
                .apply_cell_bindings(&self.writer, &rows)
                .inspect_err(|_| {
                    self.check_failed = true;
                })?;
        }
        self.device.table_ack(self.epoch).inspect_err(|_| {
            self.check_failed = true;
        })?;
        self.table_ack_count = report["match_records"]
            .as_u64()
            .ok_or("missing table match count")? as usize;
        self.stats.accepted_bindings += report["pending_matches"]
            .as_u64()
            .ok_or("missing pending matches")? as usize;
        self.stats.native_sets += rows.len();
        self.stats.phase_seconds.commit_seconds += tick.elapsed().as_secs_f64();
        Ok(native_updated || !rows.is_empty())
    }
    fn check_table_packet(
        &mut self,
        report: &Json,
        body: &[u32],
        audit: bool,
    ) -> Result<Vec<(Vec<Value>, i64)>> {
        if report["epoch"].as_u64() != Some(self.epoch as u64) {
            return Err("GPU table stale epoch".into());
        }
        let count = |key: &str| -> Result<usize> {
            Ok(usize::try_from(
                report[key].as_u64().ok_or("missing table count")?,
            )?)
        };
        let na = count("audit_rows")?;
        let ng = count("groups")?;
        let records = count("match_records")?;
        let active = count("active_matches")?;
        let pending_count = count("pending_matches")?;
        if records < self.table_ack_count
            || active > records
            || pending_count > active
            || ng > pending_count
        {
            return Err("GPU table invalid record counts".into());
        }
        let parts = self.source.parts.len();
        let ar = 5 + parts;
        let gr = 3 + parts;
        let audit_words = na.checked_mul(ar).ok_or("GPU table size overflow")?;
        let expected_words = ng
            .checked_mul(gr)
            .and_then(|g| audit_words.checked_add(g))
            .ok_or("GPU table size overflow")?;
        if body.len() != expected_words || (!audit && na != 0) {
            return Err("GPU table packet shape".into());
        }
        let number = |lo: u32, hi: u32| ((lo as u64) | ((hi as u64) << 32)) as i64;
        let mut accepted = vec![];
        let mut pending: BTreeMap<Vec<Value>, Vec<CellI64Proposal>> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut receipts = BTreeMap::new();
        for row in body[..audit_words].chunks_exact(ar) {
            if row[0] as usize >= records
                || !ids.insert(row[0])
                || row[1] != u32::from(row[0] as usize >= self.table_ack_count)
                || row[2] != 1
            {
                return Err("GPU table invalid/duplicate match receipt".into());
            }
            receipts.insert(row[0], (row[1], &row[5..]));
            let b = self.table_binding(&row[5..])?;
            if row[1] == 1 {
                pending
                    .entry(self.table_key(&b))
                    .or_default()
                    .push(CellI64Proposal {
                        binding: b.clone(),
                        value: number(row[3], row[4]),
                    });
            }
            accepted.push(b);
        }
        if audit
            && (accepted.len() != active
                || pending.values().map(Vec::len).sum::<usize>() != pending_count
                || accepted.iter().collect::<BTreeSet<_>>().len() != accepted.len())
        {
            return Err("GPU table missing/duplicate active bindings".into());
        }
        let mut groups = vec![];
        let mut writes = vec![];
        let mut seen = BTreeSet::new();
        for row in body[audit_words..].chunks_exact(gr) {
            if row[0] as usize >= records
                || (row[0] as usize) < self.table_ack_count
                || (audit && receipts.get(&row[0]) != Some(&(1, &row[3..])))
            {
                return Err("GPU table invalid group receipt".into());
            }
            let binding = self.table_binding(&row[3..])?;
            let key = self.table_key(&binding);
            if !seen.insert(key.clone()) {
                return Err("GPU table duplicate group".into());
            }
            let value = number(row[1], row[2]);
            let proposals = if audit {
                pending
                    .remove(&key)
                    .ok_or("GPU table group has no pending proposals")?
            } else {
                vec![]
            };
            if audit {
                let expected = proposals
                    .iter()
                    .map(|p| p.value)
                    .reduce(|a, b| match self.merge {
                        n::Merge::Min => a.min(b),
                        n::Merge::Max => a.max(b),
                    })
                    .unwrap();
                if expected != value {
                    return Err("GPU table reduction mismatch".into());
                }
            }
            let initial = self
                .source
                .eg
                .lookup_function_row(&self.function, &key)
                .map(|r| self.source.eg.value_to_base::<i64>(r[key.len()]));
            let result = initial.map_or(value, |old| match self.merge {
                n::Merge::Min => old.min(value),
                n::Merge::Max => old.max(value),
            });
            if initial != Some(result) {
                writes.push((key.clone(), result));
            }
            groups.push(CellI64Group {
                key,
                initial,
                proposals,
                result: Some(result),
            });
        }
        if !pending.is_empty() {
            return Err("GPU table omitted a pending key".into());
        }
        let tick = Instant::now();
        if let Some(checker) = &self.checker {
            let report = self.source.eg.check_cell_i64_batch(
                &self.source.sink,
                checker,
                &accepted,
                &groups,
            )?;
            self.stats.verification.batches += 1;
            self.stats.verification.native_bindings += report.native_bindings;
            self.stats.verification.proposals += report.proposals;
            self.stats.verification.groups += report.groups;
            if self.source.audit {
                self.stats.audit_uncovered.push(0);
            }
        } else if self.source.audit {
            let expected = self
                .source
                .eg
                .audit_cell_matches(&self.source.sink)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            let actual = accepted.into_iter().collect::<BTreeSet<_>>();
            if !actual.is_subset(&expected) {
                return Err("GPU table non-native match".into());
            }
            self.stats
                .audit_uncovered
                .push(expected.difference(&actual).count());
        }
        self.stats.phase_seconds.verification_seconds += tick.elapsed().as_secs_f64();
        Ok(writes)
    }
}
