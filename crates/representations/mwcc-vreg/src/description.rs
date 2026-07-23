//! The machine description: how the allocator reads and rewrites the registers
//! of a [`mwcc_machine_code::Instruction`].
//!
//! An allocator needs two things from each instruction — which registers it
//! *defines* and which it *uses* (to build live ranges), and a way to *rewrite*
//! those registers (to apply an assignment). Both come from one traversal,
//! [`for_each_register`], which visits every register field of an instruction in
//! turn with its [`RegisterRole`] and [`Class`]. Reading collects; rewriting
//! mutates in place — one match, never two that could drift apart.
//!
//! Register *values* here are plain physical numbers; a virtual register is
//! carried as a [`crate::Reg`] by selection and only becomes a number once
//! allocation resolves it. The description is about *which fields are registers
//! of which class and role*, independent of whether a given value is virtual.

use mwcc_machine_code::Instruction;

use crate::register::Class;

/// Whether an instruction writes a register (`Define`) or reads it (`Use`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterRole {
    Define,
    Use,
}

/// One register field of an instruction: its role, class, and current value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterOperand {
    pub role: RegisterRole,
    pub class: Class,
    pub register: u8,
}

/// Visit every register field of `instruction` in turn — definitions and uses,
/// each with its [`Class`] — passing a mutable reference so a caller can read or
/// rewrite it. This single traversal is the machine description: the allocator's
/// only knowledge of an instruction's register structure.
///
/// Caveats worth their honesty: `stwu`'s base is reported as a use (it is also
/// updated, but is always the pinned stack pointer, never a virtual). A call
/// (`bl`) has no register *fields*; its implicit argument/return/clobber set is
/// the ABI's, and is handled where calls are selected rather than here.
pub fn for_each_register(instruction: &mut Instruction, mut visit: impl FnMut(RegisterRole, Class, &mut u8)) {
    use Class::{Float as F, General as G};
    use Instruction::*;
    use RegisterRole::{Define as D, Use as U};

    match instruction {
        // d = rA op rB / rA op SIMM — general destination, general sources.
        Add { d, a, b } | AddRecord { d, a, b } | MultiplyLowRecord { d, a, b } | SubtractFrom { d, a, b } | SubtractFromRecord { d, a, b } | SubtractFromCarrying { d, a, b } | AddExtended { d, a, b }
        | AddCarrying { d, a, b } | SubtractFromExtended { d, a, b } | SubtractFromExtendedRecord { d, a, b }
        | MultiplyLow { d, a, b } | MultiplyHighWord { d, a, b } | MultiplyHighWordUnsigned { d, a, b }
        | DivideWord { d, a, b } | DivideWordUnsigned { d, a, b } => {
            visit(D, G, d);
            visit(U, G, a);
            visit(U, G, b);
        }
        // addi/addis use rA=0 to mean the literal value 0 — that encoding IS `li`/`lis`,
        // and PowerPC never reads r0 here — so a zero base is NOT a use of r0. Reporting
        // it as one adds phantom r0 dependencies that block scheduling (e.g. the LR-reload
        // hoist refuses to move past a `li`). subfic/mulli/addze have no such convention.
        AddImmediate { d, a, .. } | AddImmediateShifted { d, a, .. } => {
            visit(D, G, d);
            if *a != 0 {
                visit(U, G, a);
            }
        }
        SubtractFromImmediate { d, a, .. } | MultiplyImmediate { d, a, .. } | AddToZeroExtended { d, a }
        | SubtractFromZeroExtended { d, a }
        | AddImmediateCarryingRecord { d, a, .. } | AddImmediateCarrying { d, a, .. } => {
            visit(D, G, d);
            visit(U, G, a);
        }
        Negate { d, a } | NegateRecord { d, a } => {
            visit(D, G, d);
            visit(U, G, a);
        }
        // rA = rS op rB — PowerPC's logical/shift form: rA destination, rS/rB sources.
        Nor { a, s, b } | Nand { a, s, b } | Eqv { a, s, b } | AndComplement { a, s, b } | OrComplement { a, s, b } | Or { a, s, b } | OrRecord { a, s, b } | And { a, s, b } | AndRecord { a, s, b }
        | Xor { a, s, b } | XorRecord { a, s, b } | ShiftLeftWord { a, s, b } | ShiftRightAlgebraicWord { a, s, b } | ShiftRightWord { a, s, b } => {
            visit(D, G, a);
            visit(U, G, s);
            visit(U, G, b);
        }
        // rA = op(rS) — single general source.
        CountLeadingZeros { a, s } | ExtendSignByte { a, s } | ExtendSignByteRecord { a, s } | ExtendSignHalfword { a, s }
        | ExtendSignHalfwordRecord { a, s }
        | OrImmediate { a, s, .. } | OrImmediateShifted { a, s, .. } | ShiftLeftImmediate { a, s, .. } | ShiftRightAlgebraicImmediate { a, s, .. } | ShiftRightAlgebraicImmediateRecord { a, s, .. }
        | ShiftRightLogicalImmediate { a, s, .. } | XorImmediate { a, s, .. } | AndImmediateRecord { a, s, .. } | XorImmediateShifted { a, s, .. }
        | ClearLeftImmediate { a, s, .. } | ClearLeftImmediateRecord { a, s, .. } | AndContiguousMask { a, s, .. } | RotateAndMask { a, s, .. } | RotateAndMaskRecord { a, s, .. }
        | AndMaskRecord { a, s, .. } => {
            visit(D, G, a);
            visit(U, G, s);
        }
        // rlwnm reads the rotate amount from register `b` as well.
        RotateAndMaskVariable { a, s, b, .. } => {
            visit(D, G, a);
            visit(U, G, s);
            visit(U, G, b);
        }
        // rlwimi inserts into the existing bits of `a`, so it both uses and defines it.
        RotateAndMaskInsert { a, s, .. } => {
            visit(D, G, a);
            visit(U, G, a);
            visit(U, G, s);
        }
        // Loads: general destination, general base (+ index for the x-forms). As
        // with addi, a load/store base rA=0 means the literal address 0, not r0 —
        // skip it so global accesses (`lwz r3,0(0)`+reloc) carry no phantom r0 dep.
        LoadWord { d, a, .. } | LoadByteZero { d, a, .. } | LoadHalfwordZero { d, a, .. }
        | LoadHalfwordAlgebraic { d, a, .. } => {
            visit(D, G, d);
            if *a != 0 { visit(U, G, a); }
        }
        // The update form reads AND rewrites the base; the Use keeps the base's
        // range open (its post-update value extends through later uses anyway).
        LoadWordWithUpdate { d, a, .. } | LoadByteZeroWithUpdate { d, a, .. } | LoadHalfZeroWithUpdate { d, a, .. } => {
            visit(D, G, d);
            visit(U, G, a);
            visit(D, G, a);
        }
        LoadWordIndexed { d, a, b } | LoadByteZeroIndexed { d, a, b } | LoadHalfwordZeroIndexed { d, a, b }
        | LoadHalfwordAlgebraicIndexed { d, a, b } => {
            visit(D, G, d);
            if *a != 0 { visit(U, G, a); }
            visit(U, G, b);
        }
        // Float loads: float destination, general base (+ index).
        LoadFloatSingle { d, a, .. } | LoadFloatDouble { d, a, .. } | PairedSingleQuantizedLoad { d, a, .. } => {
            visit(D, F, d);
            if *a != 0 { visit(U, G, a); }
        }
        // The update form reads AND rewrites the general base.
        LoadFloatDoubleWithUpdate { d, a, .. } | LoadFloatSingleWithUpdate { d, a, .. } => {
            visit(D, F, d);
            visit(U, G, a);
            visit(D, G, a);
        }
        StoreFloatDoubleWithUpdate { s, a, .. } | StoreFloatSingleWithUpdate { s, a, .. } => {
            visit(U, F, s);
            visit(U, G, a);
            visit(D, G, a);
        }
        LoadFloatSingleIndexed { d, a, b } | LoadFloatDoubleIndexed { d, a, b } => {
            visit(D, F, d);
            if *a != 0 { visit(U, G, a); }
            visit(U, G, b);
        }
        // Stores: no destination — the value and base (+ index) are all uses. The
        // store-with-update base is always r1 (≠0), so the guard is a no-op there.
        StoreWord { s, a, .. } | StoreByte { s, a, .. } | StoreHalfword { s, a, .. }
        | StoreWordWithUpdate { s, a, .. } => {
            visit(U, G, s);
            if *a != 0 { visit(U, G, a); }
        }
        // The byte store-with-update rewrites its base (a general register).
        StoreByteWithUpdate { s, a, .. } => {
            visit(U, G, s);
            visit(U, G, a);
            visit(D, G, a);
        }
        StoreWordIndexed { s, a, b } | StoreByteIndexed { s, a, b } | StoreHalfwordIndexed { s, a, b } => {
            visit(U, G, s);
            if *a != 0 { visit(U, G, a); }
            visit(U, G, b);
        }
        StoreFloatSingle { s, a, .. } | StoreFloatDouble { s, a, .. } | PairedSingleQuantizedStore { s, a, .. } => {
            visit(U, F, s);
            if *a != 0 { visit(U, G, a); }
        }
        StoreFloatSingleIndexed { s, a, b } | StoreFloatDoubleIndexed { s, a, b } => {
            visit(U, F, s);
            if *a != 0 { visit(U, G, a); }
            visit(U, G, b);
        }
        // Float arithmetic — all operands float.
        FloatAddSingle { d, a, b } | FloatSubtractSingle { d, a, b } | FloatDivideSingle { d, a, b }
        | FloatAddDouble { d, a, b } | FloatSubtractDouble { d, a, b } | FloatDivideDouble { d, a, b } => {
            visit(D, F, d);
            visit(U, F, a);
            visit(U, F, b);
        }
        FloatMultiplySingle { d, a, c } | FloatMultiplyDouble { d, a, c } => {
            visit(D, F, d);
            visit(U, F, a);
            visit(U, F, c);
        }
        FloatMultiplyAddSingle { d, a, c, b } | FloatMultiplySubtractSingle { d, a, c, b }
        | FloatNegativeMultiplySubtractSingle { d, a, c, b }
        | FloatNegativeMultiplyAddSingle { d, a, c, b }
        | FloatSelect { d, a, c, b }
        | FloatMultiplyAddDouble { d, a, c, b } | FloatMultiplySubtractDouble { d, a, c, b }
        | FloatNegativeMultiplySubtractDouble { d, a, c, b } => {
            visit(D, F, d);
            visit(U, F, a);
            visit(U, F, c);
            visit(U, F, b);
        }
        FloatMove { d, b } | FloatNegate { d, b } | FloatAbsolute { d, b } | ConvertToIntegerWordZero { d, b } | RoundToSingle { d, b } | FloatReciprocalSqrtEstimate { d, b } => {
            visit(D, F, d);
            visit(U, F, b);
        }
        // Compares define cr0 (not a GPR/FPR), so they only use their operands.
        FloatCompareOrdered { a, b } | FloatCompareUnordered { a, b } | FloatCompareUnorderedField { a, b, .. } => {
            visit(U, F, a);
            visit(U, F, b);
        }
        // mfcr defines a GPR from the condition register; cror only touches cr bits.
        MoveFromConditionRegister { d } => visit(D, G, d),
        MoveFromFpscr { d } => visit(D, F, d),
        MoveToConditionRegisterFields { s, .. } => visit(U, G, s),
        MoveToFpscrFields { b, .. } => visit(U, F, b),
        // stmw/lmw touch rS..r31 / rD..r31 — inline-asm-only (never scheduled or
        // allocated), so listing the named register and the base is sufficient.
        StoreMultipleWord { s, a, .. } => {
            visit(U, G, s);
            visit(U, G, a);
        }
        LoadMultipleWord { d, a, .. } => {
            visit(D, G, d);
            visit(U, G, a);
        }
        ConditionRegisterOr { .. } | ConditionRegisterClear { .. } => {}
        CompareWord { a, b } | CompareLogicalWord { a, b } | CompareWordField { a, b, .. } => {
            visit(U, G, a);
            visit(U, G, b);
        }
        CompareWordImmediate { a, .. } | CompareWordImmediateField { a, .. } | CompareLogicalWordImmediate { a, .. } => {
            visit(U, G, a);
        }
        // Link-register and count-register moves.
        MoveFromLinkRegister { d } => visit(D, G, d),
        MoveToLinkRegister { s } => visit(U, G, s),
        MoveToCountRegister { s } => visit(U, G, s),
        // SPR/MSR moves (inline-asm only; the SPR/MSR are not virtual registers).
        MoveFromSpr { d, .. }
        | MoveFromTimeBase { d, .. }
        | MoveFromSegmentRegister { d, .. }
        | MoveFromMsr { d } => visit(D, G, d),
        MoveToSpr { s, .. } | MoveToSegmentRegister { s, .. } | MoveToMsr { s } => visit(U, G, s),
        // Cache ops address (rA|0)+rB — both are uses (rA=0 is the literal 0).
        CacheOp { a, b, .. } => {
            if *a != 0 {
                visit(U, G, a);
            }
            visit(U, G, b);
        }
        // No register fields: branches, the call, and the barrier/system ops
        // (their ABI/system registers are implicit).
        BranchConditionalForward { .. } | BranchConditionalToLinkRegister { .. } | BranchToLinkRegister | BranchToLinkRegisterAndLink
        | Branch { .. } | BranchToCountRegister | BranchToCountRegisterAndLink | BranchAndLink { .. } | BranchExternal { .. }
        | InstructionSynchronize | Synchronize | EnforceInOrderIo | ReturnFromInterrupt | SystemCall => {}
    }
}

