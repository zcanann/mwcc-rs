//! Find pair values whose high word cannot reach an observable use.
//!
//! Fixed local homes can have multiple definitions and loop backedges. A
//! monotone demand set therefore covers every definition, including both arms
//! of a structured branch, before any types are narrowed. Calls keep their pair
//! ABI. Optimized references omit an unused high load even through a volatile
//! pointer; the retained low access uses its original big-endian displacement.
use super::*;
use std::collections::HashSet;

fn low_word_arithmetic(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::BitAnd
            | BinaryOperator::BitOr
            | BinaryOperator::BitXor
    )
}

fn needed(value: Value, high: &HashSet<usize>) -> bool {
    wide(value.ty)
        && match value.source {
            Source::Register(id) => high.contains(&id),
            Source::Constant(_) => true,
        }
}

fn demand(value: Value, high: &mut HashSet<usize>) {
    if wide(value.ty) {
        if let Source::Register(id) = value.source {
            high.insert(id);
        }
    }
}

fn condition(condition: &Condition, high: &mut HashSet<usize>) {
    match *condition {
        Condition::Value(value) => demand(value, high),
        Condition::Compare { left, right, .. } => {
            demand(left, high);
            demand(right, high);
        }
    }
}

fn propagate(operations: &[Operation], high: &mut HashSet<usize>) {
    for op in operations {
        match op {
            Operation::BranchIf {
                condition: test, ..
            } => condition(test, high),
            Operation::Branch {
                condition: test,
                then_body,
                else_body,
            } => {
                condition(test, high);
                propagate(then_body, high);
                propagate(else_body, high);
            }
            Operation::Return(Some(value)) | Operation::Store { value, .. } => demand(*value, high),
            Operation::Call { arguments, .. } => {
                for (_, value) in arguments {
                    demand(*value, high);
                }
            }
            Operation::Copy { result, value } | Operation::Convert { result, value } => {
                if needed(*result, high) {
                    demand(*value, high);
                }
            }
            Operation::Binary {
                result,
                operator,
                left,
                right,
            } => {
                if !low_word_arithmetic(*operator) || needed(*result, high) {
                    demand(*left, high);
                    demand(*right, high);
                }
            }
            _ => {}
        }
    }
}

fn low(value: &mut Value) {
    if wide(value.ty) {
        value.ty = Type::UnsignedInt;
    }
}

fn narrow(operations: &mut [Operation], high: &HashSet<usize>) {
    for op in operations {
        match op {
            Operation::Branch {
                then_body,
                else_body,
                ..
            } => {
                narrow(then_body, high);
                narrow(else_body, high);
            }
            Operation::Parameter {
                result,
                high: argument,
            } if wide(result.ty) && !needed(*result, high) => {
                // The incoming pair still occupies two ABI words. Only copy
                // its low word into the narrowed home.
                *argument += 1;
                low(result);
            }
            Operation::Load { result, offset, .. } if wide(result.ty) && !needed(*result, high) => {
                *offset += 4;
                low(result);
            }
            Operation::Local { result } => {
                if !needed(*result, high) {
                    low(result);
                }
            }
            Operation::Copy { result, value } | Operation::Convert { result, value } => {
                if !needed(*result, high) {
                    low(result);
                    low(value);
                }
            }
            Operation::Binary {
                result,
                operator,
                left,
                right,
            } if wide(result.ty) && low_word_arithmetic(*operator) && !needed(*result, high) => {
                low(result);
                low(left);
                low(right);
            }
            _ => {}
        }
    }
}

impl Graph<'_> {
    pub(super) fn narrow_unobserved_high_words(&mut self) {
        // O0 references retain the unused high multiply/add instructions.
        if self.optimization == mwcc_versions::Optimization::O0
            || (self.wide_word_demand_starts_at_o2
                && self.optimization == mwcc_versions::Optimization::O1)
        {
            return;
        }
        let mut high = HashSet::new();
        if let Some(value) = self.returned {
            demand(value, &mut high);
        }
        loop {
            let before = high.len();
            propagate(&self.operations, &mut high);
            if high.len() == before {
                break;
            }
        }
        narrow(&mut self.operations, &high);
    }
}
