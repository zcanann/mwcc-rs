//! Stack images retained for narrow parameters in recovered unoptimized bodies.

use super::*;

/// Expand the canonical two-home frame and materialize the first narrow
/// parameter through its `r1+8` source image. The recovered-local plan is the
/// authority that this is an unoptimized source-home body; the instruction
/// prefix proves the currently supported indexed-use schedule.
pub(super) fn apply(
    generator: &mut Generator,
    function: &Function,
    recovered_local_plan: bool,
) -> Compilation<bool> {
    let [parameter] = function.parameters.as_slice() else {
        return Ok(false);
    };
    if !recovered_local_plan
        || !matches!(parameter.parameter_type, Type::Char | Type::UnsignedChar)
        || generator.frame_size != 16
        || generator.callee_saved.len() != 2
        || !matches!(
            generator.output.instructions.as_slice(),
            [
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
                Instruction::StoreWord {
                    a: 1,
                    offset: 12,
                    ..
                },
                Instruction::StoreWord {
                    a: 1,
                    offset: 8,
                    ..
                },
                Instruction::ShiftLeftImmediate { s: 3, .. },
                ..
            ]
        )
    {
        return Ok(false);
    }

    for instruction in &mut generator.output.instructions {
        match instruction {
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            } => {
                *instruction = Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -32,
                }
            }
            Instruction::StoreWord { a: 1, offset, .. }
            | Instruction::LoadWord { a: 1, offset, .. }
                if *offset >= 8 =>
            {
                *offset += 16;
            }
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            } => {
                *instruction = Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 32,
                }
            }
            _ => {}
        }
    }
    crate::insert_instruction_retargeting(
        generator,
        5,
        Instruction::StoreByte {
            s: 3,
            a: 1,
            offset: 8,
        },
    );
    crate::insert_instruction_retargeting(
        generator,
        6,
        Instruction::LoadByteZero {
            d: 0,
            a: 1,
            offset: 8,
        },
    );
    let Instruction::ShiftLeftImmediate { s, .. } = &mut generator.output.instructions[7] else {
        unreachable!("the recovered narrow parameter shift was classified above")
    };
    *s = 0;
    generator.frame_size = 32;
    Ok(true)
}
