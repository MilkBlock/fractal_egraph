use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Value {
    Eq(u64),
    Int(i64),
    Text(String),
    Float(i64),
    Set(Vec<Value>),
}
pub type Parents = BTreeMap<u64, u64>;
pub fn root(parents: &Parents, mut x: u64) -> u64 {
    while let Some(&y) = parents.get(&x) {
        if x == y {
            break;
        }
        x = y;
    }
    x
}
pub fn canon(v: &Value, p: &Parents) -> Value {
    match v {
        Value::Eq(x) => Value::Eq(root(p, *x)),
        Value::Set(xs) => {
            let mut xs = xs.iter().map(|x| canon(x, p)).collect::<Vec<_>>();
            xs.sort();
            xs.dedup();
            Value::Set(xs)
        }
        _ => v.clone(),
    }
}
pub fn integer(v: &Value) -> Result<i64, String> {
    if let Value::Int(x) = v {
        Ok(*x)
    } else {
        Err("expected integer".into())
    }
}
pub fn primitive(name: &str, args: &[Value], output: &str) -> Result<Option<Value>, String> {
    let name = name.strip_prefix("bool-").unwrap_or(name);
    let value = match name {
        "=" | "!=" => {
            let yes = (args[0] == args[1]) == (name == "=");
            if output == "Unit" {
                return Ok(yes.then_some(Value::Int(0)));
            }
            Value::Int(i64::from(yes))
        }
        "<" | "<=" | ">" | ">=" => {
            let a = integer(&args[0])?;
            let b = integer(&args[1])?;
            let yes = match name {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a >= b,
            };
            if output == "Unit" {
                return Ok(yes.then_some(Value::Int(0)));
            }
            Value::Int(i64::from(yes))
        }
        "+" | "-" | "*" | "/" | "%" | "min" | "max" => {
            let a = integer(&args[0])?;
            let b = integer(&args[1])?;
            let n = match name {
                "+" => a.checked_add(b),
                "-" => a.checked_sub(b),
                "*" => a.checked_mul(b),
                "/" => a.checked_div(b),
                "%" => a.checked_rem(b),
                "min" => Some(a.min(b)),
                _ => Some(a.max(b)),
            };
            let Some(n) = n else { return Ok(None) };
            Value::Int(n)
        }
        "not" => Value::Int(i64::from(integer(&args[0])? == 0)),
        "and" | "or" | "xor" | "implies" => {
            let a = integer(&args[0])? != 0;
            let b = integer(&args[1])? != 0;
            Value::Int(i64::from(match name {
                "and" => a && b,
                "or" => a || b,
                "xor" => a != b,
                _ => !a || b,
            }))
        }
        "set-of" | "set-empty" => {
            let mut xs = args.to_vec();
            xs.sort();
            xs.dedup();
            Value::Set(xs)
        }
        "set-insert" | "set-remove" => {
            let Value::Set(xs) = &args[0] else {
                return Err("set expected".into());
            };
            let mut xs = xs.clone();
            if name == "set-insert" {
                xs.push(args[1].clone());
                xs.sort();
                xs.dedup()
            } else {
                xs.retain(|x| x != &args[1])
            }
            Value::Set(xs)
        }
        "set-union" | "set-intersect" | "set-diff" => {
            let (Value::Set(a), Value::Set(b)) = (&args[0], &args[1]) else {
                return Err("set expected".into());
            };
            let mut xs = if name == "set-union" {
                [a.clone(), b.clone()].concat()
            } else {
                a.iter()
                    .filter(|x| b.contains(x) == (name == "set-intersect"))
                    .cloned()
                    .collect()
            };
            xs.sort();
            xs.dedup();
            Value::Set(xs)
        }
        "set-contains" | "set-not-contains" => {
            let Value::Set(xs) = &args[0] else {
                return Err("set expected".into());
            };
            return Ok((xs.contains(&args[1]) == (name == "set-contains")).then_some(Value::Int(0)));
        }
        "set-length" => {
            let Value::Set(xs) = &args[0] else {
                return Err("set expected".into());
            };
            Value::Int(xs.len() as i64)
        }
        _ => return Err(format!("unsupported primitive {name}")),
    };
    Ok(Some(value))
}
pub fn literal(j: &J) -> Result<Value, String> {
    let kind = j["sort"]["kind"].as_str().ok_or("literal sort")?;
    let n = &j["literal"];
    Ok(match kind {
        "String" => Value::Text(n.as_str().ok_or("string literal")?.into()),
        "f64" => Value::Float(n.as_i64().ok_or("float bits")?),
        "i64" | "bool" | "Unit" => Value::Int(n.as_i64().ok_or("integer literal")?),
        _ => return Err(format!("unsupported literal {kind}")),
    })
}
pub fn exported(v: &Value, sort: &J) -> J {
    match v {
        Value::Eq(x) => json!({"sort":sort["name"],"eq":format!("Cell({x})")}),
        Value::Int(x) | Value::Float(x) => json!({"sort":sort["name"],"value":x}),
        Value::Text(x) => json!({"sort":sort["name"],"value":x}),
        Value::Set(xs) => {
            json!({"sort":sort["name"],"elements":xs.iter().map(|v|exported(v,&sort["element"])).collect::<Vec<_>>()})
        }
    }
}
