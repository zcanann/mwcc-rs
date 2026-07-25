//! Scheduling for a computed initializer that retains an incoming parameter.
//!
//! The inlined initializer loads a repeated absolute global before selecting a
//! saved result. MWCC splits that address load across two GPRs and fills the
//! prologue's linkage and save-store latency slots with its independent high
//! half and the retained incoming parameter. The first call then overlaps its
//! global-array address with the already-selected result argument.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_initializer_live_in(&mut self) {
        let Some(prefix) = initializer_live_in_prefix(&self.output) else {
            return;
        };

        let high = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT + 1);
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[prefix.global_high]
        else {
            unreachable!("initializer prefix high was matched")
        };
        *d = high;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[prefix.global_low]
        else {
            unreachable!("initializer prefix low was matched")
        };
        *a = high;

        // stwu; mflr; lis; mr retained; stw LR; addi; stw saved; lwz
        self.move_instruction_before(prefix.global_high, prefix.link_store);
        self.move_instruction_before(prefix.retained_copy + 1, prefix.link_store + 1);
        self.move_instruction_before(prefix.global_low, prefix.saved_store + 1);

        // lis array; mr result; addi address; li size; add indexed address; bl
        if let Some(call) = initializer_live_in_first_call(&self.output.instructions) {
            self.move_instruction_before(call.global_high, call.result_copy);
            self.move_instruction_before(call.size, call.indexed_address);
        }
    }
}

#[derive(Clone, Copy)]
struct InitializerLiveInPrefix {
    link_store: usize,
    retained_copy: usize,
    saved_store: usize,
    global_high: usize,
    global_low: usize,
}

fn initializer_live_in_prefix(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<InitializerLiveInPrefix> {
    let start = output.instructions.windows(8).position(|window| {
        matches!(
            window,
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
                Instruction::Or {
                    a: retained,
                    s: Eabi::FIRST_GENERAL_ARGUMENT,
                    b: Eabi::FIRST_GENERAL_ARGUMENT,
                },
                Instruction::StoreWord {
                    s: saved,
                    a: 1,
                    offset: 12,
                },
                Instruction::AddImmediateShifted {
                    d: global,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: low_destination,
                    a: low_base,
                    immediate: 0,
                },
                Instruction::LoadWord {
                    d: loaded,
                    a: load_base,
                    offset: 0,
                },
            ] if retained != saved
                && global == low_destination
                && global == low_base
                && global == loaded
                && global == load_base
        )
    })?;
    let has_high = output.relocations.iter().any(|relocation| {
        relocation.instruction_index == start + 5 && relocation.kind == RelocationKind::Addr16Ha
    });
    let has_low = output.relocations.iter().any(|relocation| {
        relocation.instruction_index == start + 6 && relocation.kind == RelocationKind::Addr16Lo
    });
    (has_high && has_low).then_some(InitializerLiveInPrefix {
        link_store: start + 2,
        retained_copy: start + 3,
        saved_store: start + 4,
        global_high: start + 5,
        global_low: start + 6,
    })
}

#[derive(Clone, Copy)]
struct InitializerLiveInFirstCall {
    result_copy: usize,
    global_high: usize,
    indexed_address: usize,
    size: usize,
}

fn initializer_live_in_first_call(
    instructions: &[Instruction],
) -> Option<InitializerLiveInFirstCall> {
    let start = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::Or {
                    a: Eabi::FIRST_GENERAL_ARGUMENT,
                    s: saved,
                    b: saved_b,
                },
                Instruction::AddImmediateShifted {
                    d: high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: address,
                    a: low_base,
                    ..
                },
                Instruction::Add {
                    d: second_argument,
                    a: indexed_base,
                    b: retained,
                },
                Instruction::AddImmediate {
                    d: third_argument,
                    a: 0,
                    ..
                },
                Instruction::BranchAndLink { .. },
            ] if saved == saved_b
                && high == low_base
                && *second_argument == Eabi::FIRST_GENERAL_ARGUMENT + 1
                && *third_argument == Eabi::FIRST_GENERAL_ARGUMENT + 2
                && address == indexed_base
                && retained != saved
        )
    })?;
    Some(InitializerLiveInFirstCall {
        result_copy: start,
        global_high: start + 1,
        indexed_address: start + 3,
        size: start + 4,
    })
}
