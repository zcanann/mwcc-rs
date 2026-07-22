//! A guarded member decrement whose selected arm keeps an entry pointer in a
//! volatile home until a global-member method call.
//!
//! MWCC splits the pointer's live range at the guard. The condition keeps the
//! untouched `r3` alias and its loaded member in `r4`; the selected arm uses a
//! copy in `r5`, while the else edge still passes the untouched `r3` to its
//! call. Owning the whole diamond makes those path-specific aliases explicit.

#[allow(unused_imports)]
use super::*;

struct Plan<'a> {
    parameter: &'a str,
    condition_base: &'a Expression,
    member_offset: u32,
    member_type: Type,
    decrement_target: &'a Expression,
    then_tail: &'a [Statement],
    else_body: &'a [Statement],
    return_value: i16,
}

impl Generator {
    pub(crate) fn try_guarded_member_decrement_if_else(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = plan(function, &self.globals, self.behavior.frame_convention) else {
            return Ok(false);
        };
        let incoming = self
            .locations
            .get(plan.parameter)
            .filter(|location| location.class == ValueClass::General)
            .map(|location| location.register);
        if incoming != Some(Eabi::FIRST_GENERAL_ARGUMENT) {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 16;
        self.output.anonymous_label_bump = 3;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
        ]);

        let arm_home = Eabi::FIRST_GENERAL_ARGUMENT + 2;
        let condition_value = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.output.instructions.push(Instruction::move_register(
            arm_home,
            Eabi::FIRST_GENERAL_ARGUMENT,
        ));
        self.emit_member_load(
            plan.condition_base,
            plan.member_offset,
            plan.member_type,
            None,
            condition_value,
        )?;
        let cached_name = "@guarded_member_value".to_string();
        self.locations.insert(
            cached_name.clone(),
            Location {
                class: ValueClass::General,
                register: condition_value,
                // The signed/unsigned member load has already produced a
                // promoted word in r4. Treating the cached value as narrow
                // would redundantly emit `extsh.`/`extsb.` instead of MWCC's
                // plain `cmpwi r4,0`.
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        let cached_condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Variable(cached_name.clone())),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let (options, condition_bit) = self.emit_condition_test(&cached_condition)?;
        let branch_to_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });

        self.locations
            .get_mut(plan.parameter)
            .expect("guarded parameter location")
            .register = arm_home;
        let decrement = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::Variable(cached_name.clone())),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        self.emit_store(plan.decrement_target, &decrement)?;
        self.locations.remove(&cached_name);
        for statement in plan.then_tail {
            self.emit_statement(statement)?;
        }
        let branch_to_join = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let else_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_to_else]
        {
            *target = else_label;
        }
        self.locations
            .get_mut(plan.parameter)
            .expect("guarded parameter location")
            .register = Eabi::FIRST_GENERAL_ARGUMENT;
        for statement in plan.else_body {
            self.emit_statement(statement)?;
        }
        let join_label = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[branch_to_join] {
            *target = join_label;
        }
        // In this consumer-heavy diamond, build 81 fills the last call's
        // return-latency slot with the independent constant before reloading
        // LR (distinct from its simpler non-leaf constant-join schedule).
        self.load_integer_constant(Eabi::general_result().number, i64::from(plan.return_value));
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn plan<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
    frame_convention: FrameConvention,
) -> Option<Plan<'a>> {
    if frame_convention != FrameConvention::Predecrement
        || function.guards.len() != 0
        || function.parameters.len() != 1
        || reads_value_across_call(function)
    {
        return None;
    }
    let return_value = function
        .return_expression
        .as_ref()
        .and_then(constant_value)
        .and_then(|value| i16::try_from(value).ok())?;
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if then_body.is_empty()
        || else_body.is_empty()
        || !then_body.iter().chain(else_body).all(|statement| {
            matches!(
                statement,
                Statement::Store { .. } | Statement::Expression(_)
            )
        })
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(right.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Variable(parameter) = base.as_ref() else {
        return None;
    };
    if parameter != &function.parameters[0].name {
        return None;
    }
    let Statement::Store {
        target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: decremented,
                right: one,
            },
    } = &then_body[0]
    else {
        return None;
    };
    if !same_member(target, parameter, *offset, *member_type)
        || !same_member(decremented, parameter, *offset, *member_type)
        || !matches!(one.as_ref(), Expression::IntegerLiteral(1))
        || !then_body
            .iter()
            .any(|statement| endangered_member_call(statement, parameter, globals))
    {
        return None;
    }
    Some(Plan {
        parameter,
        condition_base: base,
        member_offset: *offset,
        member_type: *member_type,
        decrement_target: target,
        then_tail: &then_body[1..],
        else_body,
        return_value,
    })
}

fn same_member(expression: &Expression, parameter: &str, offset: u32, member_type: Type) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            offset: candidate_offset,
            member_type: candidate_type,
            index_stride: None,
        } if *candidate_offset == offset
            && *candidate_type == member_type
            && matches!(base.as_ref(), Expression::Variable(name) if name == parameter)
    )
}

fn endangered_member_call(
    statement: &Statement,
    parameter: &str,
    globals: &std::collections::HashMap<String, Type>,
) -> bool {
    let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
        return false;
    };
    let [first, second] = arguments.as_slice() else {
        return false;
    };
    matches!(direct_member_base(first), Some(name) if globals.contains_key(name))
        && direct_member_base(second) == Some(parameter)
}

fn direct_member_base(expression: &Expression) -> Option<&str> {
    let base = match expression {
        Expression::MemberAddress {
            base,
            index_stride: None,
            ..
        } => base.as_ref(),
        Expression::AddressOf { operand } => match operand.as_ref() {
            Expression::Member {
                base,
                index_stride: None,
                ..
            } => base.as_ref(),
            _ => return None,
        },
        _ => return None,
    };
    match base {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}
