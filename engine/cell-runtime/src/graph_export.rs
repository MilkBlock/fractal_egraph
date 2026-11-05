//! A table's JSON field names, ordering and sorts repeat in every row.
//! Encode them once; use serde_json for escaping and uncommon wire values.
use super::*;
use std::io::{self, Write};

struct ObjectSlot {
    before: Vec<u8>,
    after: Vec<u8>,
}

impl ObjectSlot {
    fn new(object: J, slot: &str) -> io::Result<Self> {
        let mut before = vec![b'{'];
        let mut after = Vec::new();
        let mut part = &mut before;
        // serde_json changes map ordering when a caller enables preserve_order.
        // Use the same object constructor and iteration order as Graph::export.
        for (i, (name, value)) in object.as_object().unwrap().iter().enumerate() {
            if i != 0 {
                part.push(b',');
            }
            serde_json::to_writer(&mut *part, name)?;
            part.push(b':');
            if name == slot {
                part = &mut after;
            } else {
                serde_json::to_writer(&mut *part, value)?;
            }
        }
        after.push(b'}');
        Ok(Self { before, after })
    }
}

struct Column<'a> {
    sort: &'a J,
    eq: ObjectSlot,
    scalar: ObjectSlot,
}

impl<'a> Column<'a> {
    fn new(sort: &'a J) -> io::Result<Self> {
        let name = &sort["name"];
        Ok(Self {
            sort,
            eq: ObjectSlot::new(json!({"sort":name,"eq":null}), "eq")?,
            scalar: ObjectSlot::new(json!({"sort":name,"value":null}), "value")?,
        })
    }

    fn write(&self, w: &mut impl Write, v: &Value) -> io::Result<()> {
        let slot = match v {
            Value::Eq(_) => &self.eq,
            Value::Int(_) | Value::Float(_) | Value::Text(_) => &self.scalar,
            Value::Set(_) | Value::Key(_) => {
                return Ok(serde_json::to_writer(w, &exported(v, self.sort))?);
            }
        };
        w.write_all(&slot.before)?;
        match v {
            Value::Eq(id) => write!(w, "\"Cell({id})\"")?,
            Value::Int(x) | Value::Float(x) => serde_json::to_writer(&mut *w, x)?,
            Value::Text(s) => serde_json::to_writer(&mut *w, s)?,
            _ => unreachable!(),
        }
        w.write_all(&slot.after)
    }
}

impl Graph {
    /// Stream current logical rows in the same witness and field order as the
    /// original wire format, including subsumed rows and canonical anchors.
    pub fn write_export(&self, mut w: impl Write) -> Result<()> {
        fn stream(g: &Graph, w: &mut impl Write) -> io::Result<()> {
            w.write_all(b"{\"rows\":[")?;
            let mut first = true;
            for (t, table) in g.program.tables.iter().enumerate() {
                if g.witness_count(t) == 0 {
                    continue;
                }
                let layout = |hidden| {
                    ObjectSlot::new(
                        json!({"table":table.name,"values":null,"subsumed":hidden}),
                        "values",
                    )
                };
                let rows = [layout(false)?, layout(true)?];
                let cols = table
                    .schema
                    .iter()
                    .map(Column::new)
                    .collect::<io::Result<Vec<_>>>()?;
                let mut visit = |witness| -> io::Result<()> {
                    let Some(row) = g.row(witness).map_err(io::Error::other)? else {
                        return Ok(());
                    };
                    if !first {
                        w.write_all(b",")?;
                    }
                    first = false;
                    let object = &rows[usize::from(row.hidden)];
                    w.write_all(&object.before)?;
                    w.write_all(b"[")?;
                    for (i, (col, value)) in cols.iter().zip(row.values.iter()).enumerate() {
                        if i != 0 {
                            w.write_all(b",")?;
                        }
                        col.write(w, value)?;
                    }
                    w.write_all(b"]")?;
                    w.write_all(&object.after)
                };
                if table.virtual_fact {
                    for &f in &g.operators[t] {
                        visit(Dependency::Fact(f))?;
                    }
                } else {
                    for &i in &g.function_operators[t] {
                        visit(Dependency::Function(i))?;
                    }
                }
            }
            w.write_all(b"],\"anchors\":[")?;
            for (i, (sort, old, id)) in g.anchors.iter().enumerate() {
                if i != 0 {
                    w.write_all(b",")?;
                }
                serde_json::to_writer(
                    &mut *w,
                    &json!({"sort":sort,"old":old,"now":format!("Cell({})",root(&g.parents,*id))}),
                )?;
            }
            w.write_all(b"]}")
        }
        stream(self, &mut w).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_columns_match_the_object_encoder() {
        let sort = json!({"name":"quoted \" sort 🌿","element":{"name":"i64"}});
        let col = Column::new(&sort).unwrap();
        for value in [
            Value::Eq(u64::MAX),
            Value::Int(i64::MIN),
            Value::Float(-1),
            Value::Text("line\n\"slash\\\t🌱".to_owned().into()),
            Value::Set(vec![Value::Int(-3), Value::Int(9)].into()),
            Value::Key(eggfuse_structural_key::Key::base_i64(42)),
        ] {
            let mut bytes = Vec::new();
            col.write(&mut bytes, &value).unwrap();
            assert_eq!(bytes, serde_json::to_vec(&exported(&value, &sort)).unwrap());
        }
    }
}