/// The register operands of an instruction — its definitions and uses, each with
/// class — collected via [`for_each_register`]. The instruction is not modified.
pub fn register_operands(instruction: &Instruction) -> Vec<RegisterOperand> {
    let mut operands = Vec::new();
    // `for_each_register` takes `&mut` to allow rewriting; reading clones first so
    // the caller's instruction is untouched.
    let mut copy = instruction.clone();
    for_each_register(&mut copy, |role, class, register| {
        operands.push(RegisterOperand { role, class, register: *register });
    });
    operands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs(instruction: &Instruction) -> Vec<(Class, u8)> {
        register_operands(instruction)
            .into_iter()
            .filter(|operand| operand.role == RegisterRole::Define)
            .map(|operand| (operand.class, operand.register))
            .collect()
    }
    fn uses(instruction: &Instruction) -> Vec<(Class, u8)> {
        register_operands(instruction)
            .into_iter()
            .filter(|operand| operand.role == RegisterRole::Use)
            .map(|operand| (operand.class, operand.register))
            .collect()
    }

    #[test]
    fn add_defines_d_and_uses_a_b_all_general() {
        let add = Instruction::Add { d: 3, a: 4, b: 5 };
        assert_eq!(defs(&add), [(Class::General, 3)]);
        assert_eq!(uses(&add), [(Class::General, 4), (Class::General, 5)]);
    }

    #[test]
    fn logical_form_destination_is_a_not_d() {
        // `or rA,rS,rB`: rA is the destination.
        let or = Instruction::Or { a: 6, s: 3, b: 4 };
        assert_eq!(defs(&or), [(Class::General, 6)]);
        assert_eq!(uses(&or), [(Class::General, 3), (Class::General, 4)]);
    }

    #[test]
    fn a_load_defines_the_value_and_uses_the_base() {
        let load = Instruction::LoadWord { d: 3, a: 13, offset: 0 };
        assert_eq!(defs(&load), [(Class::General, 3)]);
        assert_eq!(uses(&load), [(Class::General, 13)]);
    }

    #[test]
    fn a_float_load_defines_a_float_and_uses_a_general_base() {
        let load = Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 };
        assert_eq!(defs(&load), [(Class::Float, 1)]);
        assert_eq!(uses(&load), [(Class::General, 3)]); // base is a GPR
    }

    #[test]
    fn a_store_has_no_definition_only_uses() {
        let store = Instruction::StoreWord { s: 4, a: 3, offset: 8 };
        assert!(defs(&store).is_empty());
        assert_eq!(uses(&store), [(Class::General, 4), (Class::General, 3)]);
    }

    #[test]
    fn a_compare_uses_its_operands_and_defines_no_gpr() {
        let compare = Instruction::CompareWord { a: 3, b: 4 };
        assert!(defs(&compare).is_empty());
        assert_eq!(uses(&compare), [(Class::General, 3), (Class::General, 4)]);
    }

    #[test]
    fn a_fused_multiply_add_uses_three_floats() {
        let fma = Instruction::FloatMultiplyAddSingle { d: 1, a: 2, c: 3, b: 4 };
        assert_eq!(defs(&fma), [(Class::Float, 1)]);
        assert_eq!(uses(&fma), [(Class::Float, 2), (Class::Float, 3), (Class::Float, 4)]);
    }

    #[test]
    fn a_return_has_no_register_operands() {
        assert!(register_operands(&Instruction::BranchToLinkRegister).is_empty());
    }

    #[test]
    fn for_each_register_can_rewrite_in_place() {
        let mut add = Instruction::Add { d: 3, a: 4, b: 5 };
        // Renumber every register by +10.
        for_each_register(&mut add, |_role, _class, register| *register += 10);
        assert_eq!(add, Instruction::Add { d: 13, a: 14, b: 15 });
    }

    #[test]
    fn rewriting_only_a_definition_leaves_uses_untouched() {
        let mut load = Instruction::LoadWord { d: 0, a: 3, offset: 4 };
        for_each_register(&mut load, |role, _class, register| {
            if role == RegisterRole::Define {
                *register = 9;
            }
        });
        assert_eq!(load, Instruction::LoadWord { d: 9, a: 3, offset: 4 });
    }
}
