use super::*;
use crate::row_kernel::{Access, Kernel};
use smallvec::SmallVec;

pub(super) struct Registers {
    fields: Vec<Value>,
    witnesses: Vec<Option<Dependency>>,
}
impl Registers {
    pub(super) fn new(r: &Rule) -> Self {
        Self {
            fields: vec![Value::Int(0); r.inputs.iter().copied().max().map_or(0, |s| s + 1)],
            witnesses: vec![None; r.query.len()],
        }
    }
    fn val(&self, a: &Arg, g: &Graph) -> Value {
        match a {
            Arg::Slot(s) => self.fields[*s].clone(),
            Arg::Literal(v) => canon(v, &g.parents),
        }
    }
    fn agrees(&self, a: &Arg, v: &Value, g: &Graph) -> bool {
        match a {
            Arg::Slot(s) => self.fields[*s] == *v,
            Arg::Literal(l) => canon(l, &g.parents) == *v,
        }
    }
    pub(super) fn start(
        &mut self,
        exec: &mut Executor,
        r: &Rule,
        k: &Kernel,
        seed: &Row,
        domains: &[Subtrie],
        known: &QueryFacts,
        out: &mut Vec<Binding>,
    ) -> Result<()> {
        exec.graph.stats.register_seeds += 1;
        self.witnesses[k.pivot] = Some(seed.witness);
        for &(c, s) in &k.seed {
            self.fields[s] = seed.values[c].clone();
        }
        self.step(
            exec,
            r,
            k,
            0,
            (seed.epoch, seed.witness),
            seed.epoch,
            domains,
            known,
            out,
        )
    }
    fn step(
        &mut self,
        exec: &mut Executor,
        r: &Rule,
        k: &Kernel,
        pc: usize,
        owner: (u64, Dependency),
        mut epoch: u64,
        domains: &[Subtrie],
        known: &QueryFacts,
        out: &mut Vec<Binding>,
    ) -> Result<()> {
        for (offset, ins) in k.steps[pc..].iter().enumerate() {
            let a = &r.query[ins.atom];
            let keys = &a.args[..a.args.len() - 1];
            if let Access::Expand = ins.access {
                let Call::Table(t) = a.call else {
                    unreachable!()
                };
                let refs = if let Some(domain) = domains.iter().find(|d| d.atom == ins.atom) {
                    domain.rows.clone()
                } else {
                    let mut bound = smallvec::smallvec![None; a.args.len()];
                    for (col, arg) in &ins.bound {
                        bound[*col] = Some(self.val(arg, &exec.graph));
                    }
                    let bound: SmallVec<[Option<Value>; 6]> = bound;
                    exec.query_indexes.candidates(&exec.graph, t, &bound)?
                };
                for &w in refs.iter() {
                    exec.graph.stats.residual_candidates += 1;
                    let Some(row) = exec.graph.row(w)? else {
                        continue;
                    };
                    if row.hidden
                        || row.table != t
                        || (row.epoch, w) > owner
                        || ((row.epoch, w) == owner && ins.atom < k.pivot)
                    {
                        continue;
                    }
                    // Repeated fresh variables compare against this row's first column.
                    for &(col, s) in &ins.assign {
                        self.fields[s] = row.values[col].clone();
                    }
                    if !ins
                        .check
                        .iter()
                        .all(|(col, arg)| self.agrees(arg, &row.values[*col], &exec.graph))
                    {
                        continue;
                    }
                    self.witnesses[ins.atom] = Some(w);
                    self.step(
                        exec,
                        r,
                        k,
                        pc + offset + 1,
                        owner,
                        epoch.max(row.epoch),
                        domains,
                        known,
                        out,
                    )?;
                }
                return Ok(());
            }
            let args: SmallVec<[Value; 4]> =
                keys.iter().map(|arg| self.val(arg, &exec.graph)).collect();
            let result = match &a.call {
                Call::Table(t) => {
                    exec.graph.stats.residual_candidates += 1;
                    let Some(row) = exec.graph.lookup_canonical(*t, &args)? else {
                        return Ok(());
                    };
                    let stamp = (row.epoch, row.witness);
                    if row.hidden || stamp > owner || (stamp == owner && ins.atom < k.pivot) {
                        return Ok(());
                    }
                    epoch = epoch.max(row.epoch);
                    self.witnesses[ins.atom] = Some(row.witness);
                    row.values.last().unwrap().clone()
                }
                Call::Primitive(name, sort) => {
                    if name.starts_with("cell-key-") {
                        exec.graph.stats.key_operations += 1;
                    }
                    let Some(v) = value::primitive(
                        name,
                        &args,
                        sort,
                        &exec.graph.program.artifact["source_core"]["structural_key"]["nodes"],
                    )?
                    else {
                        return Ok(());
                    };
                    v
                }
            };
            if let Some((_, arg)) = ins.check.first()
                && !self.agrees(arg, &result, &exec.graph)
            {
                return Ok(());
            }
            if let Some(&(_, s)) = ins.assign.first() {
                self.fields[s] = result;
            }
        }
        let mut facts = known.clone();
        if !matches!(facts, QueryFacts::None) {
            for (i, &w) in self.witnesses.iter().enumerate() {
                facts.record(i, w);
            }
        }
        exec.graph.stats.register_answers += 1;
        out.push(Binding {
            #[cfg(feature = "iterative-bake")]
            direct: None,
            known: facts,
            composition: None,
            witnesses: MatchedFacts::None,
            values: r
                .inputs
                .iter()
                .map(|s| (*s, self.fields[*s].clone()))
                .collect(),
            epoch,
            parent: None,
            continuation: None,
            compiled: true,
        });
        Ok(())
    }
}
