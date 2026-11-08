use brush_parser::ast::{ArithmeticExpr, ArithmeticTarget};

use crate::types::Variables;

pub(crate) fn collect_arithmetic_names(text: &str, vars: &mut Variables) {
    match brush_parser::arithmetic::parse(text.trim_matches([' ', '\t', '\n', '\r'])) {
        Ok(expr) => collect_expr_names(&expr, vars),
        Err(_) => vars.dynamic = true,
    }
}

fn collect_expr_names(expr: &ArithmeticExpr, vars: &mut Variables) {
    use ArithmeticExpr as E;
    match expr {
        E::Literal(_) => {}
        E::Reference(target) | E::UnaryAssignment(_, target) => collect_target_names(target, vars),
        E::UnaryOp(_, operand) => collect_expr_names(operand, vars),
        E::BinaryOp(_, left, right) => {
            collect_expr_names(left, vars);
            collect_expr_names(right, vars);
        }
        E::Conditional(condition, then, otherwise) => {
            collect_expr_names(condition, vars);
            collect_expr_names(then, vars);
            collect_expr_names(otherwise, vars);
        }
        E::Assignment(target, value) | E::BinaryAssignment(_, target, value) => {
            collect_target_names(target, vars);
            collect_expr_names(value, vars);
        }
    }
}

fn collect_target_names(target: &ArithmeticTarget, vars: &mut Variables) {
    match target {
        ArithmeticTarget::Variable(name) => vars.push(name),
        ArithmeticTarget::ArrayElement(name, index) => {
            vars.push(name);
            collect_expr_names(index, vars);
        }
    }
}
