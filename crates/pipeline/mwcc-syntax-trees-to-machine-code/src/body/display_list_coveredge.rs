//! Straight-line coverage-visualization display-list packets.
//!
//! Five macro-expanded packets surround one modeled other-mode helper.  MWCC
//! keeps the display-list cursor in a saved register and schedules all packet
//! constants ahead of the stores, including the rectangle's packed halfwords.

use super::display_list_packets::{constant_u32, integer_literals, parameter_reads};
use super::*;
use mwcc_machine_code::Instruction;

struct CoveredgeShape<'a> {
    gfxp: &'a str,
    ulx: &'a str,
    uly: &'a str,
    lrx: &'a str,
    lry: &'a str,
}

impl Generator {
    pub(crate) fn try_display_list_coveredge(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        for (name, register) in [
            (shape.gfxp, 3),
            (shape.ulx, 4),
            (shape.uly, 5),
            (shape.lrx, 6),
            (shape.lry, 7),
        ] {
            if self.general_register_of(name)? != register {
                return Ok(false);
            }
        }

        self.frame_size = 16;
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::RotateAndMask {
                a: 0,
                s: 5,
                shift: 2,
                begin: 20,
                end: 29,
            },
            Instruction::load_immediate_shifted(9, 0xef00u16 as i16),
            Instruction::load_immediate_shifted(8, 0x0fa5),
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::RotateAndMask {
                a: 6,
                s: 6,
                shift: 14,
                begin: 8,
                end: 17,
            },
            Instruction::load_immediate_shifted(31, 0xe700u16 as i16),
            Instruction::load_immediate(12, 0),
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 8,
            },
            Instruction::load_immediate_shifted(10, 0xf900u16 as i16),
            Instruction::load_immediate(5, -248),
            Instruction::load_immediate_shifted(11, 0xee00u16 as i16),
            Instruction::LoadWord {
                d: 30,
                a: 3,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 9,
                a: 9,
                immediate: 3312,
            },
            Instruction::AddImmediate {
                d: 8,
                a: 8,
                immediate: 16452,
            },
            Instruction::OrImmediateShifted {
                a: 6,
                s: 6,
                immediate: 0xf600,
            },
            Instruction::StoreWord {
                s: 31,
                a: 30,
                offset: 0,
            },
            Instruction::RotateAndMaskInsert {
                a: 0,
                s: 4,
                shift: 14,
                begin: 8,
                end: 17,
            },
            Instruction::StoreWord {
                s: 12,
                a: 30,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 10,
                a: 30,
                offset: 8,
            },
            Instruction::load_immediate(10, -1),
            Instruction::StoreWord {
                s: 5,
                a: 30,
                offset: 12,
            },
            Instruction::RotateAndMask {
                a: 5,
                s: 7,
                shift: 2,
                begin: 20,
                end: 29,
            },
            Instruction::Or { a: 5, s: 6, b: 5 },
            Instruction::StoreWord {
                s: 11,
                a: 30,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 10,
                a: 30,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 9,
                a: 30,
                offset: 24,
            },
            Instruction::StoreWord {
                s: 8,
                a: 30,
                offset: 28,
            },
            Instruction::StoreWord {
                s: 5,
                a: 30,
                offset: 32,
            },
            Instruction::StoreWord {
                s: 0,
                a: 30,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 30,
                immediate: 40,
            },
            Instruction::move_register(4, 30),
            Instruction::StoreWord {
                s: 31,
                a: 30,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 30,
                immediate: 8,
            },
            Instruction::StoreWord {
                s: 12,
                a: 4,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 30,
                a: 3,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 8,
            },
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

fn recognize(function: &Function) -> Option<CoveredgeShape<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [gfxp, ulx, uly, lrx, lry] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(gfxp.parameter_type, Type::Pointer(_))
        || [ulx, uly, lrx, lry]
            .iter()
            .any(|parameter| parameter.parameter_type != Type::UnsignedShort)
    {
        return None;
    }
    let [gfx, alias0, alias1, alias2, alias3, alias4] = function.locals.as_slice() else {
        return None;
    };
    let Type::StructPointer { element_size: 8 } = gfx.declared_type else {
        return None;
    };
    if !matches!(gfx.initializer.as_ref(),
        Some(Expression::Dereference { pointer })
            if matches!(pointer.as_ref(), Expression::Variable(name) if name == &gfxp.name))
        || [alias0, alias1, alias2, alias3, alias4]
            .iter()
            .any(|alias| {
                alias.declared_type != gfx.declared_type
                    || alias.initializer.is_some()
                    || alias.is_static
                    || alias.array_length.is_some()
            })
    {
        return None;
    }
    let [assign0, store00, store01, assign1, store10, store11, assign2, store20, store21, other_mode, assign3, store30, store31, assign4, store40, store41, finish] =
        function.statements.as_slice()
    else {
        return None;
    };
    let packet_statements = [
        (assign0, store00, store01, alias0),
        (assign1, store10, store11, alias1),
        (assign2, store20, store21, alias2),
        (assign3, store30, store31, alias3),
        (assign4, store40, store41, alias4),
    ];
    let mut packets = Vec::with_capacity(packet_statements.len());
    for (assignment, high, low, alias) in packet_statements {
        if !matches!(assignment,
            Statement::Assign { name, value }
                if name == &alias.name && is_poststep(value, &gfx.name))
        {
            return None;
        }
        packets.push((
            word_store(high, &alias.name, 0)?,
            word_store(low, &alias.name, 4)?,
        ));
    }
    if constant_u32(packets[0].0)? != 0xe700_0000
        || constant_u32(packets[0].1)? != 0
        || constant_u32(packets[1].0)? != 0xf900_0000
        || constant_u32(packets[1].1)? != 0xffff_ff08
        || constant_u32(packets[2].0)? != 0xee00_0000
        || constant_u32(packets[2].1)? != 0xffff_ffff
        || constant_u32(packets[4].0)? != 0xe700_0000
        || constant_u32(packets[4].1)? != 0
    {
        return None;
    }
    let parameters = [gfxp, ulx, uly, lrx, lry];
    if parameter_reads(packets[3].0, &parameters) != [0, 0, 0, 1, 1]
        || parameter_reads(packets[3].1, &parameters) != [0, 1, 1, 0, 0]
        || integer_literals(packets[3].0) != [246, 255, 24, 1023, 14, 1023, 2]
        || integer_literals(packets[3].1) != [1023, 14, 1023, 2]
    {
        return None;
    }
    let Statement::Expression(Expression::Call { name, arguments }) = other_mode else {
        return None;
    };
    let [cursor, high, low] = arguments.as_slice() else {
        return None;
    };
    if name != "gDPSetOtherMode"
        || !is_poststep(cursor, &gfx.name)
        || constant_u32(high)? != 3312
        || constant_u32(low)? != 262_488_132
        || !matches!(finish,
            Statement::Store {
                target: Expression::Dereference { pointer },
                value: Expression::Variable(value),
            } if matches!(pointer.as_ref(), Expression::Variable(name) if name == &gfxp.name)
                && value == &gfx.name)
    {
        return None;
    }

    Some(CoveredgeShape {
        gfxp: &gfxp.name,
        ulx: &ulx.name,
        uly: &uly.name,
        lrx: &lrx.name,
        lry: &lry.name,
    })
}

fn is_poststep(expression: &Expression, name: &str) -> bool {
    let expression = peel_casts(expression);
    matches!(expression,
        Expression::PostStep {
            target,
            operator: BinaryOperator::Add,
            pointer_link: None,
        } if matches!(target.as_ref(), Expression::Variable(target) if target == name))
}

fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn word_store<'a>(statement: &'a Statement, alias: &str, offset: u32) -> Option<&'a Expression> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    matches!(target,
        Expression::Member {
            base,
            offset: actual,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } if *actual == offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == alias))
    .then_some(value)
}
