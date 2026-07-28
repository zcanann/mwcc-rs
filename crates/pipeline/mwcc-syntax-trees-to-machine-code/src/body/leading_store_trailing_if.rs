//! Cross-statement scheduling for a doubly-linked-list front insertion.
//!
//! Dolphin's `DLAddFront` is a small but important scheduler probe: MWCC moves
//! the following null test between materializing and storing the leading zero.
//! Keeping the recognizer here prevents the ordinary statement emitter from
//! either serializing the region or growing another source-specific branch.

#[allow(unused_imports)]
use super::*;

struct MemberStore {
    offset: i16,
    pointee: Pointee,
}

struct LeadingStoreTrailingIfPlan<'a> {
    list_name: &'a str,
    cell_name: &'a str,
    next: MemberStore,
    previous: MemberStore,
    back_link: MemberStore,
}

impl Generator {
    /// Emit `cell->next=list; cell->prev=0; if(list) list->prev=cell; return cell;`.
    ///
    /// The zero load opens a latency slot. Mainline MWCC fills that slot with
    /// the list null test before completing the zero store.
    pub(crate) fn try_leading_store_trailing_if(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = self.leading_store_trailing_if_plan(function) else {
            return Ok(false);
        };
        let list = self.lookup_general(plan.list_name).ok_or_else(|| {
            Diagnostic::error("linked-list head is not in a general register")
        })?;
        let cell = self.lookup_general(plan.cell_name).ok_or_else(|| {
            Diagnostic::error("linked-list cell is not in a general register")
        })?;

        self.output.pre_scheduled = true;
        self.output.instructions.push(displacement_store(
            plan.next.pointee,
            list,
            cell,
            plan.next.offset,
        )?);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        let (options, condition_bit) =
            self.emit_condition_test(&Expression::Variable(plan.list_name.to_string()))?;
        self.output.instructions.push(displacement_store(
            plan.previous.pointee,
            GENERAL_SCRATCH,
            cell,
            plan.previous.offset,
        )?);
        let skip_back_link = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.output.instructions.push(displacement_store(
            plan.back_link.pointee,
            cell,
            list,
            plan.back_link.offset,
        )?);
        let continuation = self.output.instructions.len();
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[skip_back_link]
        else {
            unreachable!()
        };
        *target = continuation;

        self.evaluate_general(
            &Expression::Variable(plan.cell_name.to_string()),
            Eabi::general_result().number,
        )?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn leading_store_trailing_if_plan<'a>(
        &self,
        function: &'a Function,
    ) -> Option<LeadingStoreTrailingIfPlan<'a>> {
        if !function.locals.is_empty()
            || !function.guards.is_empty()
            || function_makes_call(function)
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
        {
            return None;
        }
        let [
            Statement::Store {
                target: next_target,
                value: Expression::Variable(list_value),
            },
            Statement::Store {
                target: previous_target,
                value: previous_value,
            },
            Statement::If {
                condition,
                then_body,
                else_body,
            },
        ] = function.statements.as_slice()
        else {
            return None;
        };
        if constant_value(previous_value) != Some(0) || !else_body.is_empty() {
            return None;
        }
        let Expression::Variable(condition_name) = condition else {
            return None;
        };
        if condition_name != list_value {
            return None;
        }
        let [Statement::Store {
            target: back_link_target,
            value: Expression::Variable(back_link_value),
        }] = then_body.as_slice()
        else {
            return None;
        };
        let Expression::Variable(return_name) = function.return_expression.as_ref()? else {
            return None;
        };
        if back_link_value != return_name {
            return None;
        }

        let parse_member = |target: &'a Expression| {
            let Expression::Member {
                base: member_base,
                offset,
                member_type,
                index_stride: None,
            } = target
            else {
                return None;
            };
            let Expression::Variable(base_name) = member_base.as_ref() else {
                return None;
            };
            Some((
                base_name.as_str(),
                MemberStore {
                    offset: i16::try_from(*offset).ok()?,
                    pointee: pointee_of_type(*member_type)?,
                },
            ))
        };
        let (cell_name, next) = parse_member(next_target)?;
        let (previous_base, previous) = parse_member(previous_target)?;
        let (back_link_base, back_link) = parse_member(back_link_target)?;
        if previous_base != cell_name
            || return_name != cell_name
            || back_link_base != list_value
            || next.pointee.size() != 4
            || previous.pointee.size() != 4
            || back_link.pointee.size() != 4
        {
            return None;
        }

        Some(LeadingStoreTrailingIfPlan {
            list_name: list_value,
            cell_name,
            next,
            previous,
            back_link,
        })
    }
}
