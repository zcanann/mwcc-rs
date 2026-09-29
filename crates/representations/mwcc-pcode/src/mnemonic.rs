//! Map machine instructions to MWCC PCode opcodes (see [`crate::opcodes`]).

use mwcc_machine_code::Instruction;

use crate::opcodes::{self, OpcodeInfo};

/// The MWCC opcode an instruction corresponds to. Record forms share their
/// base opcode; `li`/`lis`/`mr` are MWCC's own pseudo-opcodes.
pub fn mwcc_mnemonic(instruction: &Instruction) -> &'static str {
    use Instruction::*;
    match instruction {
        AddImmediate { a: 0, .. } => "LI",
        AddImmediate { .. } => "ADDI",
        AddImmediateShifted { a: 0, .. } => "LIS",
        AddImmediateShifted { .. } => "ADDIS",
        Or { s, b, .. } if s == b => "MR",
        _ => {
            let name = format!("{instruction:?}");
            let head = name.split([' ', '{', '(']).next().unwrap_or("");
            let base = head.strip_suffix("Record").unwrap_or(head);
            VARIANTS
                .iter()
                .find(|(variant, _)| *variant == base)
                .map_or("ADDI", |(_, mnemonic)| mnemonic)
        }
    }
}

/// Scheduling facts for an instruction (defaults to an integer ALU op).
pub fn opcode_info(instruction: &Instruction) -> &'static OpcodeInfo {
    opcodes::by_mnemonic(mwcc_mnemonic(instruction))
        .or_else(|| opcodes::by_mnemonic("ADDI"))
        .expect("ADDI is in the opcode table")
}

