//! Emit a slotted-egraph DOT file with e-class slot sets and AppliedId edges.
use egg_layout::slotted_dot::to_dot;
use slotted_egraphs::{
    AppliedId, EGraph, Language, LanguageChildren, RecExpr, Slot, SyntaxElem, define_language,
};

define_language! {
    enum Demo {
        Add(AppliedId, AppliedId) = "+",
        Var(Slot) = "var",
        Number(i64),
    }
}
fn main() {
    let mut eg = EGraph::<Demo>::default();
    let a = eg.add_syn_expr(RecExpr::parse("(var $0)").unwrap());
    let b = eg.add_syn_expr(RecExpr::parse("(var $1)").unwrap());
    let sum = eg.add(Demo::Add(a.clone(), b.clone()));
    let swapped = eg.add(Demo::Add(b, a));
    eg.union(&sum, &swapped);
    let c = eg.add_syn_expr(RecExpr::parse("(var $2)").unwrap());
    let d = eg.add_syn_expr(RecExpr::parse("(var $3)").unwrap());
    let right = eg.add(Demo::Add(c, d));
    eg.union(&sum, &right);
    print!("{}", to_dot(&eg));
}
