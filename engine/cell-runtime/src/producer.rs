//! Access to the live producer of a committed fact.
use super::*;
#[derive(Clone, Copy)]
pub struct Context {
    pub completed: bool,
    pub cell: u32,
    pub template: usize,
    pub depth: u32,
    pub op: usize,
}
impl Graph {
    pub(super) fn retain_reference(&mut self, f: FactRef) {
        *self.parent_users.entry(f.cell).or_default() += 1;
    }
    pub(super) fn release_reference(&mut self, f: FactRef) {
        *self.parent_users.get_mut(&f.cell).expect("retained parent") -= 1;
        self.garbage.insert(f.cell);
    }
    pub fn continuation_context(&self, w: Dependency) -> Option<Context> {
        let actual = self.origin(w)?;
        #[cfg(feature = "port-frames")]
        if let Some(c)=self.port_frames.context(actual,self.epoch){return Some(c);}
        #[cfg(feature = "sparse-cells")]
        if let Dependency::Function(i) = w
            && let Some(writer) = self.functions[i].writer.as_ref()
        {
            #[cfg(feature = "function-blocks")]
            if writer.values.is_packed() {
                self.writer_dir.blocks_contexts.set(self.writer_dir.blocks_contexts.get()+1);
                if writer.epoch < self.epoch {
                    self.writer_dir.blocks_cross_run_contexts.set(self.writer_dir.blocks_cross_run_contexts.get()+1);
                }
            }
            return Some(Context {
                completed: writer.completed,
                cell: actual.cell,
                template: writer.template,
                depth: writer.depth,
                op: actual.op as usize,
            });
        }
        let f = if let Dependency::Fact(f) = w {
            self.fact_state(f)?
                .entry
                .unwrap_or(f)
        } else {
            actual
        };
        let c = self.rhs_frame(f.cell)?;
        Some(Context {
            completed: c.completed,
            cell: f.cell,
            template: c.template,
            depth: c.recipe_depth,
            op: f.op as usize,
        })
    }
    pub fn context_argument(&self, c: Context, a: &Arg) -> Result<Value> {
        self.argument(c.cell, a)
    }
    pub fn context_arguments(&self, c: Context, aa: &[Arg]) -> Result<Vec<Value>> {
        aa.iter().map(|a| self.context_argument(c, a)).collect()
    }
    pub fn context_fact(&self, c: Context, op: usize) -> FactRef {
        FactRef {
            cell: c.cell,
            op: op as u32,
        }
    }
}
