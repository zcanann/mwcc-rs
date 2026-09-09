//! Conditions consume comparisons directly. Boolean values still materialize
//! when stored or returned; fixed local homes allow short-circuit branch edges
//! without a separate boolean merge or speculative right-operand evaluation.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Condition {
    Value(Value),
    Compare {
        operator: BinaryOperator,
        left: Value,
        right: Value,
    },
}

impl Graph<'_> {
    pub(super) fn predicate(&mut self, value: Value) -> Condition {
        if let Some(Operation::Binary {
            result,
            operator,
            left,
            right,
            ..
        }) = self.operations.last()
        {
            if *result == value
                && is_comparison(*operator)
                && !wide(left.ty)
                && !self.bindings.values().any(|binding| *binding == value)
            {
                let condition = Condition::Compare {
                    operator: *operator,
                    left: *left,
                    right: *right,
                };
                self.operations.pop();
                return condition;
            }
        }
        Condition::Value(self.truth(value))
    }

    pub(super) fn condition(&mut self, expression: &Expression) -> Option<Condition> {
        let value = self.expression(expression)?;
        Some(self.predicate(value))
    }

    pub(super) fn conditional_jump(
        &mut self,
        expression: &Expression,
        target: usize,
        on_true: bool,
    ) -> Option<()> {
        // Only fixed homes can merge assignments along these flat edges. The
        // SSA path retains its explicit copies when a logical value is needed.
        if self.fixed_bindings {
            match expression {
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand,
                } => return self.conditional_jump(operand, target, !on_true),
                Expression::Binary {
                    operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
                    left,
                    right,
                } => {
                    let is_or = matches!(
                        expression,
                        Expression::Binary {
                            operator: BinaryOperator::LogicalOr,
                            ..
                        }
                    );
                    if is_or == on_true {
                        self.conditional_jump(left, target, on_true)?;
                        self.conditional_jump(right, target, on_true)?;
                    } else {
                        let skip = self.label();
                        self.conditional_jump(left, skip, !on_true)?;
                        self.conditional_jump(right, target, on_true)?;
                        self.operations.push(Operation::Label(skip));
                    }
                    return Some(());
                }
                _ => {}
            }
        }
        let condition = self.condition(expression)?;
        self.operations.push(Operation::BranchIf {
            condition,
            target,
            on_true,
        });
        Some(())
    }

    pub(super) fn fixed_if(
        &mut self,
        condition: &Expression,
        yes: &[Statement],
        no: &[Statement],
    ) -> Option<()> {
        let otherwise = self.label();
        let join = self.label();
        self.conditional_jump(condition, otherwise, false)?;
        self.statements(yes)?;
        if !no.is_empty() && control::falls_through(yes) {
            self.operations.push(Operation::Jump(join));
        }
        self.operations.push(Operation::Label(otherwise));
        self.statements(no)?;
        self.operations.push(Operation::Label(join));
        Some(())
    }
}

fn swapped(operator: BinaryOperator) -> BinaryOperator {
    use BinaryOperator::*;
    match operator {
        Less => Greater,
        LessEqual => GreaterEqual,
        Greater => Less,
        GreaterEqual => LessEqual,
        other => other,
    }
}

fn branch(operator: BinaryOperator, on_true: bool) -> (u8, u8) {
    use BinaryOperator::*;
    let (options, bit) = match operator {
        Equal => (12, 2),
        NotEqual => (4, 2),
        Less => (12, 0),
        LessEqual => (4, 1),
        Greater => (12, 1),
        GreaterEqual => (4, 0),
        _ => unreachable!(),
    };
    (if on_true { options } else { options ^ 8 }, bit)
}

impl Generator {
    pub(super) fn emit_wide_graph_condition(
        &mut self,
        condition: Condition,
        on_true: bool,
        target: Label,
        registers: &[Option<Registers>],
    ) {
        let (mut operator, mut left, mut right) = match condition {
            Condition::Value(value) => (
                BinaryOperator::NotEqual,
                value,
                Value {
                    ty: value.ty,
                    source: Source::Constant(0),
                },
            ),
            Condition::Compare {
                operator,
                left,
                right,
            } => (operator, left, right),
        };
        if matches!(left.source, Source::Constant(_))
            && !matches!(right.source, Source::Constant(_))
        {
            std::mem::swap(&mut left, &mut right);
            operator = swapped(operator);
        }
        let signed = left.ty.is_signed();
        let equality = matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual);
        let a = self.wide_graph_operand(left, registers).low;
        let immediate = match right.source {
            Source::Constant(bits) if signed || equality => i16::try_from(bits as i32)
                .ok()
                .map(|immediate| Instruction::CompareWordImmediate { a, immediate }),
            _ => None,
        }
        .or_else(|| match right.source {
            Source::Constant(bits) if !signed => u16::try_from(bits)
                .ok()
                .map(|immediate| Instruction::CompareLogicalWordImmediate { a, immediate }),
            _ => None,
        });
        let compare = immediate.unwrap_or_else(|| {
            let b = self.wide_graph_operand(right, registers).low;
            if signed || equality {
                Instruction::CompareWord { a, b }
            } else {
                Instruction::CompareLogicalWord { a, b }
            }
        });
        self.output.instructions.push(compare);
        let (options, bit) = branch(operator, on_true);
        self.emit_branch_conditional_to(options, bit, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_comparison_owned_by_a_local_keeps_its_boolean_value() {
        let empty = HashMap::new();
        let mut graph = Graph {
            operations: Vec::new(),
            values: 0,
            signed_word_promotions: Default::default(),
            word_subtrahend_extension: mwcc_versions::WordSubtrahendExtension::FullWidth,
            computed_unsigned_addend_zero_extends: false,
            optimization: mwcc_versions::Optimization::O4,
            wide_word_demand_starts_at_o2: false,
            materialized_word_promotions: Default::default(),
            named_values: Default::default(),
            bindings: HashMap::new(),
            types: HashMap::new(),
            globals: &empty,
            returns: &empty,
            parameters: &HashMap::new(),
            indirects: &HashMap::new(),
            returned: None,
            return_type: Type::Void,
            fixed_bindings: false,
            labels: 0,
            loop_targets: Vec::new(),
            aggregate_slots: HashMap::new(),
            stack_end: 8,
            allow_implicit_calls: false,
        };
        let left = graph.fresh(Type::Int);
        let right = graph.fresh(Type::Int);
        let result = graph.fresh(Type::Int);
        graph.operations.push(Operation::Binary {
            retain_pair_carry: false,
            result,
            operator: BinaryOperator::Less,
            left,
            right,
        });
        graph.bindings.insert("saved".into(), result);
        assert!(matches!(graph.predicate(result), Condition::Value(v) if v == result));
        assert_eq!(graph.operations.len(), 1);
        graph.bindings.clear();
        assert!(matches!(
            graph.predicate(result),
            Condition::Compare {
                operator: BinaryOperator::Less,
                ..
            }
        ));
        assert!(graph.operations.is_empty());
    }
}
