use brush_parser::ast;

use crate::types::Sub;
use crate::word::collect_word_subs;

/// `[[ ... ]]` words can hide substitutions (`[[ -n $(cmd) ]]`); surface them.
pub(crate) fn collect_extended_test_subs(expr: &ast::ExtendedTestExpr, subs: &mut Vec<Sub>) {
    use ast::ExtendedTestExpr as E;
    match expr {
        E::And(l, r) | E::Or(l, r) => {
            collect_extended_test_subs(l, subs);
            collect_extended_test_subs(r, subs);
        }
        E::Not(e) | E::Parenthesized(e) => collect_extended_test_subs(e, subs),
        // The test itself is never an emitted stage, so its substitutions have no parent.
        E::UnaryTest(_, w) => collect_word_subs(&w.value, None, subs),
        E::BinaryTest(_, l, r) => {
            collect_word_subs(&l.value, None, subs);
            collect_word_subs(&r.value, None, subs);
        }
    }
}