const VARIANTS: &[(&str, &str)] = &[
    ("AddImmediateCarrying", "ADDIC"),
    ("OrImmediate", "ORI"),
    ("OrImmediateShifted", "ORIS"),
    ("Add", "ADD"),
    ("SubtractFrom", "SUBF"),
    ("Negate", "NEG"),
    ("AndImmediate", "ANDI"),
    ("AndImmediateShifted", "ANDIS"),
    ("Nor", "NOR"),
    ("Xor", "XOR"),
    ("Nand", "NAND"),
    ("Eqv", "EQV"),
    ("CountLeadingZeros", "CNTLZW"),
    ("ExtendSignByte", "EXTSB"),
    ("ExtendSignHalfword", "EXTSH"),
    ("AndComplement", "ANDC"),
    ("OrComplement", "ORC"),
    ("SubtractFromImmediate", "SUBFIC"),
    ("SubtractFromCarrying", "SUBFC"),
    ("SubtractFromExtended", "SUBFE"),
    ("SubtractFromZeroExtended", "SUBFZE"),
    ("AddCarrying", "ADDC"),
    ("AddExtended", "ADDE"),
    ("AddToZeroExtended", "ADDZE"),
    ("MultiplyLow", "MULLW"),
    ("MultiplyHighWord", "MULHW"),
    ("MultiplyHighWordUnsigned", "MULHWU"),
    ("MultiplyImmediate", "MULLI"),
    ("DivideWord", "DIVW"),
    ("DivideWordUnsigned", "DIVWU"),
    ("ShiftLeftImmediate", "RLWINM"),
    ("Or", "OR"),
    ("And", "AND"),
    ("ShiftLeftWord", "SLW"),
    ("ShiftRightAlgebraicWord", "SRAW"),
    ("ShiftRightWord", "SRW"),
    ("ShiftRightAlgebraicImmediate", "SRAWI"),
    ("ShiftRightLogicalImmediate", "RLWINM"),
    ("XorImmediate", "XORI"),
    ("XorImmediateShifted", "XORIS"),
    ("ClearLeftImmediate", "RLWINM"),
    ("AndContiguousMask", "RLWINM"),
    ("RotateAndMask", "RLWINM"),
    ("RotateAndMaskVariable", "RLWNM"),
    ("RotateAndMaskInsert", "RLWIMI"),
    ("AndMask", "RLWINM"),
    ("LoadWord", "LWZ"),
    ("LoadWordIndexed", "LWZX"),
    ("LoadWordWithUpdate", "LWZU"),
    ("LoadByteZero", "LBZ"),
    ("LoadByteZeroIndexed", "LBZX"),
    ("LoadHalfwordZero", "LHZ"),
    ("LoadHalfwordZeroIndexed", "LHZX"),
    ("LoadHalfwordAlgebraic", "LHA"),
    ("LoadHalfwordAlgebraicIndexed", "LHAX"),
    ("LoadMultipleWord", "LMW"),
    ("StoreWord", "STW"),
    ("StoreWordIndexed", "STWX"),
    ("StoreWordWithUpdate", "STWU"),
    ("StoreByte", "STB"),
    ("StoreByteIndexed", "STBX"),
    ("StoreHalfword", "STH"),
    ("StoreHalfwordIndexed", "STHX"),
    ("StoreMultipleWord", "STMW"),
    ("LoadFloatSingle", "LFS"),
    ("LoadFloatSingleIndexed", "LFSX"),
    ("LoadFloatSingleWithUpdate", "LFSU"),
    ("LoadFloatDouble", "LFD"),
    ("LoadFloatDoubleIndexed", "LFDX"),
    ("LoadFloatDoubleWithUpdate", "LFDU"),
    ("StoreFloatSingle", "STFS"),
    ("StoreFloatSingleIndexed", "STFSX"),
    ("StoreFloatSingleWithUpdate", "STFSU"),
    ("StoreFloatDouble", "STFD"),
    ("StoreFloatDoubleIndexed", "STFDX"),
    ("StoreFloatDoubleWithUpdate", "STFDU"),
    ("CompareWord", "CMP"),
    ("CompareWordImmediate", "CMPI"),
    ("CompareWordImmediateField", "CMPI"),
    ("CompareLogicalWord", "CMPL"),
    ("CompareLogicalWordImmediate", "CMPLI"),
    ("FloatCompareOrdered", "FCMPO"),
    ("FloatCompareUnordered", "FCMPU"),
    ("FloatAddSingle", "FADDS"),
    ("FloatSubtractSingle", "FSUBS"),
    ("FloatMultiplySingle", "FMULS"),
    ("FloatDivideSingle", "FDIVS"),
    ("FloatMultiplyAddSingle", "FMADDS"),
    ("FloatMultiplySubtractSingle", "FMSUBS"),
    ("FloatNegativeMultiplySubtractSingle", "FNMSUBS"),
    ("FloatNegativeMultiplyAddSingle", "FNMADDS"),
    ("FloatSelect", "FSEL"),
    ("FloatAddDouble", "FADD"),
    ("FloatSubtractDouble", "FSUB"),
    ("FloatMultiplyDouble", "FMUL"),
    ("FloatDivideDouble", "FDIV"),
    ("FloatMultiplyAddDouble", "FMADD"),
    ("FloatMultiplySubtractDouble", "FMSUB"),
    ("FloatNegativeMultiplySubtractDouble", "FNMSUB"),
    ("RoundToSingle", "FRSP"),
    ("FloatReciprocalSqrtEstimate", "FRSQRTE"),
    ("FloatMove", "FMR"),
    ("FloatNegate", "FNEG"),
    ("FloatAbsolute", "FABS"),
    ("ConvertToIntegerWordZero", "FCTIWZ"),
    ("PairedSingleAdd", "PS_ADD"),
    ("PairedSingleSubtract", "PS_SUB"),
    ("PairedSingleMultiply", "PS_MUL"),
    ("PairedSingleMultiplyScalar0", "PS_MULS0"),
    ("PairedSingleMultiplyScalar1", "PS_MULS1"),
    ("PairedSingleMultiplyAdd", "PS_MADD"),
    ("PairedSingleMultiplyAddScalar0", "PS_MADDS0"),
    ("PairedSingleMultiplyAddScalar1", "PS_MADDS1"),
    ("PairedSingleSum0", "PS_SUM0"),
    ("PairedSingleSum1", "PS_SUM1"),
    ("PairedSingleMove", "PS_MR"),
    ("PairedSingleMerge00", "PS_MERGE00"),
    ("PairedSingleMerge01", "PS_MERGE01"),
    ("PairedSingleMerge10", "PS_MERGE10"),
    ("PairedSingleMerge11", "PS_MERGE11"),
    ("PairedSingleQuantizedLoad", "PSQ_L"),
    ("PairedSingleQuantizedLoadWithUpdate", "PSQ_LU"),
    ("PairedSingleQuantizedStore", "PSQ_ST"),
    ("PairedSingleQuantizedStoreWithUpdate", "PSQ_STU"),
    ("Branch", "B"),
    ("BranchExternal", "B"),
    ("BranchAndLink", "BL"),
    ("BranchConditionalForward", "BC"),
    ("BranchConditionalToLinkRegister", "BCLR"),
    ("BranchToLinkRegister", "BLR"),
    ("BranchToCountRegister", "BCTR"),
    ("BranchToCountRegisterAndLink", "BCTRL"),
    ("BranchToLinkRegisterAndLink", "BLRL"),
    ("MoveFromLinkRegister", "MFLR"),
    ("MoveToLinkRegister", "MTLR"),
    ("MoveToCountRegister", "MTCTR"),
    ("MoveFromConditionRegister", "MFCR"),
    ("InstructionSynchronize", "ISYNC"),
    ("Synchronize", "SYNC"),
    ("EnforceInOrderIo", "EIEIO"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_pseudo_opcodes_and_record_forms() {
        assert_eq!(mwcc_mnemonic(&Instruction::AddImmediate { d: 3, a: 0, immediate: 1 }), "LI");
        assert_eq!(mwcc_mnemonic(&Instruction::Or { a: 3, s: 4, b: 4 }), "MR");
        assert_eq!(mwcc_mnemonic(&Instruction::Or { a: 3, s: 4, b: 5 }), "OR");
        assert_eq!(mwcc_mnemonic(&Instruction::AddRecord { d: 3, a: 4, b: 5 }), "ADD");
        assert_eq!(mwcc_mnemonic(&Instruction::LoadWord { d: 3, a: 4, offset: 0 }), "LWZ");
        assert_eq!(opcode_info(&Instruction::LoadWord { d: 3, a: 4, offset: 0 }).latency, 2);
    }
}
