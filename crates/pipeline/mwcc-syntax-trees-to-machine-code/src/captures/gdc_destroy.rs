//! gdc_destroy: the `__destroy_global_chain` exact-match capture (fire 531).
//! Walks `__global_destructor_chain`, calling each node's destructor with -1 and
//! unlinking it — mwcc's canonical bottom-test `while` (a `b` to the tail test,
//! `bne` back into the body) with a `bctrl` call through the node's function
//! pointer. Captured verbatim (loop + indirect-call codegen not yet general).
//! The recognizer checks the runtime loop's semantics rather than an AST hash:
//! project headers spell the destructor-call macro differently, and source
//! preprocessing can legitimately add or remove casts without changing this
//! canonical operation.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LoopKind, Statement, Type};

const CHAIN_HEAD: &str = "__global_destructor_chain";

fn without_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    matches!(without_casts(expression), Expression::Variable(candidate) if candidate == name)
}

fn is_integer(expression: &Expression, expected: i64) -> bool {
    matches!(without_casts(expression), Expression::IntegerLiteral(value) if *value == expected)
}

fn is_member(expression: &Expression, base_name: &str, expected_offset: u32) -> bool {
    matches!(
        without_casts(expression),
        Expression::Member {
            base,
            offset,
            index_stride: None,
            ..
        } if *offset == expected_offset && is_variable(base, base_name)
    )
}

fn is_chain_assignment(expression: &Expression, local_name: &str) -> bool {
    matches!(
        without_casts(expression),
        Expression::Assign { target, value }
            if is_variable(target, local_name) && is_variable(value, CHAIN_HEAD)
    )
}

fn is_loop_condition(expression: &Expression, local_name: &str) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = without_casts(expression)
    else {
        return false;
    };
    (is_chain_assignment(left, local_name) && is_integer(right, 0))
        || (is_integer(left, 0) && is_chain_assignment(right, local_name))
}

fn is_chain_advance(statement: &Statement, local_name: &str) -> bool {
    matches!(
        statement,
        Statement::Store { target, value }
            if is_variable(target, CHAIN_HEAD) && is_member(value, local_name, 0)
    )
}

fn is_destructor_call(statement: &Statement, local_name: &str) -> bool {
    let Statement::Expression(Expression::CallThrough { target, arguments }) = statement else {
        return false;
    };
    arguments.len() == 2
        && is_member(target, local_name, 4)
        && is_member(&arguments[0], local_name, 8)
        && is_integer(&arguments[1], -1)
}

fn is_canonical_destroy_loop(function: &Function) -> bool {
    let [local] = function.locals.as_slice() else {
        return false;
    };
    if local.initializer.is_some()
        || local.is_static
        || local.is_volatile
        || local.array_length.is_some()
    {
        return false;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return false;
    };
    let [advance, call] = body.as_slice() else {
        return false;
    };
    is_loop_condition(condition, &local.name)
        && is_chain_advance(advance, &local.name)
        && is_destructor_call(call, &local.name)
}

impl Generator {
    pub(super) fn try_gdc_destroy(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__destroy_global_chain"
            || function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        if !is_canonical_destroy_loop(function) {
            return Ok(false);
        }
        // No context gate: no pooled constants / anonymous labels here, so the
        // skipped-inline fingerprint carries no @N bump — the semantic shape identifies
        // the function and its bytes are identical across every project sharing
        // the runtime source (the ctx fingerprint varies: ww/strikers differ from pik).
        // -- emit (non-leaf, 16-byte frame, only LR saved) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [4, 11] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_to(labels[&11]); // b <test>
        self.bind_label(labels[&4]); // loop body: iter is in r3
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        }); // iter->next
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1)); // li r4,-1
        self.record_relocation(RelocationKind::EmbSda21, "__global_destructor_chain");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        }); // head = iter->next
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 3,
            offset: 4,
        }); // iter->destructor
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 8,
        }); // iter->object
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink); // (*destructor)(object, -1)
        self.bind_label(labels[&11]); // test: iter = head
        self.record_relocation(RelocationKind::EmbSda21, "__global_destructor_chain");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 }); // cmplwi r3,0
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne <body>
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::LocalDeclaration;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.to_owned())
    }

    fn member(local_name: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(variable(local_name)),
            offset,
            member_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
            index_stride: None,
        }
    }

    fn canonical_function() -> Function {
        let local_name = "node";
        Function {
            return_type: Type::Void,
            name: "__destroy_global_chain".to_owned(),
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![LocalDeclaration {
                declared_type: Type::StructPointer { element_size: 12 },
                name: local_name.to_owned(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: vec![],
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: Box::new(Expression::Assign {
                        target: Box::new(variable(local_name)),
                        value: Box::new(variable(CHAIN_HEAD)),
                    }),
                    right: Box::new(Expression::IntegerLiteral(0)),
                }),
                step: None,
                body: vec![
                    Statement::Store {
                        target: variable(CHAIN_HEAD),
                        value: member(local_name, 0),
                    },
                    Statement::Expression(Expression::CallThrough {
                        target: Box::new(Expression::Cast {
                            target_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
                            operand: Box::new(member(local_name, 4)),
                        }),
                        arguments: vec![member(local_name, 8), Expression::IntegerLiteral(-1)],
                    }),
                ],
            }],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_cast_insensitive_canonical_loop() {
        assert!(is_canonical_destroy_loop(&canonical_function()));
    }

    #[test]
    fn rejects_a_different_member_schedule() {
        let mut function = canonical_function();
        let Statement::Loop { body, .. } = &mut function.statements[0] else {
            unreachable!()
        };
        let Statement::Store { value, .. } = &mut body[0] else {
            unreachable!()
        };
        let Expression::Member { offset, .. } = value else {
            unreachable!()
        };
        *offset = 4;
        assert!(!is_canonical_destroy_loop(&function));
    }
}
