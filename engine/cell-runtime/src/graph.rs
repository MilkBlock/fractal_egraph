//! Rows are temporary views of shared recipes, never a constructor row store.
use crate::program::*;
use crate::value::{self, Parents, Value, canon, exported, root};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FactRef {
    pub cell: u32,
    pub op: u32,
}
/// Copied fields only need source-owner identities for containment candidates.
/// Provider projection additionally needs atom/operation coordinates.
#[derive(Clone, Debug)]
pub enum MatchedFacts {
    None,
    Owners(Vec<u32>),
    Rows(Vec<Option<Dependency>>),
}
impl MatchedFacts {
    pub fn new(atoms: usize, full: bool) -> Self {
        if atoms == 0 {
            Self::None
        } else if full {
            Self::Rows(vec![None; atoms])
        } else {
            Self::Owners(vec![0; atoms])
        }
    }
    pub fn record(&mut self, atom: usize, row: Option<Dependency>) {
        match self {
            Self::None => {}
            Self::Rows(rows) => rows[atom] = row,
            Self::Owners(owners) => {
                owners[atom] = match row {
                    Some(Dependency::Fact(f)) => f.cell,
                    _ => 0,
                }
            }
        }
    }
    pub fn get(&self, atom: usize) -> Option<Dependency> {
        match self {
            Self::Rows(rows) => rows.get(atom).copied().flatten(),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactState {
    // Canonicalized on read, like an explicit Unit-function key. Reconstructed
    // from the checked recipe on checkpoint restore; never trusted serialized data.
    #[serde(skip)]
    pub marker_row: Option<MarkerRow>,
    #[serde(default)]
    pub entry: Option<FactRef>,
    pub hidden: bool,
    pub epoch: u64,
    pub born: u64,
    pub hash: u64,
}
#[derive(Clone, Debug)]
pub struct MarkerRow {
    pub table: usize,
    pub values: Vec<Value>,
    pub canonical: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cell {
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub composition: Option<crate::compositions::Cursor>,
    pub template: usize,
    pub parent: Option<FactRef>,
    pub values: BTreeMap<usize, Value>,
    pub facts: BTreeMap<u32, FactState>,
    #[serde(default)]
    pub recipe_depth: u32,
    #[serde(default)]
    pub providers: BTreeMap<usize, FactRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Function {
    pub table: usize,
    pub key: Vec<Value>,
    pub value: Value,
    pub epoch: u64,
    pub live: bool,
    #[serde(default)]
    pub origin: Option<FactRef>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Dependency {
    Fact(FactRef),
    Function(usize),
}
#[derive(Clone, Debug)]
pub struct Row {
    pub table: usize,
    pub values: Vec<Value>,
    pub hidden: bool,
    pub epoch: u64,
    pub witness: Dependency,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Stats {
    pub constructor_requests: u64,
    pub constructor_reused: u64,
    pub logical_facts_created: u64,
    pub function_updates: u64,
    pub merge_calls: u64,
    #[serde(default)]
    pub key_operations: u64,
    #[serde(default)]
    pub merge_port_reads: u64,
    #[serde(default)]
    pub merge_contract_calls: BTreeMap<String, u64>,
    pub unions: u64,
    pub repaired_dependencies: u64,
    pub decoded_rows: u64,
    pub residual_candidates: u64,
    pub bindings: u64,
    pub direct_bindings: u64,
    pub cells_created: u64,
    pub empty_frames_released: u64,
    pub native_table_reads: u64,
    pub native_table_writes: u64,
    pub native_free_join_calls: u64,
    #[serde(default)]
    pub delivery_events: u64,
    #[serde(default)]
    pub distinct_delivery_events: u64,
    #[serde(default)]
    pub union_requests: u64,
    #[serde(default)]
    pub repair_key_hash_changes: u64,
    #[serde(default)]
    pub repair_collisions: u64,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct CongruenceStats {
    pub instance_visits: u64,
    pub guard_tests: u64,
    pub unavailable_proofs: u64,
    pub proven_unions: u64,
    #[serde(default)]
    pub coalesced_facts: u64,
}
pub struct Graph {
    pub key_lookup_calls: std::cell::Cell<u64>,
    pub program: Program,
    pub views_enabled: bool,
    pub marker_fields_copied: bool,
    pub absorption_enabled: bool,
    pub view_stats: crate::views::ViewStats,
    retired_contexts: BTreeMap<u32, crate::views::RetiredContext>,
    fact_forwards: BTreeMap<FactRef, FactRef>,
    reference_users: BTreeMap<FactRef, usize>,
    absorption_candidates: BTreeSet<(u32, u32)>,
    pub cells: BTreeMap<u32, Cell>,
    pub functions: Vec<Function>,
    pub parents: Parents,
    pub epoch: u64,
    pub changed: bool,
    pub stats: Stats,
    pub pending_unions: Vec<(u64, u64)>,
    pub events: Vec<Dependency>,
    pub anchors: Vec<(String, String, u64)>,
    pub next_cell: u32,
    pub query_versions: Vec<u64>,
    pub query_changes: HashMap<Dependency, usize>,
    pub congruence_enabled: bool,
    pub congruence_stats: CongruenceStats,
    congruence_dirty: BTreeSet<u32>,
    operators: Vec<BTreeSet<FactRef>>,
    function_operators: Vec<BTreeSet<usize>>,
    intern: HashMap<u64, BTreeSet<FactRef>>,
    function_keys: BTreeMap<(usize, Vec<Value>), usize>,
    uses: BTreeMap<u64, BTreeSet<Dependency>>,
    dependencies: BTreeMap<Dependency, BTreeSet<u64>>,
    dirty: BTreeSet<Dependency>,
    dirty_markers: BTreeSet<FactRef>,
    dirty_facts: Vec<BTreeSet<FactRef>>,
    parent_users: BTreeMap<u32, usize>,
    garbage: BTreeSet<u32>,
}
fn hashkey(table: usize, key: &[Value]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    table.hash(&mut h);
    key.hash(&mut h);
    h.finish()
}
impl Graph {
    pub fn image(&self) -> J {
        json!({"key_lookup_calls":self.key_lookup_calls.get(),"cells":self.cells,"functions":self.functions,"parents":self.parents,"epoch":self.epoch,"stats":self.stats,"anchors":self.anchors,"next_cell":self.next_cell,"congruence_stats":self.congruence_stats,"reference_views_enabled":self.views_enabled,"marker_fields_copied":self.marker_fields_copied,"view_stats":self.view_stats,"retired_contexts":self.retired_contexts,"fact_forwards":self.fact_forwards.iter().collect::<Vec<_>>()})
    }
    pub fn restore(program: Program, image: &J) -> Result<Self> {
        fn decode<T: serde::de::DeserializeOwned>(j: &J) -> Result<T> {
            serde_json::from_value(j.clone()).map_err(|e| e.to_string())
        }
        let mut empty = program.clone();
        empty.initial.clear();
        let mut g = Self::new(empty)?;
        g.program = program;
        g.cells = decode(&image["cells"])?;
        g.next_cell = decode(&image["next_cell"])?;
        if image["reference_views_enabled"].as_bool().unwrap_or(false) != g.views_enabled {
            return Err("checkpoint reference storage mode differs".into());
        }
        if image["marker_fields_copied"].as_bool().unwrap_or(false) != g.marker_fields_copied {
            return Err("checkpoint marker field storage mode differs".into());
        }
        if g.views_enabled {
            for t in &g.program.views.markers {
                g.program.tables[*t].virtual_fact = true;
            }
        }
        if !image["view_stats"].is_null() {
            g.view_stats = decode(&image["view_stats"])?;
        }
        if !image["retired_contexts"].is_null() {
            g.retired_contexts = decode(&image["retired_contexts"])?;
        }
        if g.retired_contexts.iter().any(|(id, c)| {
            *id >= g.next_cell
                || c.template >= g.program.templates.len()
                || g.cells.contains_key(id)
        }) {
            return Err("invalid retired context".into());
        }
        if !image["fact_forwards"].is_null() {
            let pairs: Vec<(FactRef, FactRef)> = decode(&image["fact_forwards"])?;
            for (old, new) in pairs {
                if old.cell >= new.cell || g.fact_forwards.insert(old, new).is_some() {
                    return Err("invalid forward reference".into());
                }
            }
        }
        for (&id, c) in &g.cells {
            for atom in c.providers.keys() {
                if !g
                    .program
                    .views
                    .layouts
                    .get(&c.template)
                    .is_some_and(|l| l.carriers.iter().any(|r| r.atom == *atom))
                {
                    return Err("unknown checkpoint provider".into());
                }
            }
            for f in c.parent.iter().chain(c.providers.values()) {
                let f = g.resolve_reference(*f);
                if f.cell == id
                    || !g.cells.contains_key(&f.cell)
                    || g.dependency_table(Dependency::Fact(f)).is_none()
                {
                    return Err("invalid live Cell reference".into());
                }
            }
        }
        let references = g
            .cells
            .values()
            .flat_map(|c| c.parent.iter().chain(c.providers.values()))
            .copied()
            .collect::<Vec<_>>();
        for f in references {
            g.retain_reference(f);
        }
        // Forwarding may point to a newer owner. Check actual dependency cycles,
        // rather than assuming that every parent has an older numeric ID.
        fn visit(
            g: &Graph,
            id: u32,
            active: &mut BTreeSet<u32>,
            done: &mut BTreeSet<u32>,
        ) -> Result<()> {
            if done.contains(&id) {
                return Ok(());
            }
            if !active.insert(id) {
                return Err("cyclic Cell references".into());
            }
            let c = &g.cells[&id];
            for f in c.parent.iter().chain(c.providers.values()) {
                visit(g, g.resolve_reference(*f).cell, active, done)?;
            }
            active.remove(&id);
            done.insert(id);
            Ok(())
        }
        let mut done = BTreeSet::new();
        for id in g.cells.keys() {
            visit(&g, *id, &mut BTreeSet::new(), &mut done)?;
        }
        g.functions = decode(&image["functions"])?;
        g.parents = decode(&image["parents"])?;
        g.epoch = decode(&image["epoch"])?;
        g.stats = decode(&image["stats"])?;
        g.key_lookup_calls
            .set(image["key_lookup_calls"].as_u64().unwrap_or(0));
        if !image["congruence_stats"].is_null() {
            g.congruence_stats = decode(&image["congruence_stats"])?;
        }
        g.anchors = decode(&image["anchors"])?;
        g.next_cell = decode(&image["next_cell"])?;
        if g.parents.iter().any(|(a, b)| b >= a) || g.cells.keys().any(|i| *i >= g.next_cell) {
            return Err("invalid checkpoint identities".into());
        }
        let mut facts = vec![];
        for (&id, c) in &g.cells {
            if c.template >= g.program.templates.len()
                || c.parent
                    .is_some_and(|p| !g.cells.contains_key(&g.resolve_reference(p).cell))
            {
                return Err("invalid checkpoint Cell reference".into());
            }
            if c.facts.keys().any(|i| {
                *i as usize >= g.program.templates[c.template].ops.len()
                    && g.carrier(FactRef { cell: id, op: *i }).is_none()
            }) {
                return Err("invalid checkpoint fact position".into());
            }
            facts.extend(c.facts.keys().map(|op| FactRef { cell: id, op: *op }));
        }
        for f in facts {
            let row = g.fact(f)?.ok_or("checkpoint fact missing")?;
            if g.marker_fields_copied && g.program.views.markers.contains(&row.table) {
                g.cells
                    .get_mut(&f.cell)
                    .unwrap()
                    .facts
                    .get_mut(&f.op)
                    .unwrap()
                    .marker_row = Some(MarkerRow {
                    table: row.table,
                    values: row.values.clone(),
                    canonical: true,
                });
            }
            let h = hashkey(row.table, &row.values[..row.values.len() - 1]);
            g.cells
                .get_mut(&f.cell)
                .unwrap()
                .facts
                .get_mut(&f.op)
                .unwrap()
                .hash = h;
            g.operators[row.table].insert(f);
            g.intern.entry(h).or_default().insert(f);
            g.subscribe(row.witness, &row.values);
            g.events.push(row.witness);
        }
        for i in 0..g.functions.len() {
            if let Some(row) = g.function(i) {
                if let Some(origin) = g.functions[i].origin {
                    let cell = g
                        .cells
                        .get(&origin.cell)
                        .ok_or("expired function producer")?;
                    if !matches!(g.program.templates[cell.template].ops.get(origin.op as usize),Some(Op::Set(t,_,_)) if *t==row.table)
                    {
                        return Err("invalid function producer recipe".into());
                    }
                    *g.parent_users.entry(origin.cell).or_default() += 1;
                }
                g.function_operators[row.table].insert(i);
                let key = (row.table, row.values[..row.values.len() - 1].to_vec());
                if g.function_keys.insert(key, i).is_some() {
                    return Err("checkpoint duplicate function key".into());
                }
                g.subscribe(row.witness, &row.values);
                g.events.push(row.witness);
            }
        }
        Ok(g)
    }
    pub fn new(program: Program) -> Result<Self> {
        let enabled = std::env::var("CELL_CPU_REFERENCE_VIEWS").as_deref() == Ok("1");
        Self::new_with_reference_views(program, enabled)
    }
    pub fn new_with_reference_views(mut program: Program, enabled: bool) -> Result<Self> {
        let views_enabled = enabled && !program.views.layouts.is_empty();
        let marker_fields_copied = views_enabled
            && match std::env::var("CELL_CPU_MARKER_FIELDS").as_deref() {
                Ok("copy") | Err(_) => true,
                Ok("reference") => false,
                Ok(_) => return Err("CELL_CPU_MARKER_FIELDS must be copy or reference".into()),
            };
        if views_enabled {
            for t in &program.views.markers {
                program.tables[*t].virtual_fact = true;
            }
        }
        let n = program.tables.len();
        let initial = program.initial.clone();
        let congruence_enabled = std::env::var("CELL_CPU_CONGRUENCE").as_deref() == Ok("1")
            && program.congruence.reorder_safe;
        let mut g = Self {
            key_lookup_calls: std::cell::Cell::new(0),
            views_enabled,
            marker_fields_copied,
            absorption_enabled: views_enabled
                && std::env::var("CELL_CPU_ABSORB").as_deref() != Ok("0"),
            view_stats: Default::default(),
            retired_contexts: BTreeMap::new(),
            fact_forwards: BTreeMap::new(),
            reference_users: BTreeMap::new(),
            absorption_candidates: BTreeSet::new(),
            program,
            cells: BTreeMap::new(),
            functions: vec![],
            parents: BTreeMap::new(),
            epoch: 0,
            changed: false,
            stats: Stats::default(),
            pending_unions: vec![],
            events: vec![],
            anchors: vec![],
            next_cell: 1,
            query_versions: vec![0; n],
            query_changes: HashMap::new(),
            congruence_enabled,
            congruence_stats: CongruenceStats::default(),
            congruence_dirty: BTreeSet::new(),
            operators: vec![BTreeSet::new(); n],
            function_operators: vec![BTreeSet::new(); n],
            intern: HashMap::new(),
            function_keys: BTreeMap::new(),
            uses: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            dirty: BTreeSet::new(),
            dirty_markers: BTreeSet::new(),
            dirty_facts: vec![BTreeSet::new(); n],
            parent_users: BTreeMap::new(),
            garbage: BTreeSet::new(),
        };
        let mut ids = BTreeMap::new();
        for r in &initial {
            let table = g
                .program
                .tables
                .iter()
                .position(|t| r["table"] == t.name)
                .ok_or("unknown initial operator")?;
            let vals = array(&r["values"])?
                .iter()
                .map(|v| Self::import(v, &mut ids))
                .collect::<Result<Vec<_>>>()?;
            if vals.len() != g.program.tables[table].schema.len() {
                return Err("initial arity".into());
            }
            let key = vals[..vals.len() - 1].to_vec();
            let val = vals.last().unwrap().clone();
            if g.program.tables[table].virtual_fact {
                let template = g.program.table_templates[table];
                let cell = g.frame(template, (0..vals.len()).zip(vals).collect())?;
                g.activate(
                    FactRef { cell, op: 0 },
                    table,
                    &key,
                    val,
                    r["subsumed"].as_bool().unwrap_or(false),
                )?;
            } else {
                g.set_function(table, key, val)?;
            }
        }
        g.anchors = ids
            .into_iter()
            .map(|((sort, old), id)| (sort, old, id))
            .collect();
        g.repair()?;
        g.changed = false;
        Ok(g)
    }
    fn import(j: &J, ids: &mut BTreeMap<(String, String), u64>) -> Result<Value> {
        if j.get("key").is_some() {
            return Ok(Value::Key(eggfuse_structural_key::Key::from_wire(
                serde_json::from_value(j["key"].clone()).map_err(|e| e.to_string())?,
            )?));
        }
        if let Some(eq) = j["eq"].as_str() {
            let next = ids.len() as u64 + 1;
            if next >= 1u64 << 32 {
                return Err("initial identity capacity".into());
            }
            return Ok(Value::Eq(
                *ids.entry((string(&j["sort"])?.into(), eq.into()))
                    .or_insert(next),
            ));
        }
        if let Some(xs) = j["elements"].as_array() {
            let mut xs = xs
                .iter()
                .map(|x| Self::import(x, ids))
                .collect::<Result<Vec<_>>>()?;
            xs.sort();
            xs.dedup();
            return Ok(Value::Set(xs));
        }
        Ok(match string(&j["sort"])? {
            "String" => Value::Text(string(&j["value"])?.into()),
            "f64" => Value::Float(j["value"].as_i64().ok_or("f64 bits")?),
            _ => Value::Int(
                j["value"]
                    .as_i64()
                    .or_else(|| j["value"].as_str().and_then(|s| s.parse().ok()))
                    .ok_or("initial scalar")?,
            ),
        })
    }
    pub fn frame(&mut self, template: usize, values: BTreeMap<usize, Value>) -> Result<u32> {
        let id = self.next_cell;
        self.next_cell = self
            .next_cell
            .checked_add(1)
            .ok_or("Cell identity exhausted")?;
        self.cells.insert(
            id,
            Cell {
                completed: false,
                composition: None,
                template,
                parent: None,
                values,
                facts: BTreeMap::new(),
                recipe_depth: 0,
                providers: BTreeMap::new(),
            },
        );
        self.stats.cells_created += 1;
        Ok(id)
    }
    pub fn linked_frame(
        &mut self,
        template: usize,
        mut values: BTreeMap<usize, Value>,
        parent: Option<FactRef>,
    ) -> Result<u32> {
        let parent = parent.filter(|_| !self.program.templates[template].parent_columns.is_empty());
        if parent.is_some() {
            for slot in self.program.templates[template].parent_columns.keys() {
                values.remove(slot);
            }
        }
        let id = self.frame(template, values)?;
        self.cells.get_mut(&id).unwrap().parent = parent;
        if let Some(p) = parent {
            self.retain_reference(p);
        }
        Ok(id)
    }
    pub fn argument(&self, cell: u32, a: &Arg) -> Result<Value> {
        match a {
            Arg::Literal(v) => Ok(canon(v, &self.parents)),
            Arg::Slot(i) => self.slot(cell, *i),
        }
    }
    pub fn slot(&self, cell: u32, i: usize) -> Result<Value> {
        let c = self.cells.get(&cell).ok_or("expired Cell reference")?;
        self.slot_in(cell, c, i)
    }
    fn argument_in(&self, cell: u32, c: &Cell, a: &Arg) -> Result<Value> {
        match a {
            Arg::Literal(v) => Ok(canon(v, &self.parents)),
            Arg::Slot(i) => self.slot_in(cell, c, *i),
        }
    }
    fn slot_in(&self, cell: u32, c: &Cell, i: usize) -> Result<Value> {
        if let Some(v) = c.values.get(&i) {
            return Ok(canon(v, &self.parents));
        }
        if let Some(&(atom, col)) = self
            .program
            .views
            .layouts
            .get(&c.template)
            .and_then(|l| l.slots.get(&i))
        {
            if let Some(f) = c.providers.get(&atom) {
                return self.fact_column(*f, col);
            }
        }
        let t = &self.program.templates[c.template];
        if let (Some(parent), Some(&col)) = (c.parent, t.parent_columns.get(&i)) {
            return self.fact_column(parent, col);
        }
        let op = &t.ops[*t.defs.get(&i).ok_or("unready slot")?];
        match op {
            Op::Copy(_, a) => self.argument_in(cell, c, a),
            Op::Let(_, Call::Table(t), _) if self.program.tables[*t].virtual_fact => {
                Ok(if self.program.tables[*t].constructor {
                    Value::Eq(root(&self.parents, ((cell as u64) << 32) | (i as u64 + 1)))
                } else {
                    Value::Int(0)
                })
            }
            _ => Err("unready computed/function slot".into()),
        }
    }
    fn fact_column(&self, f: FactRef, col: usize) -> Result<Value> {
        let f = self.resolve_reference(f);
        if let Some(c) = self.carrier(f) {
            return self.argument(f.cell, c.columns.get(col).ok_or("carrier column")?);
        }
        // Recipe parameters are immutable even after this fact is hidden/deleted.
        // Derived Cells can keep their binding without copying the parent's fields.
        let c = self.cells.get(&f.cell).ok_or("expired parent Cell")?;
        match self.program.templates[c.template]
            .ops
            .get(f.op as usize)
            .ok_or("invalid retained fact position")?
        {
            Op::Let(dst, Call::Table(_), args) => {
                if col == args.len() {
                    self.slot(f.cell, *dst)
                } else {
                    self.argument(f.cell, args.get(col).ok_or("parent column")?)
                }
            }
            Op::Set(_, args, v) => self.argument(
                f.cell,
                if col == args.len() {
                    v
                } else {
                    args.get(col).ok_or("parent column")?
                },
            ),
            _ => Err("invalid parent fact".into()),
        }
    }
    pub fn arguments(&self, cell: u32, aa: &[Arg]) -> Result<Vec<Value>> {
        aa.iter().map(|a| self.argument(cell, a)).collect()
    }
    pub fn fact(&self, f: FactRef) -> Result<Option<Row>> {
        let Some(c) = self.cells.get(&f.cell) else {
            return Ok(None);
        };
        let Some(s) = c.facts.get(&f.op) else {
            return Ok(None);
        };
        if let Some(marker) = &s.marker_row {
            return Ok(Some(Row {
                table: marker.table,
                values: if marker.canonical {
                    marker.values.clone()
                } else {
                    marker
                        .values
                        .iter()
                        .map(|v| canon(v, &self.parents))
                        .collect()
                },
                hidden: s.hidden,
                epoch: s.epoch,
                witness: Dependency::Fact(f),
            }));
        }
        let arguments = |aa: &[Arg]| {
            aa.iter()
                .map(|a| self.argument_in(f.cell, c, a))
                .collect::<Result<Vec<_>>>()
        };
        let (table, values) = if let Some(carrier) = self.program.views.carrier(
            c.template,
            f.op as usize,
            self.program.templates[c.template].ops.len(),
        ) {
            (carrier.table, arguments(&carrier.columns)?)
        } else {
            match self.program.templates[c.template]
                .ops
                .get(f.op as usize)
                .ok_or("invalid live fact position")?
            {
                Op::Let(dst, Call::Table(t), aa) => {
                    let mut vals = arguments(aa)?;
                    vals.push(self.slot_in(f.cell, c, *dst)?);
                    (*t, vals)
                }
                Op::Set(t, aa, v) => {
                    let mut vals = arguments(aa)?;
                    vals.push(self.argument_in(f.cell, c, v)?);
                    (*t, vals)
                }
                _ => return Err("invalid fact recipe position".into()),
            }
        };
        // Every argument/slot path already returns a canonical value, including
        // recursive provider projections and literal/container arguments.
        Ok(Some(Row {
            table,
            values,
            hidden: s.hidden,
            epoch: s.epoch,
            witness: Dependency::Fact(f),
        }))
    }
    pub fn function(&self, i: usize) -> Option<Row> {
        let f = self.functions.get(i)?;
        if !f.live {
            return None;
        }
        let mut values = f
            .key
            .iter()
            .map(|v| canon(v, &self.parents))
            .collect::<Vec<_>>();
        values.push(canon(&f.value, &self.parents));
        Some(Row {
            table: f.table,
            values,
            hidden: false,
            epoch: f.epoch,
            witness: Dependency::Function(i),
        })
    }
    pub fn row(&self, w: Dependency) -> Result<Option<Row>> {
        match w {
            Dependency::Fact(f) => self.fact(f),
            Dependency::Function(i) => Ok(self.function(i)),
        }
    }
    /// Delivery only needs liveness and the table, not the decoded binding.
    pub fn live_table(&self, w: Dependency) -> Option<usize> {
        match w {
            Dependency::Function(i) => self.functions.get(i).filter(|f| f.live).map(|f| f.table),
            Dependency::Fact(f) => {
                let c = self.cells.get(&f.cell)?;
                let s = c.facts.get(&f.op)?;
                if let Some(marker) = &s.marker_row {
                    return Some(marker.table);
                }
                if let Some(carrier) = self.program.views.carrier(
                    c.template,
                    f.op as usize,
                    self.program.templates[c.template].ops.len(),
                ) {
                    return Some(carrier.table);
                }
                match self.program.templates[c.template].ops.get(f.op as usize)? {
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => Some(*t),
                    _ => None,
                }
            }
        }
    }
    pub fn origin(&self, w: Dependency) -> Option<FactRef> {
        match w {
            Dependency::Fact(f) => Some(f),
            Dependency::Function(i) => self
                .functions
                .get(i)
                .filter(|f| f.live)
                .and_then(|f| f.origin),
        }
    }
    fn function_origin(&mut self, index: usize, origin: Option<FactRef>) {
        if self.functions[index].origin == origin {
            return;
        }
        if let Some(old) = self.functions[index].origin.take() {
            *self
                .parent_users
                .get_mut(&old.cell)
                .expect("function producer reference") -= 1;
            self.garbage.insert(old.cell);
        }
        if let Some(new) = origin {
            *self.parent_users.entry(new.cell).or_default() += 1;
        }
        self.functions[index].origin = origin;
    }
    pub(crate) fn dependency_table(&self, w: Dependency) -> Option<usize> {
        match w {
            Dependency::Function(i) => self.functions.get(i).map(|f| f.table),
            Dependency::Fact(f) => {
                let c = self.cells.get(&f.cell)?;
                if let Some(carrier) = self.carrier(f) {
                    return Some(carrier.table);
                }
                match self.program.templates[c.template].ops.get(f.op as usize)? {
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => Some(*t),
                    _ => None,
                }
            }
        }
    }
    pub fn witnesses(&self, table: usize) -> Vec<Dependency> {
        if self.program.tables[table].virtual_fact {
            self.operators[table]
                .iter()
                .copied()
                .map(Dependency::Fact)
                .collect()
        } else {
            self.function_operators[table]
                .iter()
                .copied()
                .map(Dependency::Function)
                .collect()
        }
    }
    /// A query index contains references, never copies of virtual constructor rows.
    /// `uses` already follows unions for rebuild; a bound identity can also narrow
    /// a query. It is a superset (any column, including container members), so the
    /// matcher must still check all columns and visibility on the decoded row.
    pub fn witness_count(&self, table: usize) -> usize {
        if self.program.tables[table].virtual_fact {
            self.operators[table].len()
        } else {
            self.function_operators[table].len()
        }
    }
    fn bound_uses(&self, bound: &[Option<Value>]) -> Option<&BTreeSet<Dependency>> {
        static EMPTY: BTreeSet<Dependency> = BTreeSet::new();
        bound
            .iter()
            .filter_map(|v| match v {
                Some(Value::Eq(id)) => {
                    Some(self.uses.get(&root(&self.parents, *id)).unwrap_or(&EMPTY))
                }
                _ => None,
            })
            .min_by_key(|uses| uses.len())
    }
    pub fn candidate_count(&self, table: usize, bound: &[Option<Value>]) -> usize {
        let all = self.witness_count(table);
        self.bound_uses(bound)
            .map_or(all, |uses| all.min(uses.len()))
    }
    pub fn candidates(&self, table: usize, bound: &[Option<Value>]) -> Vec<Dependency> {
        if let Some(uses) = self.bound_uses(bound) {
            if uses.len() < self.witness_count(table) {
                return uses
                    .iter()
                    .copied()
                    .filter(|w| match w {
                        Dependency::Fact(f) => self.cells.get(&f.cell).is_some_and(|c| {
                            if let Some(carrier) = self.carrier(*f) {
                                return carrier.table == table;
                            }
                            match &self.program.templates[c.template].ops[f.op as usize] {
                                Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => *t == table,
                                _ => false,
                            }
                        }),
                        Dependency::Function(i) => {
                            self.functions[*i].live && self.functions[*i].table == table
                        }
                    })
                    .collect();
            }
        }
        self.witnesses(table)
    }
    pub fn lookup(&self, table: usize, key: &[Value]) -> Result<Option<Row>> {
        self.key_lookup_calls.set(self.key_lookup_calls.get() + 1);
        let key = key
            .iter()
            .map(|v| canon(v, &self.parents))
            .collect::<Vec<_>>();
        if !self.program.tables[table].virtual_fact {
            return Ok(self
                .function_keys
                .get(&(table, key))
                .and_then(|i| self.function(*i)));
        }
        let h = hashkey(table, &key);
        if self.dirty_facts[table].is_empty() {
            // Matching observes a repaired snapshot. Avoid allocating/copying
            // a set on every keyed read; retain the original FactRef order.
            for &f in self.intern.get(&h).into_iter().flatten() {
                if let Some(r) = self.fact(f)? {
                    if r.table == table && r.values[..key.len()] == key {
                        return Ok(Some(r));
                    }
                }
            }
            return Ok(None);
        }
        let mut refs = self.intern.get(&h).cloned().unwrap_or_default();
        // A merge callback may request a constructor whose old key is dirty. Include
        // only affected references until repair catches up, not all graph rows.
        refs.extend(self.dirty_facts[table].iter().copied());
        for f in refs {
            if let Some(r) = self.fact(f)? {
                if r.table == table && r.values[..key.len()] == key {
                    return Ok(Some(r));
                }
            }
        }
        Ok(None)
    }
    fn refs(v: &Value, out: &mut BTreeSet<u64>) {
        match v {
            Value::Eq(i) => {
                out.insert(*i);
            }
            Value::Set(xs) => {
                for x in xs {
                    Self::refs(x, out)
                }
            }
            _ => {}
        }
    }
    fn subscribe(&mut self, d: Dependency, values: &[Value]) {
        if let Some(table) = self.dependency_table(d) {
            self.query_versions[table] += 1;
            self.query_changes.insert(d, table);
        }
        if let Some(old) = self.dependencies.remove(&d) {
            for id in old {
                if let Some(uses) = self.uses.get_mut(&root(&self.parents, id)) {
                    uses.remove(&d);
                }
            }
        }
        let mut refs = BTreeSet::new();
        for v in values {
            Self::refs(&canon(v, &self.parents), &mut refs)
        }
        for id in &refs {
            self.uses.entry(*id).or_default().insert(d);
        }
        self.dependencies.insert(d, refs);
    }
    fn activate(
        &mut self,
        f: FactRef,
        t: usize,
        key: &[Value],
        v: Value,
        hidden: bool,
    ) -> Result<()> {
        let h = hashkey(t, key);
        let mut vs = key.to_vec();
        vs.push(v);
        self.cells.get_mut(&f.cell).unwrap().facts.insert(
            f.op,
            FactState {
                marker_row: (self.marker_fields_copied && self.program.views.markers.contains(&t))
                    .then(|| MarkerRow {
                        table: t,
                        values: vs.clone(),
                        canonical: true,
                    }),
                entry: None,
                hidden,
                epoch: self.epoch,
                born: self.epoch,
                hash: h,
            },
        );
        self.operators[t].insert(f);
        self.intern.entry(h).or_default().insert(f);
        self.subscribe(Dependency::Fact(f), &vs);
        self.events.push(Dependency::Fact(f));
        self.stats.logical_facts_created += 1;
        if self.views_enabled && self.program.views.markers.contains(&t) {
            self.view_stats.marker_rows += 1;
        }
        self.changed = true;
        Ok(())
    }
    pub fn construct(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        dst: usize,
    ) -> Result<Value> {
        let key = self.arguments(cell, aa)?;
        self.stats.constructor_requests += 1;
        if let Some(row) = self.lookup(t, &key)? {
            self.stats.constructor_reused += 1;
            let value = row.values.last().unwrap().clone();
            self.cells
                .get_mut(&cell)
                .unwrap()
                .values
                .insert(dst, value.clone());
            return Ok(value);
        }
        let value = self.slot(cell, dst)?;
        self.activate(
            FactRef {
                cell,
                op: op as u32,
            },
            t,
            &key,
            value.clone(),
            false,
        )?;
        Ok(value)
    }
    pub fn dynamic_construct(&mut self, t: usize, args: Vec<Value>) -> Result<Value> {
        if let Some(row) = self.lookup(t, &args)? {
            return Ok(row.values.last().unwrap().clone());
        }
        let dst = args.len();
        let cell = self.frame(
            self.program.table_templates[t],
            args.into_iter().enumerate().collect(),
        )?;
        self.construct(
            cell,
            0,
            t,
            &(0..dst).map(Arg::Slot).collect::<Vec<_>>(),
            dst,
        )
    }
    pub fn enqueue_union(&mut self, a: Value, b: Value) -> Result<()> {
        self.stats.union_requests += 1;
        let (Value::Eq(a), Value::Eq(b)) = (a, b) else {
            return Err("union requires equality values".into());
        };
        self.pending_unions.push((a, b));
        Ok(())
    }
    fn union(&mut self, a: u64, b: u64) {
        let a = root(&self.parents, a);
        let b = root(&self.parents, b);
        if a == b {
            return;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        self.parents.insert(hi, lo);
        let mut affected = self.uses.remove(&hi).unwrap_or_default();
        affected.extend(self.uses.get(&lo).into_iter().flatten().copied());
        self.mark_affected(&affected);
        self.uses.insert(lo, affected);
        self.stats.unions += 1;
        self.changed = true;
    }
    fn mark_affected(&mut self, affected: &BTreeSet<Dependency>) {
        for d in affected {
            let table = self.dependency_table(*d);
            if let Dependency::Fact(f) = d
                && self.views_enabled
                && table.is_some_and(|t| self.program.views.markers.contains(&t))
            {
                // Certified inert Unit markers cannot generate unions or be
                // observed by merge callbacks. Repair them after the ordinary
                // congruence closure, but before publishing this Run.
                self.dirty_markers.insert(*f);
            } else {
                self.dirty.insert(*d);
            }
            if let Dependency::Fact(f) = d {
                if let Some(marker) = self.marker_fields_copied.then(|| ()).and_then(|_| {
                    self.cells
                        .get_mut(&f.cell)
                        .and_then(|c| c.facts.get_mut(&f.op))
                        .and_then(|s| s.marker_row.as_mut())
                }) {
                    marker.canonical = false;
                }
                if self.congruence_enabled
                    && self.cells.get(&f.cell).is_some_and(|c| {
                        self.program.congruence.templates.contains_key(&c.template)
                    })
                {
                    self.congruence_dirty.insert(f.cell);
                }
                if let Some(table) = table {
                    self.dirty_facts[table].insert(*f);
                }
            }
        }
    }
    fn union_local_batch(&mut self, pairs: &[(u64, u64)]) {
        let mut affected = BTreeSet::new();
        for &(a, b) in pairs {
            let (a, b) = (root(&self.parents, a), root(&self.parents, b));
            if a == b {
                continue;
            }
            let (lo, hi) = (a.min(b), a.max(b));
            self.parents.insert(hi, lo);
            let from = self.uses.remove(&hi).unwrap_or_default();
            let target = self.uses.entry(lo).or_default();
            affected.extend(from.iter().copied());
            affected.extend(target.iter().copied());
            target.extend(from);
            self.stats.unions += 1;
            self.congruence_stats.proven_unions += 1;
            self.changed = true;
        }
        // No callback or query runs within a proved union batch. Dependencies
        // are notified once after all its internal equivalences are installed.
        self.mark_affected(&affected);
    }
    pub fn set(&mut self, cell: u32, op: usize, t: usize, aa: &[Arg], v: &Arg) -> Result<()> {
        let key = self.arguments(cell, aa)?;
        let value = self.argument(cell, v)?;
        if !self.program.tables[t].virtual_fact {
            let before = self.stats.function_updates;
            self.set_function(t, key.clone(), value)?;
            if self.stats.function_updates != before {
                let index = self.function_keys[&(t, key)];
                self.function_origin(
                    index,
                    Some(FactRef {
                        cell,
                        op: op as u32,
                    }),
                );
            }
            return Ok(());
        }
        if let Some(old) = self.lookup(t, &key)? {
            if self.program.tables[t].constructor {
                self.enqueue_union(old.values.last().unwrap().clone(), value)?
            } else if old.values.last() != Some(&value) {
                return Err("relation output conflict".into());
            }
        } else {
            self.activate(
                FactRef {
                    cell,
                    op: op as u32,
                },
                t,
                &key,
                value,
                false,
            )?;
        }
        Ok(())
    }
    fn merge_expression(&mut self, e: &J, old: &Value, new: &Value) -> Result<Value> {
        // Native ResolvedMergeFn::Function returns old on identical arguments.
        // Primitive merges do NOT have this shortcut: (+ old new) must execute.
        if let Some(v) = e["var"].as_str() {
            return match v {
                "old" => Ok(old.clone()),
                "new" => Ok(new.clone()),
                _ => Err("unknown merge variable".into()),
            };
        }
        if !e["literal"].is_null() {
            return value::literal(e);
        }
        let aa = array(&e["args"])?
            .iter()
            .map(|a| self.merge_expression(a, old, new))
            .collect::<Result<Vec<_>>>()?;
        if let Some(name) = e["call"]["table"].as_str() {
            let t = self
                .program
                .tables
                .iter()
                .position(|t| t.name == name)
                .ok_or("merge operator")?;
            if !self.program.tables[t].virtual_fact {
                self.stats.merge_port_reads += 1;
                return self.read_merge_port(t, &aa);
            }
            self.dynamic_construct(t, aa)
        } else {
            if e["call"]["primitive"]
                .as_str()
                .is_some_and(|n| n.starts_with("cell-key-"))
            {
                self.stats.key_operations += 1;
            }
            value::primitive(
                string(&e["call"]["primitive"])?,
                &aa,
                string(&e["call"]["output"])?,
                &self.program.artifact["source_core"]["structural_key"]["nodes"],
            )?
            .ok_or("merge arithmetic failed".into())
        }
    }
    fn merge_value(&mut self, t: usize, old: Value, new: Value) -> Result<Value> {
        self.stats.merge_calls += 1;
        if let Some(kind) = self.program.artifact["merge_contracts"]["tables"][t]["kind"].as_str() {
            *self
                .stats
                .merge_contract_calls
                .entry(kind.into())
                .or_default() += 1;
        }
        let expr = self.program.tables[t].merge.clone();
        if old == new
            && if self.program.artifact["merge_contracts"].is_null() {
                expr["call"]["table"].is_string()
            } else {
                self.program.artifact["merge_contracts"]["tables"][t]["equal_inputs"]
                    == "return_old"
            }
        {
            return Ok(old);
        }
        if expr.is_null() {
            return if old == new {
                Ok(old)
            } else {
                Err(format!(
                    "conflicting function {} without merge",
                    self.program.tables[t].name
                ))
            };
        }
        self.merge_expression(&expr, &old, &new)
    }
    fn read_merge_port(&mut self, t: usize, args: &[Value]) -> Result<Value> {
        let key = args
            .iter()
            .map(|v| canon(v, &self.parents))
            .collect::<Vec<_>>();
        let mut result: Option<Value> = None;
        // During repair the ordinary key directory may still contain displaced
        // keys. Resolve the complete canonical port, including pending pure
        // lattice collisions; never pick an arbitrary stale entry.
        for i in self.function_operators[t].iter().copied() {
            let Some(row) = self.function(i) else {
                continue;
            };
            if row.values[..key.len()] != key {
                continue;
            }
            let value = row.values.last().unwrap().clone();
            if let Some(old) = result.as_ref() {
                if *old == value {
                    continue;
                }
                let e = &self.program.tables[t].merge;
                let Some(op) = e["call"]["primitive"]
                    .as_str()
                    .filter(|op| matches!(*op, "min" | "max" | "or" | "and" | "cell-key-min"))
                else {
                    return Err("ambiguous non-lattice merge read".into());
                };
                let a = e["args"].as_array().ok_or("invalid lattice merge")?;
                if a.len() != 2
                    || !((a[0]["var"] == "old" && a[1]["var"] == "new")
                        || (a[0]["var"] == "new" && a[1]["var"] == "old"))
                {
                    return Err("ambiguous merge port requires a pure lattice contract".into());
                }
                result = value::primitive(
                    op,
                    &[old.clone(), value],
                    self.program.tables[t].schema.last().unwrap()["name"]
                        .as_str()
                        .unwrap(),
                    &self.program.artifact["source_core"]["structural_key"]["nodes"],
                )?;
            } else {
                result = Some(value);
            }
        }
        result.ok_or_else(|| format!("unready merge port {}", self.program.tables[t].name))
    }
    pub fn set_function(&mut self, t: usize, key: Vec<Value>, value: Value) -> Result<()> {
        let key = key
            .into_iter()
            .map(|v| canon(&v, &self.parents))
            .collect::<Vec<_>>();
        let value = canon(&value, &self.parents);
        let index = self.function_keys.get(&(t, key.clone())).copied();
        if let Some(i) = index {
            let old = canon(&self.functions[i].value, &self.parents);
            let next = self.merge_value(t, old.clone(), value)?;
            if next != old {
                self.functions[i].value = next;
                self.functions[i].epoch = self.epoch;
                self.changed = true;
                self.stats.function_updates += 1;
                self.events.push(Dependency::Function(i));
                let row = self.function(i).unwrap();
                self.subscribe(row.witness, &row.values);
            }
        } else {
            let i = self.functions.len();
            self.functions.push(Function {
                table: t,
                key: key.clone(),
                value,
                epoch: self.epoch,
                live: true,
                origin: None,
            });
            self.function_operators[t].insert(i);
            self.function_keys.insert((t, key), i);
            self.changed = true;
            self.stats.function_updates += 1;
            self.events.push(Dependency::Function(i));
            let row = self.function(i).unwrap();
            self.subscribe(row.witness, &row.values);
        }
        Ok(())
    }
    fn retire(&mut self, f: FactRef) -> Result<()> {
        if let Some(r) = self.fact(f)? {
            self.query_versions[r.table] += 1;
            self.query_changes.insert(Dependency::Fact(f), r.table);
            let s = self
                .cells
                .get_mut(&f.cell)
                .unwrap()
                .facts
                .remove(&f.op)
                .unwrap();
            self.operators[r.table].remove(&f);
            if let Some(b) = self.intern.get_mut(&s.hash) {
                b.remove(&f);
            }
            self.subscribe(Dependency::Fact(f), &[]);
            self.garbage.insert(f.cell);
        }
        Ok(())
    }
    pub fn hide(&mut self, t: usize, key: &[Value], delete: bool) -> Result<bool> {
        let row = self.lookup(t, key)?;
        let Some(row) = row else {
            return if delete {
                Ok(false)
            } else {
                let v = self.dynamic_construct(t, key.to_vec())?;
                let _ = v;
                self.hide(t, key, false)?;
                Ok(false)
            };
        };
        match row.witness {
            Dependency::Fact(f) => {
                let born = self.cells[&f.cell].facts[&f.op].born;
                if delete {
                    self.retire(f)?;
                    self.changed = true;
                } else {
                    let s = self
                        .cells
                        .get_mut(&f.cell)
                        .unwrap()
                        .facts
                        .get_mut(&f.op)
                        .unwrap();
                    if !s.hidden {
                        self.query_versions[t] += 1;
                        self.query_changes.insert(row.witness, t);
                        s.hidden = true;
                        s.epoch = self.epoch;
                        self.changed = true;
                        self.events.push(row.witness);
                    }
                }
                Ok(born < self.epoch)
            }
            Dependency::Function(i) => {
                if !delete {
                    return Err("function subsume unsupported".into());
                }
                self.functions[i].live = false;
                self.function_origin(i, None);
                self.query_versions[t] += 1;
                self.query_changes.insert(row.witness, t);
                self.function_operators[t].remove(&i);
                self.function_keys.remove(&(t, key.to_vec()));
                self.subscribe(row.witness, &[]);
                self.changed = true;
                Ok(true)
            }
        }
    }
    pub fn repair(&mut self) -> Result<()> {
        loop {
            if !self.pending_unions.is_empty() {
                let pairs = std::mem::take(&mut self.pending_unions);
                for (a, b) in pairs {
                    self.union(a, b);
                }
                continue;
            }
            if let Some(cell) = self.congruence_dirty.pop_first() {
                self.repair_local_congruence(cell)?;
                continue;
            }
            let Some(d) = self
                .dirty
                .pop_first()
                .or_else(|| self.dirty_markers.pop_first().map(Dependency::Fact))
            else {
                break;
            };
            if let Dependency::Fact(f) = d {
                if let Some(table) = self.dependency_table(d) {
                    self.dirty_facts[table].remove(&f);
                }
            }
            self.stats.repaired_dependencies += 1;
            match d {
                Dependency::Fact(f) => {
                    let Some(r) = self.fact(f)? else { continue };
                    let key = &r.values[..r.values.len() - 1];
                    let h = hashkey(r.table, key);
                    let old = self.cells[&f.cell].facts[&f.op].hash;
                    self.stats.repair_key_hash_changes += u64::from(h != old);
                    if let Some(bucket) = self.intern.get_mut(&old) {
                        bucket.remove(&f);
                    }
                    let candidates = self.intern.get(&h).cloned().unwrap_or_default();
                    let mut found = None;
                    for other in candidates {
                        if let Some(row) = self.fact(other)? {
                            if row.table == r.table && row.values[..key.len()] == *key {
                                found = Some(row);
                                break;
                            }
                        }
                    }
                    if let Some(other) = found {
                        self.stats.repair_collisions += 1;
                        if self.program.tables[r.table].constructor {
                            self.enqueue_union(
                                r.values.last().unwrap().clone(),
                                other.values.last().unwrap().clone(),
                            )?;
                        }
                        if let Dependency::Fact(keep) = other.witness {
                            self.query_versions[r.table] += 1;
                            self.query_changes.insert(other.witness, r.table);
                            let target = self
                                .cells
                                .get_mut(&keep.cell)
                                .unwrap()
                                .facts
                                .get_mut(&keep.op)
                                .unwrap();
                            target.hidden |= r.hidden;
                            target.epoch = self.epoch;
                            self.events.push(other.witness);
                        }
                        self.retire(f)?;
                    } else {
                        self.intern.entry(h).or_default().insert(f);
                        let s = self
                            .cells
                            .get_mut(&f.cell)
                            .unwrap()
                            .facts
                            .get_mut(&f.op)
                            .unwrap();
                        s.hash = h;
                        s.epoch = self.epoch;
                        if let Some(marker) = &mut s.marker_row {
                            marker.values.clone_from(&r.values);
                            marker.canonical = true;
                        }
                        self.subscribe(d, &r.values);
                        self.events.push(d);
                    }
                }
                Dependency::Function(i) => {
                    let Some(row) = self.function(i) else {
                        continue;
                    };
                    let key = row.values[..row.values.len() - 1].to_vec();
                    let oldkey = (row.table, self.functions[i].key.clone());
                    if self.function_keys.get(&oldkey) == Some(&i) {
                        self.function_keys.remove(&oldkey);
                    }
                    if let Some(j) = self.function_keys.get(&(row.table, key.clone())).copied() {
                        if j != i {
                            let old = canon(&self.functions[j].value, &self.parents);
                            let next = self.merge_value(
                                row.table,
                                old,
                                row.values.last().unwrap().clone(),
                            )?;
                            self.functions[j].value = next;
                            self.functions[j].epoch = self.epoch;
                            self.functions[i].live = false;
                            self.function_origin(i, None);
                            self.function_operators[row.table].remove(&i);
                            self.subscribe(d, &[]);
                            let target = self.function(j).unwrap();
                            self.subscribe(target.witness, &target.values);
                            self.events.push(target.witness);
                            continue;
                        }
                    }
                    self.functions[i].key = key.clone();
                    self.functions[i].value = row.values.last().unwrap().clone();
                    self.functions[i].epoch = self.epoch;
                    self.function_keys.insert((row.table, key), i);
                    self.subscribe(d, &row.values);
                    self.events.push(d);
                }
            }
        }
        Ok(())
    }
    fn repair_local_congruence(&mut self, cell: u32) -> Result<()> {
        let Some(c) = self.cells.get(&cell) else {
            return Ok(());
        };
        let Some(groups) = self.program.congruence.templates.get(&c.template).cloned() else {
            return Ok(());
        };
        self.congruence_stats.instance_visits += 1;
        let mut pairs = vec![];
        let mut duplicates = BTreeMap::new();
        for group in groups.iter() {
            let mut equal = true;
            for (a, b) in &group.guards {
                self.congruence_stats.guard_tests += 1;
                // A future/unready port never authorizes a shortcut.
                match (self.argument(cell, a), self.argument(cell, b)) {
                    (Ok(a), Ok(b)) if a == b => (),
                    _ => {
                        equal = false;
                        break;
                    }
                }
            }
            if !equal {
                continue;
            }
            // All depths of a repeated spine often have the same leaf guard.
            // Check their common carrier once, rather than rescanning every
            // prefix for every conclusion (quadratic on two long chains).
            let all_ready = group
                .requires
                .iter()
                .all(|op| self.cells[&cell].facts.get(op).is_some_and(|f| !f.hidden));
            for proof in &group.merges {
                let c = &self.cells[&cell];
                if !all_ready
                    && !proof
                        .requires
                        .iter()
                        .all(|op| c.facts.get(op).is_some_and(|f| !f.hidden))
                {
                    self.congruence_stats.unavailable_proofs += 1;
                    continue;
                }
                let t = &self.program.templates[c.template];
                let (Op::Let(a, _, _), Op::Let(b, _, _)) =
                    (&t.ops[proof.left], &t.ops[proof.right])
                else {
                    unreachable!()
                };
                let (Value::Eq(a), Value::Eq(b)) = (self.slot(cell, *a)?, self.slot(cell, *b)?)
                else {
                    unreachable!()
                };
                if a != b {
                    pairs.push((a, b));
                }
                // This proves identical constructor keys, not merely equal
                // result classes. Only the former permits retiring a fact.
                let (a, b) = (
                    root(&duplicates, proof.left as u64),
                    root(&duplicates, proof.right as u64),
                );
                if a != b {
                    duplicates.insert(a.max(b), a.min(b));
                }
            }
        }
        if !pairs.is_empty() {
            self.union_local_batch(&pairs);
        }
        // Known local duplicates do not need another global signature probe.
        // The surviving occurrence remains dirty and must still meet any
        // duplicates in other instances through the exact global repairer.
        for &op in duplicates.keys() {
            let keep = FactRef {
                cell,
                op: root(&duplicates, op) as u32,
            };
            let old = FactRef {
                cell,
                op: op as u32,
            };
            let table = self
                .dependency_table(Dependency::Fact(old))
                .ok_or("missing proved constructor")?;
            let target = self
                .cells
                .get_mut(&cell)
                .unwrap()
                .facts
                .get_mut(&keep.op)
                .ok_or("missing local survivor")?;
            target.epoch = self.epoch;
            self.events.push(Dependency::Fact(keep));
            self.query_versions[table] += 1;
            self.query_changes.insert(Dependency::Fact(keep), table);
            self.retire(old)?;
            self.dirty.remove(&Dependency::Fact(old));
            self.dirty_markers.remove(&old);
            self.dirty_facts[table].remove(&old);
            self.congruence_stats.coalesced_facts += 1;
        }
        Ok(())
    }
    pub fn congruence_report(&self) -> J {
        json!({"enabled":self.congruence_enabled,"reorder_safe":self.program.congruence.reorder_safe,
            "templates":self.program.congruence.templates.len(),"condition_groups":self.program.congruence.templates.values().map(|gs|gs.len()).sum::<usize>(),
            "proofs":self.program.congruence.templates.values().flat_map(|gs|gs.iter()).map(|g|g.merges.len()).sum::<usize>(),
            "stats":self.congruence_stats,"scope":"conditional local constructor closure; residual global repair retained"})
    }
    pub fn release_empty(&mut self, cell: u32) {
        // Only the newly executed frame is retired here. Ancestors are kept
        // until the Run boundary: later bindings in this Run may still use them.
        self.retire_empty(cell);
    }
    fn retire_empty(&mut self, cell: u32) {
        if self.parent_users.get(&cell).copied().unwrap_or(0) == 0
            && self.cells.get(&cell).is_some_and(|c| c.facts.is_empty())
        {
            if let Some(c) = self.cells.remove(&cell) {
                if let Some(p) = c.parent {
                    self.release_reference(p);
                }
                for f in c.providers.values() {
                    self.release_reference(*f);
                }
            }
            self.parent_users.remove(&cell);
            self.stats.empty_frames_released += 1;
        }
    }
    pub fn collect_garbage(&mut self) {
        while let Some(cell) = self.garbage.pop_first() {
            self.retire_empty(cell);
        }
    }
    pub fn export(&self) -> Result<J> {
        let mut rows = vec![];
        for t in 0..self.program.tables.len() {
            for w in self.witnesses(t) {
                if let Some(r) = self.row(w)? {
                    rows.push(json!({"table":self.program.tables[t].name,"values":r.values.iter().zip(&self.program.tables[t].schema).map(|(v,s)|exported(v,s)).collect::<Vec<_>>(),"subsumed":r.hidden}));
                }
            }
        }
        let anchors=self.anchors.iter().map(|(sort,old,id)|json!({"sort":sort,"old":old,"now":format!("Cell({})",root(&self.parents,*id))})).collect::<Vec<_>>();
        Ok(json!({"rows":rows,"anchors":anchors}))
    }
    /// Stream diagnostics without allocating a second complete row graph.
    pub fn write_export(&self, writer: impl std::io::Write) -> Result<()> {
        use serde::ser::{SerializeSeq, SerializeStruct};
        struct Rows<'a>(&'a Graph);
        impl Serialize for Rows<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(None)?;
                for table in 0..self.0.program.tables.len() {
                    for witness in self.0.witnesses(table) {
                        if let Some(row) = self.0.row(witness).map_err(serde::ser::Error::custom)? {
                            let table = &self.0.program.tables[row.table];
                            seq.serialize_element(&json!({"table":table.name,"values":row.values.iter().zip(&table.schema).map(|(v,s)|exported(v,s)).collect::<Vec<_>>(),"subsumed":row.hidden}))?;
                        }
                    }
                }
                seq.end()
            }
        }
        struct Export<'a>(&'a Graph);
        impl Serialize for Export<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut result = serializer.serialize_struct("Graph", 2)?;
                result.serialize_field("rows", &Rows(self.0))?;
                let anchors=self.0.anchors.iter().map(|(sort,old,id)|json!({"sort":sort,"old":old,"now":format!("Cell({})",root(&self.0.parents,*id))})).collect::<Vec<_>>();
                result.serialize_field("anchors", &anchors)?;
                result.end()
            }
        }
        serde_json::to_writer(writer, &Export(self)).map_err(|e| e.to_string())
    }
}

#[path = "reference_storage.rs"]
mod reference_storage;
