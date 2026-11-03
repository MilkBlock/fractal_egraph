//! Terminal snapshot loading for extraction. Never resumes seminaive rules.
use super::*;

impl EGraph {
    /// Replace all function rows with a canonical snapshot. No rules or rebuild
    /// run here. Caller must use fresh IDs, unique keys and validated schemas.
    /// This invalidates rule watermarks; use the result only for extraction.
    pub fn replace_snapshot_for_extraction(
        &mut self,
        rows: Vec<(FunctionId, Vec<Value>, bool)>,
    ) -> Result<()> {
        let mut keys = std::collections::BTreeSet::new();
        for (func, row, hidden) in &rows {
            let info = &self.funcs[*func];
            anyhow::ensure!(row.len() == info.schema.len(), "snapshot row arity mismatch");
            anyhow::ensure!(!hidden || info.can_subsume, "snapshot hides a non-subsumable row");
            anyhow::ensure!(keys.insert((*func, row[..row.len()-1].to_vec())), "duplicate canonical snapshot key");
        }
        for (_, info) in self.funcs.iter() {
            self.db.get_table_mut(info.table).clear();
        }
        self.db.get_table_mut(self.uf_table).clear();
        let mut buffers = DenseIdMap::default();
        for (func, row, hidden) in rows {
            let info = &self.funcs[func];
            let math = SchemaMath { subsume: info.can_subsume, func_cols: info.schema.len() };
            let mut extended = row;
            math.write_table_row(&mut extended, RowVals {
                timestamp: self.next_ts().to_value(),
                subsume: info.can_subsume.then_some(if hidden { SUBSUMED } else { NOT_SUBSUMED }),
                ret_val: None,
            });
            buffers.get_or_insert(info.table, || self.db.new_buffer(info.table)).stage_insert(&extended);
        }
        drop(buffers);
        // Deliberately avoid flush_updates(), which can invoke rebuild.
        self.db.merge_all();
        anyhow::ensure!(self.db.get_table(self.uf_table).len() == 0, "snapshot loading unexpectedly introduced unions");
        self.inc_ts();
        Ok(())
    }
}
