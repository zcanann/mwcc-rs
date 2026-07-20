//! Generated linkage around otherwise verbatim inline-asm bodies.

use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, Relocation};
use mwcc_versions::{FrameConvention, PlainLinkageEpilogueStyle};

/// Wrap an asm body that lacks `nofralloc` (but uses the stack) in mwcc's generated
/// 16-byte frame: prologue `stwu r1,-16(r1); mr r31,r1` prepended, epilogue
/// `mr r10,r1; lwz r1,0(r1)` inserted at the resolved epilogue position: the
/// `frfree` directive when present, otherwise the end of the written body. Thus
/// a compiler-appended `blr` follows teardown while a written `blr` precedes it.
pub(super) fn wrap_auto_frame(
    instructions: &mut Vec<Instruction>,
    relocations: &mut [Relocation],
    entry_points: &mut [(String, usize)],
    insertion: usize,
) -> Compilation<()> {
    wrap_frame(
        instructions,
        relocations,
        entry_points,
        insertion,
        vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            }, // stwu r1, -16(r1)
            Instruction::move_register(31, 1), // mr r31, r1
        ],
        vec![
            Instruction::move_register(10, 1), // mr r10, r1
            Instruction::LoadWord {
                d: 1,
                a: 1,
                offset: 0,
            }, // lwz r1, 0(r1)
        ],
    )
}

/// Synthesize the ordinary non-leaf linkage requested by an explicit `fralloc`
/// directive. The instruction schedules are the same version decisions used by
/// C function linkage; only their insertion into a verbatim asm body is local.
pub(super) fn wrap_fralloc_frame(
    instructions: &mut Vec<Instruction>,
    relocations: &mut [Relocation],
    entry_points: &mut [(String, usize)],
    insertion: usize,
    convention: FrameConvention,
    epilogue_style: PlainLinkageEpilogueStyle,
) -> Compilation<()> {
    let (prologue, epilogue) = match convention {
        FrameConvention::Predecrement => (
            vec![
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
            ],
            vec![
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
            ],
        ),
        FrameConvention::LinkageFirst => {
            let prologue = vec![
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -8,
                },
            ];
            let stack_restore = Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 8,
            };
            let epilogue = match epilogue_style {
                PlainLinkageEpilogueStyle::ReloadBeforeStackRestore => vec![
                    Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset: 12,
                    },
                    stack_restore,
                    Instruction::MoveToLinkRegister { s: 0 },
                ],
                PlainLinkageEpilogueStyle::StackRestoreBeforeReload => vec![
                    stack_restore,
                    Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset: 4,
                    },
                    Instruction::MoveToLinkRegister { s: 0 },
                ],
            };
            (prologue, epilogue)
        }
    };
    wrap_frame(
        instructions,
        relocations,
        entry_points,
        insertion,
        prologue,
        epilogue,
    )
}

/// Insert a generated prologue and epilogue while preserving every index-based
/// branch, relocation, and secondary entry point in the written asm body.
fn wrap_frame(
    instructions: &mut Vec<Instruction>,
    relocations: &mut [Relocation],
    entry_points: &mut [(String, usize)],
    insertion: usize,
    prologue: Vec<Instruction>,
    epilogue: Vec<Instruction>,
) -> Compilation<()> {
    let prologue_len = prologue.len();
    let epilogue_len = epilogue.len();
    let shift =
        |index: usize| index + prologue_len + if index >= insertion { epilogue_len } else { 0 };
    for instruction in instructions.iter_mut() {
        match instruction {
            Instruction::Branch { target } => *target = shift(*target),
            Instruction::BranchConditionalForward { target, .. } => *target = shift(*target),
            _ => {}
        }
    }
    for relocation in relocations.iter_mut() {
        relocation.instruction_index = shift(relocation.instruction_index);
    }
    for (_, index) in entry_points.iter_mut() {
        *index = shift(*index);
    }
    let mut framed = Vec::with_capacity(instructions.len() + prologue_len + epilogue_len);
    framed.extend(prologue);
    framed.extend(instructions.drain(..insertion));
    framed.extend(epilogue);
    framed.append(instructions);
    *instructions = framed;
    Ok(())
}
