//! Lower the shared program to named, typed row-major matrices. All field offsets,
//! widths, alignment and shader ABI constants are generated here, not duplicated
//! in Swift and Metal struct layouts.
use crate::{
    Result,
    program::{Checked, Op},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Matrix {
    pub dtype: &'static str,
    pub rows: usize,
    pub cols: usize,
    pub offset: usize,
    pub fields: Vec<String>,
}
#[derive(Default)]
pub(crate) struct Arena {
    data: Vec<u32>,
    layout: BTreeMap<String, Matrix>,
}
impl Arena {
    pub(crate) fn add(
        &mut self,
        name: &str,
        dtype: &'static str,
        rows: usize,
        cols: usize,
        fields: &[&str],
        values: Vec<u32>,
    ) -> Result<usize> {
        let size = rows.checked_mul(cols).ok_or("matrix shape overflow")?;
        if cols == 0 || values.len() > size {
            return Err("matrix data/shape mismatch".into());
        }
        let off = (self.data.len() + 3) / 4 * 4;
        let end = off
            .checked_add(size.max(1))
            .ok_or("matrix address overflow")?;
        if end >= u32::MAX as usize {
            return Err("matrix address space exhausted".into());
        }
        self.data.try_reserve(end - self.data.len())?;
        self.data.resize(end, 0);
        self.data[off..off + values.len()].copy_from_slice(&values);
        self.layout.insert(
            name.into(),
            Matrix {
                dtype,
                rows,
                cols,
                offset: off,
                fields: fields.iter().map(|s| s.to_string()).collect(),
            },
        );
        Ok(off)
    }
    pub(crate) fn table(
        &mut self,
        name: &str,
        fields: &[&str],
        rows: Vec<Vec<u32>>,
    ) -> Result<usize> {
        if rows.iter().any(|r| r.len() != fields.len()) {
            return Err("record field/schema mismatch".into());
        }
        self.add(
            name,
            "u32",
            rows.len(),
            fields.len(),
            fields,
            rows.into_iter().flatten().collect(),
        )
    }
    pub(crate) fn zero(
        &mut self,
        name: &str,
        dtype: &'static str,
        r: usize,
        c: usize,
    ) -> Result<usize> {
        self.add(name, dtype, r, c, &[], vec![])
    }
}
#[derive(Serialize)]
pub struct Stage {
    pub start: usize,
    pub count: usize,
    pub levels: Vec<usize>,
}
#[derive(Serialize)]
pub struct GpuProgram {
    pub version: u32,
    pub layout: BTreeMap<String, Matrix>,
    pub data: Vec<u32>,
    pub shader: String,
    pub stages: Vec<Stage>,
    pub epochs: Vec<[usize; 2]>,
    pub groups: usize,
    pub evidence: Value,
    pub numeric: bool,
}
const NO: u32 = u32::MAX;
const WIRE_FIELDS: &[&str] = &["cell", "node"];
const PAIR_FIELDS: &[&str] = &["condition", "left", "right"];
impl Checked {
    pub fn lower(&self) -> Result<GpuProgram> {
        self.lower_with_layout(&crate::layout::Plan::uniform(
            self,
            crate::layout::Method::Gram,
        ))
    }
    pub fn lower_with_layout(&self, plan: &crate::layout::Plan) -> Result<GpuProgram> {
        use crate::layout::Method;
        plan.validate(self)?;
        let p = &self.program;
        let mut a = Arena::default();
        let cells = p.cells.len();
        let nodes = p.nodes.len();
        let conds = p.conditions.len();
        let mut binding = vec![];
        let mut partitions = vec![];
        for c in &p.cells {
            let mut row = c.binding.clone().unwrap_or_default();
            row.resize(p.slots, 0);
            binding.extend(row);
            let mut row = c.partition.iter().map(|v| *v as u32).collect::<Vec<_>>();
            row.resize(p.slots, NO);
            partitions.extend(row);
        }
        a.add("bindings", "u32", cells, p.slots, &[], binding)?;
        a.add("partitions", "u32", cells, p.slots, &[], partitions)?;
        a.table(
            "cell_meta",
            &["arity", "initial", "template"],
            p.cells
                .iter()
                .map(|c| {
                    vec![
                        c.partition.len() as u32,
                        u32::from(c.binding.is_some()),
                        c.template as u32,
                    ]
                })
                .collect(),
        )?;
        a.zero("live", "u32", cells, 1)?;
        a.zero("dirty_ports", "u32", cells, p.slots)?;
        a.zero("features", "f32", cells, self.width)?;
        a.zero("valid", "u32", cells, nodes)?;
        a.zero("needed", "u32", cells, nodes)?;
        a.table(
            "feature_ops",
            &["op", "offset", "width", "a", "b", "slot"],
            p.nodes
                .iter()
                .enumerate()
                .map(|(n, s)| {
                    vec![
                        match s.op {
                            Op::Port => 0,
                            Op::Lift2 => 1,
                            Op::Mean => 2,
                            Op::Concat => 3,
                            Op::Moments => 4,
                        },
                        self.features[n].offset as u32,
                        self.features[n].width as u32,
                        s.args.first().map_or(NO, |n| *n as u32),
                        s.args.get(1).map_or(NO, |n| *n as u32),
                        s.slot as u32,
                    ]
                })
                .collect(),
        )?;
        a.add(
            "supports",
            "u32",
            nodes,
            p.slots,
            &[],
            self.features
                .iter()
                .flat_map(|f| (0..p.slots).map(|s| u32::from(f.support.contains(&s))))
                .collect(),
        )?;
        a.table(
            "condition_ops",
            &["lc", "ln", "rc", "rn", "known"],
            p.conditions
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    vec![
                        c.left.0 as u32,
                        c.left.1 as u32,
                        c.right.0 as u32,
                        c.right.1 as u32,
                        self.known[i],
                    ]
                })
                .collect(),
        )?;
        a.add("evidence", "u32", conds, 1, &[], self.known.clone())?;
        a.zero("requested", "u32", conds, 1)?;
        let (mut sources, mut branches, mut reqs, mut gathers, mut chans) =
            (vec![], vec![], vec![], vec![], vec![]);
        let (mut specializations, mut alias_partitions, mut projections) = (vec![], vec![], vec![]);
        for c in &p.channels {
            let so = sources.len();
            sources.extend(c.sources.iter().map(|v| vec![*v as u32]));
            let bo = branches.len();
            for b in &c.alternatives {
                branches.push(vec![reqs.len() as u32, b.len() as u32]);
                reqs.extend(b.iter().map(|r| vec![r.condition as u32, r.depth as u32]));
            }
            let go = gathers.len();
            if let Some(t) = &c.target {
                gathers.extend(t.bindings.iter().map(|w| vec![w.0 as u32, w.1 as u32]));
            }
            let sp_start = specializations.len();
            if let Some(t) = &c.target {
                for sp in &t.specializations {
                    specializations.push(vec![
                        sp.cell as u32,
                        alias_partitions.len() as u32,
                        sp.partition.len() as u32,
                        projections.len() as u32,
                        sp.projection.len() as u32,
                    ]);
                    alias_partitions.extend(sp.partition.iter().map(|v| vec![*v as u32]));
                    projections.extend(sp.projection.iter().map(|v| vec![*v as u32]));
                }
            }
            chans.push(vec![
                so as u32,
                c.sources.len() as u32,
                bo as u32,
                c.alternatives.len() as u32,
                c.target.as_ref().map_or(NO, |t| t.cell as u32),
                go as u32,
                c.target.as_ref().map_or(0, |t| t.bindings.len() as u32),
                sp_start as u32,
                (specializations.len() - sp_start) as u32,
                u32::from(c.target.as_ref().is_some_and(|t| t.certified)),
            ]);
        }
        a.table(
            "specializations",
            &[
                "target",
                "partition_start",
                "arity",
                "projection_start",
                "output_arity",
            ],
            specializations,
        )?;
        a.table("alias_partitions", &["group"], alias_partitions)?;
        a.table("projections", &["slot"], projections)?;
        a.table("sources", &["cell"], sources)?;
        a.table("branches", &["start", "count"], branches)?;
        a.table("requirements", &["condition", "depth"], reqs)?;
        a.table("gathers", &["cell", "slot"], gathers)?;
        a.table(
            "channel_ops",
            &[
                "source_start",
                "source_count",
                "branch_start",
                "branch_count",
                "target",
                "gather_start",
                "gather_count",
                "specialization_start",
                "specialization_count",
                "certified",
            ],
            chans,
        )?;
        a.zero("decisions", "u32", p.channels.len(), 1)?;
        a.table(
            "execution_counts",
            &["classifications", "matches"],
            vec![vec![0, 0]; p.channels.len()],
        )?;
        a.zero("target_status", "u32", p.channels.len(), 1)?;
        a.add(
            "specialization_targets",
            "u32",
            p.channels.len(),
            1,
            &[],
            vec![NO; p.channels.len()],
        )?;
        let mut stages = vec![];
        let mut channel_ids = vec![];
        for (i, stage) in p.stages.iter().enumerate() {
            stages.push(Stage {
                start: channel_ids.len(),
                count: stage.len(),
                levels: self.levels[i].clone(),
            });
            channel_ids.extend(stage.iter().map(|i| *i as u32));
        }
        a.add(
            "stage_channels",
            "u32",
            channel_ids.len(),
            1,
            &[],
            channel_ids,
        )?;
        let mut epochs = vec![[0, 0]];
        let mut updates = vec![];
        for us in &p.updates {
            epochs.push([updates.len(), us.len()]);
            updates.extend(
                us.iter()
                    .map(|u| vec![u.cell as u32, u.slot as u32, u.value]),
            );
        }
        a.table("updates", &["cell", "slot", "value"], updates)?;
        a.add(
            "histogram",
            "u32",
            1,
            3,
            &["continued", "complete", "rejected"],
            vec![],
        )?;
        a.add(
            "counters",
            "u32",
            1,
            9,
            &[
                "matrix_calls",
                "matrix_positions",
                "matrix_macs",
                "requested_conditions",
                "computed_features",
                "reused_features",
                "feature_components_written",
                "feature_adds",
                "feature_multiplies",
            ],
            vec![],
        )?;
        let mut groups = vec![];
        a.zero("layout_counters", "u32", 1, 3)?;
        for (gi, block) in plan.blocks.iter().enumerate() {
            let mut left = block
                .conditions
                .iter()
                .map(|i| p.conditions[*i].left)
                .collect::<Vec<_>>();
            let mut right = block
                .conditions
                .iter()
                .map(|i| p.conditions[*i].right)
                .collect::<Vec<_>>();
            left.sort();
            left.dedup();
            right.sort();
            right.dedup();
            let nl = left.len();
            let nr = right.len();
            let mut ws = left.clone();
            ws.extend(&right);
            if block.method != Method::Rectangular {
                ws.sort();
                ws.dedup();
            }
            let width = self.features[ws[0].1].width;
            let w = ws.len();
            let rc = (width + 3) / 4 * 4;
            let gc = (if block.method == Method::Rectangular {
                nr
            } else {
                w
            } + 3)
                / 4
                * 4;
            let paired = block.method == Method::Paired;
            let wires = a.table(
                &format!("g{gi}_wires"),
                WIRE_FIELDS,
                ws.iter().map(|w| vec![w.0 as u32, w.1 as u32]).collect(),
            )?;
            let map = a.zero(&format!("g{gi}_map"), "u32", w, 1)?;
            let packed = a.zero(&format!("g{gi}_packed"), "u32", w, 1)?;
            let count = a.zero(&format!("g{gi}_count"), "u32", 1, 2)?;
            let rows = a.zero(
                &format!("g{gi}_rows"),
                "f32",
                if paired { 0 } else { w },
                rc,
            )?;
            let norm = a.zero(&format!("g{gi}_norm"), "f32", if paired { 0 } else { w }, 1)?;
            let gram = a.zero(
                &format!("g{gi}_gram"),
                "f32",
                if paired {
                    0
                } else if block.method == Method::Rectangular {
                    nl
                } else {
                    w
                },
                gc,
            )?;
            let pairs = a.table(
                &format!("g{gi}_conditions"),
                PAIR_FIELDS,
                block
                    .conditions
                    .iter()
                    .map(|i| {
                        let c = &p.conditions[*i];
                        vec![
                            *i as u32,
                            if block.method == Method::Rectangular {
                                left.binary_search(&c.left).unwrap()
                            } else {
                                ws.binary_search(&c.left).unwrap()
                            } as u32,
                            if block.method == Method::Rectangular {
                                nl + right.binary_search(&c.right).unwrap()
                            } else {
                                ws.binary_search(&c.right).unwrap()
                            } as u32,
                        ]
                    })
                    .collect(),
            )?;
            groups.push(
                vec![
                    width,
                    w,
                    wires,
                    map,
                    packed,
                    count,
                    rows,
                    rc,
                    norm,
                    gram,
                    gc,
                    pairs,
                    block.conditions.len(),
                    match block.method {
                        Method::Gram => 0,
                        Method::Paired => 1,
                        Method::Rectangular => 2,
                    },
                    nl,
                ]
                .into_iter()
                .map(|v| v as u32)
                .collect(),
            );
        }
        a.table(
            "groups",
            &[
                "width",
                "wire_count",
                "wires",
                "map",
                "packed",
                "count",
                "rows",
                "row_cols",
                "norm",
                "gram",
                "gram_cols",
                "conditions",
                "condition_count",
                "method",
                "left_size",
            ],
            groups,
        )?;
        crate::numeric_lower::lower(&mut a, p.numeric.as_ref(), p.channels.len())?;
        if a.data.len() >= u32::MAX as usize {
            return Err("matrix address space exhausted".into());
        }
        let mut shader = format!(
            "#define BITS {}u\n#define CELL_COUNT {}u\n#define NODE_COUNT {}u\n#define CONDITION_COUNT {}u\n",
            p.bits, cells, nodes, conds
        );
        for (name, m) in &a.layout {
            let name = name.to_uppercase();
            shader += &format!(
                "#define {name}_OFF {}u\n#define {name}_ROWS {}u\n#define {name}_COLS {}u\n",
                m.offset, m.rows, m.cols
            );
            for (i, f) in m.fields.iter().enumerate() {
                shader += &format!("#define {name}_{} {}u\n", f.to_uppercase(), i);
            }
        }
        for (name, fields) in [("WIRE", WIRE_FIELDS), ("PAIR", PAIR_FIELDS)] {
            shader += &format!("#define {name}_COLS {}u\n", fields.len());
            for (i, f) in fields.iter().enumerate() {
                shader += &format!("#define {name}_{} {}u\n", f.to_uppercase(), i);
            }
        }
        shader += &format!(
            "#define NUMERIC_ENABLED {}u\n",
            u32::from(p.numeric.is_some())
        );
        shader += include_str!("../matrix.metal");
        shader += include_str!("../numeric.metal");
        Ok(GpuProgram {
            version: 5,
            layout: a.layout,
            data: a.data,
            shader,
            stages,
            epochs,
            groups: plan.blocks.len(),
            numeric: p.numeric.is_some(),
            evidence: json!({"source":p.evidence,"templates":p.templates,"physical_layout":plan,"scope":"finite baked coordinate program; explicit boundary channels; no global egglog claim"}),
        })
    }
}
