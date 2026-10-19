//! Adapter only: whole-Cell proofs stay in egg_layout; backend scheduling and
//! committed-row transport stay in cell-assisted-egglog. No graph clones.
use cell_assisted_egglog::{Result, table_live::SaturationHook};
use egg_layout::whole_cell::automatic::SaturationObserver;
use egglog::{EGraph, ast::Schedule, cell_events::CellCommitBatch};
use serde_json::Value;
pub struct WholeCellHook(pub SaturationObserver);
impl SaturationHook for WholeCellHook {
    fn diagnostics(&self) -> Value {
        self.0.diagnostics()
    }
    fn try_boundary(&mut self, eg: &mut EGraph, s: &Schedule) -> Result<Option<Value>> {
        self.0.try_boundary(eg, s)
    }
    fn take_commits(&mut self, eg: &mut EGraph) -> Result<(Vec<String>, Vec<CellCommitBatch>)> {
        self.0.take_commits(eg)
    }
}
