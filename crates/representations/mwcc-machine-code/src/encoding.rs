//! Instruction encoding: each `Instruction` to its 32-bit big-endian word, plus
//! the PowerPC instruction-form helpers. Encodings are verified against real
//! `mwcceppc` output by the differential oracle.

use crate::instruction::Instruction;

impl Instruction {
    /// Encode to a 32-bit big-endian instruction word.
    pub fn encode(&self) -> u32 {
        match *self {
            Instruction::AddImmediate { d, a, immediate } => d_form(14, d, a, immediate as u16),
            Instruction::AddImmediateCarryingRecord { d, a, immediate } => d_form(13, d, a, immediate as u16),
            Instruction::AddImmediateCarrying { d, a, immediate } => d_form(12, d, a, immediate as u16),
            Instruction::AddImmediateShifted { d, a, immediate } => d_form(15, d, a, immediate as u16),
            Instruction::OrImmediate { a, s, immediate } => d_form(24, s, a, immediate),
            Instruction::OrImmediateShifted { a, s, immediate } => d_form(25, s, a, immediate),
            Instruction::Add { d, a, b } => xo_form(d, a, b, 266),
            Instruction::AddRecord { d, a, b } => xo_form(d, a, b, 266) | 1,
            Instruction::SubtractFrom { d, a, b } => xo_form(d, a, b, 40),
            Instruction::SubtractFromRecord { d, a, b } => xo_form(d, a, b, 40) | 1,
            Instruction::Negate { d, a } => xo_form(d, a, 0, 104),
            Instruction::NegateRecord { d, a } => (xo_form(d, a, 0, 104)) | 1,
            Instruction::AndImmediateRecord { a, s, immediate } => (28 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | (immediate as u32),
            Instruction::Nor { a, s, b } => logical_form(s, a, b, 124),
            Instruction::Nand { a, s, b } => logical_form(s, a, b, 476),
            Instruction::Eqv { a, s, b } => logical_form(s, a, b, 284),
            Instruction::CountLeadingZeros { a, s } => logical_form(s, a, 0, 26),
            Instruction::ExtendSignByte { a, s } => logical_form(s, a, 0, 954),
            Instruction::ExtendSignByteRecord { a, s } => logical_form(s, a, 0, 954) | 1,
            Instruction::ExtendSignHalfword { a, s } => logical_form(s, a, 0, 922),
            Instruction::ExtendSignHalfwordRecord { a, s } => logical_form(s, a, 0, 922) | 1,
            Instruction::AndComplement { a, s, b } => logical_form(s, a, b, 60),
            Instruction::OrComplement { a, s, b } => logical_form(s, a, b, 412),
            Instruction::SubtractFromImmediate { d, a, immediate } => d_form(8, d, a, immediate as u16),
            Instruction::SubtractFromCarrying { d, a, b } => xo_form(d, a, b, 8),
            Instruction::SubtractFromExtended { d, a, b } => xo_form(d, a, b, 136),
            Instruction::SubtractFromExtendedRecord { d, a, b } => xo_form(d, a, b, 136) | 1,
            Instruction::SubtractFromZeroExtended { d, a } => (31 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | (200 << 1),
            Instruction::AddCarrying { d, a, b } => xo_form(d, a, b, 10),
            Instruction::AddExtended { d, a, b } => xo_form(d, a, b, 138),
            Instruction::AddToZeroExtended { d, a } => (31 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | (202 << 1),
            Instruction::MultiplyLow { d, a, b } => xo_form(d, a, b, 235),
            Instruction::MultiplyLowRecord { d, a, b } => xo_form(d, a, b, 235) | 1,
            Instruction::MultiplyHighWord { d, a, b } => xo_form(d, a, b, 75),
            Instruction::MultiplyHighWordUnsigned { d, a, b } => xo_form(d, a, b, 11),
            Instruction::MultiplyImmediate { d, a, immediate } => d_form(7, d, a, immediate as u16),
            Instruction::DivideWord { d, a, b } => xo_form(d, a, b, 491),
            Instruction::DivideWordUnsigned { d, a, b } => xo_form(d, a, b, 459),
            // slwi rA,rS,n == rlwinm rA,rS,n,0,31-n
            Instruction::ShiftLeftImmediate { a, s, shift } => {
                let mask_end = 31 - shift as u32;
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | (mask_end << 1)
            }
            Instruction::Or { a, s, b } => logical_form(s, a, b, 444),
            Instruction::OrRecord { a, s, b } => logical_form(s, a, b, 444) | 1,
            Instruction::And { a, s, b } => logical_form(s, a, b, 28),
            Instruction::AndRecord { a, s, b } => logical_form(s, a, b, 28) | 1,
            Instruction::Xor { a, s, b } => logical_form(s, a, b, 316),
            Instruction::XorRecord { a, s, b } => logical_form(s, a, b, 316) | 1,
            Instruction::ShiftLeftWord { a, s, b } => logical_form(s, a, b, 24),
            Instruction::ShiftRightAlgebraicWord { a, s, b } => logical_form(s, a, b, 792),
            Instruction::ShiftRightWord { a, s, b } => logical_form(s, a, b, 536),
            Instruction::ShiftRightAlgebraicImmediate { a, s, shift } => {
                (31 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | (824 << 1)
            }
            Instruction::ShiftRightAlgebraicImmediateRecord { a, s, shift } => {
                (31 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | (824 << 1) | 1
            }
            // srwi rA,rS,n == rlwinm rA,rS,32-n,n,31
            Instruction::ShiftRightLogicalImmediate { a, s, shift } => {
                let rotate = 32 - shift as u32;
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | (rotate << 11) | ((shift as u32) << 6) | (31 << 1)
            }
            Instruction::XorImmediate { a, s, immediate } => d_form(26, s, a, immediate),
            Instruction::XorImmediateShifted { a, s, immediate } => d_form(27, s, a, immediate),
            Instruction::StoreWord { s, a, offset } => d_form(36, s, a, offset as u16),
            Instruction::StoreByte { s, a, offset } => d_form(38, s, a, offset as u16),
            Instruction::StoreHalfword { s, a, offset } => d_form(44, s, a, offset as u16),
            Instruction::StoreFloatSingle { s, a, offset } => d_form(52, s, a, offset as u16),
            Instruction::StoreWordIndexed { s, a, b } => xo_form(s, a, b, 151),
            Instruction::StoreByteIndexed { s, a, b } => xo_form(s, a, b, 215),
            Instruction::StoreHalfwordIndexed { s, a, b } => xo_form(s, a, b, 407),
            Instruction::StoreFloatSingleIndexed { s, a, b } => xo_form(s, a, b, 663),
            Instruction::LoadFloatDouble { d, a, offset } => d_form(50, d, a, offset as u16),
            Instruction::LoadFloatDoubleIndexed { d, a, b } => xo_form(d, a, b, 599),
            Instruction::StoreFloatDoubleIndexed { s, a, b } => xo_form(s, a, b, 727),
            // clrlwi rA,rS,n == rlwinm rA,rS,0,n,31
            Instruction::ClearLeftImmediate { a, s, clear } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((clear as u32) << 6) | (31 << 1)
            }
            Instruction::ClearLeftImmediateRecord { a, s, clear } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((clear as u32) << 6) | (31 << 1) | 1
            }
            Instruction::AndContiguousMask { a, s, begin, end } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((begin as u32) << 6) | ((end as u32) << 1)
            }
            Instruction::RotateAndMask { a, s, shift, begin, end } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | ((begin as u32) << 6) | ((end as u32) << 1)
            }
            Instruction::RotateAndMaskRecord { a, s, shift, begin, end } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | ((begin as u32) << 6) | ((end as u32) << 1) | 1
            }
            Instruction::RotateAndMaskVariable { a, s, b, begin, end } => {
                (23 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((b as u32) << 11) | ((begin as u32) << 6) | ((end as u32) << 1)
            }
            Instruction::RotateAndMaskInsert { a, s, shift, begin, end } => {
                (20 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | ((begin as u32) << 6) | ((end as u32) << 1)
            }
            Instruction::AndMaskRecord { a, s, begin, end } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((begin as u32) << 6) | ((end as u32) << 1) | 1
            }
            Instruction::FloatAddSingle { d, a, b } => a_form(59, d, a, b, 0, 21),
            Instruction::FloatSubtractSingle { d, a, b } => a_form(59, d, a, b, 0, 20),
            Instruction::FloatMultiplySingle { d, a, c } => a_form(59, d, a, 0, c, 25),
            Instruction::FloatDivideSingle { d, a, b } => a_form(59, d, a, b, 0, 18),
            Instruction::FloatMultiplyAddSingle { d, a, c, b } => a_form(59, d, a, b, c, 29),
            Instruction::FloatMultiplySubtractSingle { d, a, c, b } => a_form(59, d, a, b, c, 28),
            Instruction::FloatNegativeMultiplySubtractSingle { d, a, c, b } => a_form(59, d, a, b, c, 30),
            Instruction::FloatNegativeMultiplyAddSingle { d, a, c, b } => a_form(59, d, a, b, c, 31),
            Instruction::FloatSelect { d, a, c, b } => a_form(63, d, a, b, c, 23),
            Instruction::FloatAddDouble { d, a, b } => a_form(63, d, a, b, 0, 21),
            Instruction::FloatSubtractDouble { d, a, b } => a_form(63, d, a, b, 0, 20),
            Instruction::FloatMultiplyDouble { d, a, c } => a_form(63, d, a, 0, c, 25),
            Instruction::FloatDivideDouble { d, a, b } => a_form(63, d, a, b, 0, 18),
            Instruction::FloatMultiplyAddDouble { d, a, c, b } => a_form(63, d, a, b, c, 29),
            Instruction::FloatMultiplySubtractDouble { d, a, c, b } => a_form(63, d, a, b, c, 28),
            Instruction::FloatNegativeMultiplySubtractDouble { d, a, c, b } => a_form(63, d, a, b, c, 30),
            Instruction::RoundToSingle { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (12 << 1),
            Instruction::FloatMove { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (72 << 1),
            // frsqrte: opcode 63, A-form xo 26 (fc 40 08 34 = frsqrte f2,f1)
            Instruction::FloatReciprocalSqrtEstimate { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (26 << 1),
            Instruction::FloatNegate { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (40 << 1),
            Instruction::FloatAbsolute { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (264 << 1),
            Instruction::ConvertToIntegerWordZero { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (15 << 1),
            Instruction::StoreWordWithUpdate { s, a, offset } => d_form(37, s, a, offset as u16),
            Instruction::LoadWord { d, a, offset } => d_form(32, d, a, offset as u16),
            Instruction::LoadWordWithUpdate { d, a, offset } => d_form(33, d, a, offset as u16),
            Instruction::LoadFloatDoubleWithUpdate { d, a, offset } => d_form(51, d, a, offset as u16),
            Instruction::LoadFloatSingleWithUpdate { d, a, offset } => d_form(49, d, a, offset as u16),
            Instruction::StoreFloatSingleWithUpdate { s, a, offset } => d_form(53, s, a, offset as u16),
            Instruction::StoreFloatDoubleWithUpdate { s, a, offset } => d_form(55, s, a, offset as u16),
            Instruction::LoadByteZeroWithUpdate { d, a, offset } => d_form(35, d, a, offset as u16),
            Instruction::LoadHalfZeroWithUpdate { d, a, offset } => d_form(41, d, a, offset as u16),
            Instruction::StoreByteWithUpdate { s, a, offset } => d_form(39, s, a, offset as u16),
            Instruction::LoadByteZero { d, a, offset } => d_form(34, d, a, offset as u16),
            Instruction::LoadHalfwordZero { d, a, offset } => d_form(40, d, a, offset as u16),
            Instruction::LoadHalfwordAlgebraic { d, a, offset } => d_form(42, d, a, offset as u16),
            Instruction::LoadFloatSingle { d, a, offset } => d_form(48, d, a, offset as u16),
            Instruction::LoadWordIndexed { d, a, b } => xo_form(d, a, b, 23),
            Instruction::LoadByteZeroIndexed { d, a, b } => xo_form(d, a, b, 87),
            Instruction::LoadHalfwordZeroIndexed { d, a, b } => xo_form(d, a, b, 279),
            Instruction::LoadHalfwordAlgebraicIndexed { d, a, b } => xo_form(d, a, b, 343),
            Instruction::LoadFloatSingleIndexed { d, a, b } => xo_form(d, a, b, 535),
            Instruction::StoreFloatDouble { s, a, offset } => d_form(54, s, a, offset as u16),
            Instruction::FloatCompareOrdered { a, b } => (63 << 26) | ((a as u32) << 16) | ((b as u32) << 11) | (32 << 1),
            Instruction::FloatCompareUnordered { a, b } => (63 << 26) | ((a as u32) << 16) | ((b as u32) << 11),
            Instruction::FloatCompareUnorderedField { crf, a, b } => (63 << 26) | ((crf as u32) << 23) | ((a as u32) << 16) | ((b as u32) << 11),
            Instruction::MoveFromConditionRegister { d } => (31 << 26) | ((d as u32) << 21) | (19 << 1),
            // mffs frD (63/583; measured fc 00 04 8e for f0)
            Instruction::MoveFromFpscr { d } => 0xFC00_048E | ((d as u32) << 21),
            // mtcrf CRM,rS (31/144; measured 7c cf f1 20 for 255,r6)
            Instruction::MoveToConditionRegisterFields { mask, s } => 0x7C00_0120 | ((s as u32) << 21) | ((mask as u32) << 12),
            // mtfsf FM,frB (63/711; measured fd fe 05 8e for 255,f0)
            Instruction::MoveToFpscrFields { mask, b } => 0xFC00_058E | ((mask as u32) << 17) | ((b as u32) << 11),
            // stmw rS,d(rA) (opcode 47; measured bd a3 00 14 for r13,20(r3))
            Instruction::StoreMultipleWord { s, a, offset } => (47 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | (offset as u16 as u32),
            // lmw rD,d(rA) (opcode 46; measured b9 a3 00 14 for r13,20(r3))
            Instruction::LoadMultipleWord { d, a, offset } => (46 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | (offset as u16 as u32),
            Instruction::ConditionRegisterOr { d, a, b } => (19 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | ((b as u32) << 11) | (449 << 1),
            Instruction::ConditionRegisterClear { d } => 0x4C00_0182 | ((d as u32) << 21) | ((d as u32) << 16) | ((d as u32) << 11),
            Instruction::CompareWordImmediate { a, immediate } => (11 << 26) | ((a as u32) << 16) | (immediate as u16 as u32),
            Instruction::CompareWordImmediateField { crf, a, immediate } => (11 << 26) | ((crf as u32) << 23) | ((a as u32) << 16) | (immediate as u16 as u32),
            Instruction::CompareWordField { crf, a, b } => (31 << 26) | ((crf as u32) << 23) | ((a as u32) << 16) | ((b as u32) << 11),
            Instruction::CompareWord { a, b } => (31 << 26) | ((a as u32) << 16) | ((b as u32) << 11),
            Instruction::CompareLogicalWordImmediate { a, immediate } => (10 << 26) | ((a as u32) << 16) | (immediate as u32),
            Instruction::CompareLogicalWord { a, b } => (31 << 26) | ((a as u32) << 16) | ((b as u32) << 11) | (32 << 1),
            // resolved positionally in encode_text
            Instruction::BranchConditionalForward { .. } => 0,
            Instruction::Branch { .. } => 0,
            Instruction::BranchConditionalToLinkRegister { options, condition_bit } => {
                (19 << 26) | ((options as u32) << 21) | ((condition_bit as u32) << 16) | (16 << 1)
            }
            Instruction::PairedSingleQuantizedLoad { d, a, offset, w, i } => {
                (56 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | ((w as u32) << 15) | ((i as u32) << 12) | ((offset as u32) & 0xfff)
            }
            Instruction::PairedSingleQuantizedStore { s, a, offset, w, i } => {
                (60 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((w as u32) << 15) | ((i as u32) << 12) | ((offset as u32) & 0xfff)
            }
            Instruction::BranchToLinkRegister => 0x4E80_0020,
            Instruction::BranchToLinkRegisterAndLink => 0x4E80_0021,
            // The displacement is supplied by the relocation; emit the placeholder.
            Instruction::BranchAndLink { .. } => 0x4800_0001,
            Instruction::BranchExternal { .. } => 0x4800_0000,
            Instruction::MoveFromLinkRegister { d } => 0x7C08_02A6 | ((d as u32) << 21),
            Instruction::MoveToLinkRegister { s } => 0x7C08_03A6 | ((s as u32) << 21),
            Instruction::MoveToCountRegister { s } => 0x7C09_03A6 | ((s as u32) << 21),
            Instruction::BranchToCountRegister => 0x4E80_0420,
            Instruction::BranchToCountRegisterAndLink => 0x4E80_0421,
            // The SPR/TBR field is the two 5-bit halves of the number, SWAPPED:
            // instruction bits 11-15 = spr[0:4], bits 16-20 = spr[5:9].
            Instruction::MoveFromSpr { d, spr } => {
                let field = ((spr as u32 & 0x1F) << 5) | ((spr as u32 >> 5) & 0x1F);
                (31 << 26) | ((d as u32) << 21) | (field << 11) | (339 << 1)
            }
            Instruction::MoveFromTimeBase { d, tbr } => {
                let field = ((tbr as u32 & 0x1F) << 5) | ((tbr as u32 >> 5) & 0x1F);
                (31 << 26) | ((d as u32) << 21) | (field << 11) | (371 << 1)
            }
            Instruction::MoveToSpr { spr, s } => {
                let field = ((spr as u32 & 0x1F) << 5) | ((spr as u32 >> 5) & 0x1F);
                (31 << 26) | ((s as u32) << 21) | (field << 11) | (467 << 1)
            }
            Instruction::MoveFromSegmentRegister { d, segment } => {
                (31 << 26) | ((d as u32) << 21) | ((segment as u32) << 16) | (595 << 1)
            }
            Instruction::MoveToSegmentRegister { segment, s } => {
                (31 << 26) | ((s as u32) << 21) | ((segment as u32) << 16) | (210 << 1)
            }
            Instruction::MoveFromMsr { d } => (31 << 26) | ((d as u32) << 21) | (83 << 1),
            Instruction::MoveToMsr { s } => (31 << 26) | ((s as u32) << 21) | (146 << 1),
            Instruction::InstructionSynchronize => 0x4C00_012C,
            Instruction::Synchronize => 0x7C00_04AC,
            Instruction::EnforceInOrderIo => 0x7C00_06AC,
            Instruction::ReturnFromInterrupt => 0x4C00_0064,
            Instruction::CacheOp { primary, xo, a, b } => {
                ((primary as u32) << 26) | ((a as u32) << 16) | ((b as u32) << 11) | ((xo as u32) << 1)
            }
            Instruction::SystemCall => 0x4400_0002,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Instruction;

    #[test]
    fn encodes_move_from_segment_register_fields() {
        assert_eq!(
            Instruction::MoveFromSegmentRegister { d: 16, segment: 0 }.encode(),
            0x7e00_04a6
        );
        assert_eq!(
            Instruction::MoveFromSegmentRegister { d: 31, segment: 15 }.encode(),
            0x7fef_04a6
        );
    }

    #[test]
    fn encodes_move_to_segment_register_fields() {
        assert_eq!(
            Instruction::MoveToSegmentRegister { segment: 0, s: 16 }.encode(),
            0x7e00_01a4
        );
        assert_eq!(
            Instruction::MoveToSegmentRegister { segment: 15, s: 31 }.encode(),
            0x7fef_01a4
        );
    }

    #[test]
    fn encodes_float_select_operand_order() {
        assert_eq!(
            Instruction::FloatSelect {
                d: 31,
                a: 1,
                c: 2,
                b: 3,
            }
            .encode(),
            0xffe1_18ae
        );
    }
}

fn d_form(opcode: u32, d: u8, a: u8, immediate: u16) -> u32 {
    (opcode << 26) | ((d as u32) << 21) | ((a as u32) << 16) | (immediate as u32)
}
fn xo_form(d: u8, a: u8, b: u8, extended_opcode: u32) -> u32 {
    (31 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | ((b as u32) << 11) | (extended_opcode << 1)
}
/// Logical/shift register form: opcode 31, rS in the D slot, rA in the A slot, rB.
fn logical_form(s: u8, a: u8, b: u8, extended_opcode: u32) -> u32 {
    (31 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((b as u32) << 11) | (extended_opcode << 1)
}
fn a_form(opcode: u32, d: u8, a: u8, b: u8, c: u8, extended_opcode: u32) -> u32 {
    (opcode << 26)
        | ((d as u32) << 21)
        | ((a as u32) << 16)
        | ((b as u32) << 11)
        | ((c as u32) << 6)
        | (extended_opcode << 1)
}
