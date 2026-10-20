//! Exact pure LHS bytecode. Extra registers never become native Value IDs.
use super::*;
#[derive(Clone, Debug)]
pub struct CellPrimitive {
    /// 0..3 signed comparisons; 4 token !=; 5 numeric !=; 6 checked +; 7 checked %.
    pub opcode: u32,
    pub args: [CellOperand; 2],
    pub result: Option<CellOperand>,
}
pub(super) fn compile(
    eg: &EGraph,
    body: &crate::core::Query<ResolvedCall, ResolvedVar>,
    vars: &[ResolvedVar],
) -> Result<(Vec<CellPrimitive>, usize), Error> {
    let fail = |s: &str| Error::BackendError(s.into());
    let mut registers = vars.to_vec();
    for a in &body.atoms {
        if let ResolvedCall::Primitive(p) = &a.head {
            if p.output().name() == "i64" {
                if let Some(ResolvedAtomTerm::Var(_, v)) = a.args.last() {
                    if !registers.contains(v) {
                        registers.push(v.clone());
                    }
                }
            }
        }
    }
    let operand = |a: &ResolvedAtomTerm| -> Result<CellOperand, Error> {
        match a {
            ResolvedAtomTerm::Var(_, v) => registers
                .iter()
                .position(|x| x == v)
                .map(CellOperand::Slot)
                .ok_or_else(|| fail("ungrounded primitive operand")),
            ResolvedAtomTerm::Literal(_, l) => Ok(CellOperand::Constant {
                value: literal_to_value(&eg.backend, l),
                text: l.to_string(),
            }),
            _ => Err(fail("unsupported primitive global")),
        }
    };
    let mut ops = vec![];
    for a in &body.atoms {
        let ResolvedCall::Primitive(p) = &a.head else {
            continue;
        };
        if a.args.len() != 3 || p.input().len() != 2 {
            return Err(fail("GPU primitive requires binary inputs"));
        }
        let numeric = p.input().iter().all(|s| s.name() == "i64");
        let opcode = match (p.name(), numeric, p.output().name()) {
            ("<", true, "Unit") => 0,
            (">", true, "Unit") => 1,
            ("<=", true, "Unit") => 2,
            (">=", true, "Unit") => 3,
            ("!=", true, "Unit") => 5,
            ("!=", false, "Unit") if p.input()[0].name() == p.input()[1].name() => 4,
            ("+", true, "i64") => 6,
            ("%", true, "i64") => 7,
            _ => {
                return Err(fail(&format!(
                    "unsupported GPU primitive {} {:?}",
                    p.name(),
                    p.input().iter().map(|s| s.name()).collect::<Vec<_>>()
                )));
            }
        };
        ops.push(CellPrimitive {
            opcode,
            args: [operand(&a.args[0])?, operand(&a.args[1])?],
            result: if opcode >= 6 {
                Some(operand(&a.args[2])?)
            } else {
                None
            },
        });
    }
    // This is a dependency proof, not a heuristic iteration budget.
    let mut known = (0..vars.len()).collect::<std::collections::BTreeSet<_>>();
    let mut pending = (0..ops.len()).collect::<std::collections::BTreeSet<_>>();
    while !pending.is_empty() {
        let Some(i) = pending.iter().copied().find(|i| {
            ops[*i].args.iter().all(|o| match o {
                CellOperand::Slot(i) => known.contains(i),
                _ => true,
            })
        }) else {
            return Err(fail(
                "primitive inputs cannot be grounded by table bindings",
            ));
        };
        if let Some(CellOperand::Slot(i)) = &ops[i].result {
            known.insert(*i);
        }
        pending.remove(&i);
    }
    Ok((ops, registers.len() - vars.len()))
}
pub(super) fn hold(eg: &EGraph, sink: &CellActionSink, binding: &[Value]) -> Result<bool, Error> {
    if sink.primitives.is_empty() {
        return Ok(true);
    }
    if sink.primitives.iter().all(|p| p.result.is_none()) {
        let value = |o: &CellOperand| match o {
            CellOperand::Slot(i) => binding[*i],
            CellOperand::Constant { value, .. } => *value,
        };
        for p in &sink.primitives {
            let a = value(&p.args[0]);
            let b = value(&p.args[1]);
            if p.opcode == 4 {
                if a == b {
                    return Ok(false);
                };
                continue;
            }
            let a = eg.value_to_base::<i64>(a);
            let b = eg.value_to_base::<i64>(b);
            if !match p.opcode {
                0 => a < b,
                1 => a > b,
                2 => a <= b,
                3 => a >= b,
                5 => a != b,
                _ => unreachable!(),
            } {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    let mut values = vec![None; sink.device_slots()];
    for (i, (v, s)) in binding.iter().zip(&sink.vars).enumerate() {
        if s.sort.name() == "i64" {
            values[i] = Some(eg.value_to_base::<i64>(*v));
        }
    }
    let mut pending = (0..sink.primitives.len()).collect::<std::collections::BTreeSet<_>>();
    while !pending.is_empty() {
        let mut progress = false;
        for i in pending.iter().copied().collect::<Vec<_>>() {
            let p = &sink.primitives[i];
            if p.opcode == 4 {
                let value = |o: &CellOperand| match o {
                    CellOperand::Slot(i) => binding[*i],
                    CellOperand::Constant { value, .. } => *value,
                };
                if value(&p.args[0]) == value(&p.args[1]) {
                    return Ok(false);
                }
            } else {
                let value = |o: &CellOperand| match o {
                    CellOperand::Slot(i) => values[*i],
                    CellOperand::Constant { value, .. } => Some(eg.value_to_base::<i64>(*value)),
                };
                let (Some(a), Some(b)) = (value(&p.args[0]), value(&p.args[1])) else {
                    continue;
                };
                if p.opcode < 6 {
                    if !match p.opcode {
                        0 => a < b,
                        1 => a > b,
                        2 => a <= b,
                        3 => a >= b,
                        5 => a != b,
                        _ => unreachable!(),
                    } {
                        return Ok(false);
                    }
                } else {
                    let result = if p.opcode == 6 {
                        a.checked_add(b)
                    } else {
                        a.checked_rem(b)
                    };
                    let Some(result) = result else {
                        return Ok(false);
                    };
                    match p.result.as_ref().unwrap() {
                        CellOperand::Slot(j) => {
                            if values[*j].is_some_and(|x| x != result) {
                                return Ok(false);
                            }
                            values[*j] = Some(result);
                        }
                        CellOperand::Constant { value, .. } => {
                            if result != eg.value_to_base::<i64>(*value) {
                                return Ok(false);
                            }
                        }
                    }
                }
            }
            pending.remove(&i);
            progress = true;
        }
        if !progress {
            return Err(Error::BackendError("incomplete primitive binding".into()));
        }
    }
    Ok(true)
}
