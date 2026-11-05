//! Share complete unary point reads for compiler-selected table consumers
//! during the immutable matching phase. The source Run epoch seals each slot.
use super::*;
use crate::row_kernel::Access;

struct Receipt {
    epoch: u64,
    key: u64,
    row: Option<Row>,
}
pub(super) struct PointPorts {
    tables: Vec<Option<Box<[Option<Receipt>]>>>,
}
impl PointPorts {
    pub(super) fn new(p: &Program) -> Self {
        let mut consumers = vec![0usize; p.tables.len()];
        for (rid, kernels) in p.kernels.iter().enumerate() {
            for kernel in kernels.iter().flatten() {
                for step in &kernel.steps {
                    if !matches!(step.access, Access::Key) {
                        continue;
                    }
                    let atom = &p.rules[rid].query[step.atom];
                    if let Call::Table(t) = atom.call
                        && atom.args.len() == 2
                        && p.tables[t].schema[0]["kind"] == "eq"
                    {
                        consumers[t] += 1;
                    }
                }
            }
        }
        let tables = consumers
            .into_iter()
            .map(|n| (n > 1).then(|| std::iter::repeat_with(|| None).take(32).collect()))
            .collect();
        Self { tables }
    }

    pub(super) fn read(
        &mut self,
        graph: &mut Graph,
        t: usize,
        args: &[Value],
    ) -> Result<Option<Row>> {
        let ([Value::Eq(key)], Some(slots)) = (args, &mut self.tables[t]) else {
            return graph.lookup_canonical(t, args);
        };
        let pos = (key ^ key.rotate_left(32)).wrapping_mul(0x9e3779b185ebca87) >> 59;
        let slot = &mut slots[pos as usize];
        graph.stats.shared_point_requests += 1;
        if let Some(hit) = slot
            && hit.epoch == graph.epoch
            && hit.key == *key
        {
            graph.stats.shared_point_hits += 1;
            graph.stats.shared_negative_hits += u64::from(hit.row.is_none());
            return Ok(hit.row.clone());
        }
        let row = graph.lookup_canonical(t, args)?;
        *slot = Some(Receipt {
            epoch: graph.epoch,
            key: *key,
            row: row.clone(),
        });
        Ok(row)
    }
}
