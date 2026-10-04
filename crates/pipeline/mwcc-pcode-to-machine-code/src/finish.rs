//! After INITIAL CODE, in MWCC's `CodeGen_Generator` order: instruction
//! scheduling on virtual registers, register coloring, prologue/epilogue
//! generation, final scheduling, and the physical [`MachineFunction`].
//!
//! Branch instructions carry *block* indices as targets until flattening
//! converts them to instruction positions.

use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, MachineFunction, Relocation, RelocationKind};
use mwcc_pcode::{Block, PCodeFunction, PInstr};

use crate::{coloring, schedule};

/// Scheduling policy for the final code.
#[derive(Debug, Clone, Copy)]
pub struct FinishOptions {
    /// MWCC's scheduler runs (`-O4`-style latency scheduling).
    pub schedule: bool,
    /// GC/1.x-2.x: a surviving copy tested against 0 is `mr.`.
    pub move_record: bool,
    /// Remove dead definitions while building interference (MWCC's
    /// `gDeleteDeadInstructions`).
    pub delete_dead: bool,
    /// Schedule for two integer units (IU2 takes simple integer operations):
    /// the machine model after GC/1.2.5, whose single-IU default model the
    /// decompilation documents.
    pub two_integer_units: bool,
    /// GC/3.x and Wii: accesses through one base register at disjoint
    /// offsets are independent in the first schedule.
    pub based_disambiguation: bool,
    /// `-O0`: no copy propagation or live-range splitting before coloring.
    pub unoptimized: bool,
    /// Fold `addi rX,rB,sym@l` into a following zero-displacement access
    /// even when the load's destination is rX (GC/3.0+).
    pub fold_absolute_into_own_base: bool,
    /// The LR reload follows every saved-register restore (GC/3.x, Wii)
    /// instead of preceding the final double reload or the GPR reloads.
    pub link_reload_after_float_restores: bool,
    /// The fewest saved GPRs handled by `_savegpr_N`/`_restgpr_N`.
    pub general_save_helper_minimum: usize,
    /// `-use_lmw_stmw on`: `stmw`/`lmw` where the helpers would be called.
    pub use_lmw_stmw: bool,
    /// GC/1.0-1.2.5n frames: `mflr; stw r0,4(r1); stwu`, the save area
    /// 8-aligned at the top of an 8-aligned frame, `stmw`/`lmw` instead of
    /// the helpers, and `addi r1` before `mtlr`.
    pub early_frame: bool,
    /// GC/1.1p1: the LR comes back from the caller's slot after the pop
    /// (`addi r1; lwz r0,4(r1); mtlr`).
    pub link_reload_after_pop: bool,
}

/// Schedule, color, frame, and flatten `pcode`. The last block is the exit
/// (return) block.
pub fn finish(
    mut pcode: PCodeFunction,
    makes_calls: bool,
    options: FinishOptions,
) -> Compilation<MachineFunction> {
    prune_unreachable(&mut pcode);
    schedule::TWO_INTEGER_UNITS.with(|flag| flag.set(options.two_integer_units));
    schedule::FREE_MARKERS.with(|flag| flag.set(options.early_frame && !toggle("MWCC_SCHED_MARKER_AT_CALL")));
    schedule::BASED.with(|flag| flag.set(options.based_disambiguation && std::env::var_os("MWCC_SCHED_NO_BASED").is_none()));
    schedule::FRAME_OBJECTS.with(|objects| *objects.borrow_mut() = pcode.frame_objects.clone());
    schedule::PRIVATE_FRAME_OBJECTS.with(|objects| *objects.borrow_mut() = pcode.private_frame_objects.clone());
    schedule::STRUCT_FRAME_OBJECTS.with(|objects| *objects.borrow_mut() = pcode.struct_frame_objects.clone());
    dump(&pcode, "INITIAL CODE");
    if !options.unoptimized && !toggle("MWCC_PCODE_NO_WEBS") {
        split_webs(&mut pcode);
    }
    if !options.unoptimized && !toggle("MWCC_PCODE_NO_COPYPROP") {
        // Eliminating one copy can expose another (`x = a` of a parameter).
        while propagate_physical_copies(&mut pcode) {}
    }
    if options.early_frame && !options.unoptimized && !toggle("MWCC_PCODE_NO_UPDATE_LOADS") {
        update_loads(&mut pcode);
    }
    // (GC/1.3 on, a sum read only as a load's base is its index.)
    if !options.early_frame && !options.unoptimized && !toggle("MWCC_PCODE_NO_INDEXED_SUM_LOADS") {
        fold_indexed_loads(&mut pcode);
    }
    if options.schedule && !toggle("MWCC_PCODE_NO_PRESCHEDULE") {
        for block in &mut pcode.blocks {
            schedule::schedule_block(&mut block.instructions, true);
        }
    }
    if !options.unoptimized && !toggle("MWCC_PCODE_NO_LATE_FORWARD") {
        forward_physical_reads(&mut pcode);
    }
    dump(&pcode, "AFTER INSTRUCTION SCHEDULING");
    // (Stores fold an absolute address before coloring.)
    if !options.unoptimized && !options.fold_absolute_into_own_base && !toggle("MWCC_PCODE_LATE_STORE_FOLD") {
        fold_absolute_stores(&mut pcode);
    }
    let colors = coloring::color(&mut pcode, options.delete_dead)?;
    dump(&pcode, "AFTER REGISTER COLORING");
    if !options.early_frame && !options.unoptimized && !toggle("MWCC_PCODE_NO_MOVE_RECORD") {
        move_records(&mut pcode, options.move_record);
    } else if options.move_record && !options.unoptimized && !toggle("MWCC_PCODE_NO_MOVE_RECORD") {
        move_records(&mut pcode, true);
    }
    if !options.early_frame && !options.unoptimized && !toggle("MWCC_PCODE_NO_REDUNDANT_COPIES") {
        remove_redundant_copies(&mut pcode);
    }
    // (GC/3.x folds at -O0 too.)
    if !options.unoptimized || options.fold_absolute_into_own_base {
        fold_absolute_displacements(&mut pcode, options.fold_absolute_into_own_base);
    }
    // GC/1.0-1.2.5n copy parameters aside with `addi d,s,0`
    // when two GPR parameters are used or the frame holds objects.
    if options.early_frame {
        let entry_copy = |instruction: &PInstr| {
            instruction.flags.entry_copy
                && matches!(instruction.instruction, Instruction::Or { a, s, b } if s == b)
        };
        let copies = pcode.blocks.iter().flat_map(|block| &block.instructions).filter(|i| entry_copy(i)).count();
        if copies > 0 && (pcode.referenced_general_parameters >= 2 || pcode.variable_frame_objects > 0) {
            for instruction in pcode.blocks.iter_mut().flat_map(|block| block.instructions.iter_mut()) {
                if entry_copy(instruction) {
                    if let Instruction::Or { a, s, .. } = instruction.instruction {
                        instruction.instruction = Instruction::AddImmediate { d: a, a: s, immediate: 0 };
                    }
                }
            }
        }
    }
    // (And a copy feeding a call with two or more word arguments before any
    // other call: the argument's own copy, or the copy of a value passed in
    // place; a cycle's final move out of r0 stays `mr`.)
    if options.early_frame && !options.unoptimized && !toggle("MWCC_PCODE_NO_ARGUMENT_ADDI_COPIES") {
        argument_addi_copies(&mut pcode);
    }
    // (And copies marked so when lowered.)
    if options.early_frame {
        for instruction in pcode.blocks.iter_mut().flat_map(|block| block.instructions.iter_mut()) {
            if let (true, Instruction::Or { a, s, b }) = (instruction.flags.addi_copy, instruction.instruction.clone()) {
                if s == b && s != 0 && a != s {
                    instruction.instruction = Instruction::AddImmediate { d: a, a: s, immediate: 0 };
                }
            }
        }
    }
    // Saved FPRs: the contiguous range from f31 down to the lowest used.
    // (Also FPRs assigned directly: `-O0` register variables.)
    let direct_float = pcode
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .flat_map(|instruction| instruction.defs(mwcc_pcode::Class::Float))
        .filter(|register| (14..32).contains(register))
        .min();
    let saved_float: Vec<u32> = match colors.saved_float.iter().copied().chain(direct_float).min() {
        Some(lowest) => (lowest..32).rev().collect(),
        None => Vec::new(),
    };
    // Gekko also saves the paired-single half of each saved FPR when the
    // function does single-precision arithmetic (rounding included).
    let paired = !saved_float.is_empty()
        && std::env::var_os("MWCC_PCODE_NO_PAIRED_SAVES").is_none()
        && pcode.blocks.iter().flat_map(|block| block.instructions.iter()).any(|instruction| {
            matches!(
                instruction.instruction,
                Instruction::FloatAddSingle { .. }
                    | Instruction::FloatSubtractSingle { .. }
                    | Instruction::FloatMultiplySingle { .. }
                    | Instruction::FloatDivideSingle { .. }
                    | Instruction::FloatMultiplyAddSingle { .. }
                    | Instruction::FloatMultiplySubtractSingle { .. }
                    | Instruction::FloatNegativeMultiplyAddSingle { .. }
                    | Instruction::FloatNegativeMultiplySubtractSingle { .. }
                    | Instruction::RoundToSingle { .. }
            ) || (!toggle("MWCC_PCODE_PAIRED_ARITHMETIC_ONLY")
                && matches!(
                    instruction.instruction,
                    Instruction::LoadFloatSingle { .. } | Instruction::LoadFloatSingleIndexed { .. }
                ))
        });
    // Callee-saved registers the code writes: claimed by coloring, or
    // assigned directly (`-O0` register variables).
    let mut saved = colors.saved_general.clone();
    for instruction in pcode.blocks.iter().flat_map(|block| block.instructions.iter()) {
        for defined in instruction.defs(mwcc_pcode::Class::General) {
            if (14..32).contains(&defined) && !saved.contains(&defined) {
                saved.push(defined);
            }
        }
    }
    saved.sort_unstable_by(|a, b| b.cmp(a));
    // Enough saved GPRs go through the helpers, which save rN..r31.
    let helper = saved.len() >= options.general_save_helper_minimum && !toggle("MWCC_PCODE_NO_SAVE_HELPERS");
    if helper {
        if !makes_calls && !options.use_lmw_stmw && !options.early_frame && toggle("MWCC_PCODE_NO_LEAF_HELPERS") {
            return Err(mwcc_core::Diagnostic::error("PCode frame: save helpers in a leaf function"));
        }
        let lowest = *saved.last().expect("saved registers");
        saved = (lowest..32).rev().collect();
    }
    // (Calling the helpers makes a leaf save the link register.)
    let makes_calls = makes_calls || (helper && !options.use_lmw_stmw && !options.early_frame);
    let framed = makes_calls || !saved.is_empty() || !saved_float.is_empty() || pcode.frame_local_bytes > 0;
    let float_frame = FloatFrame {
        link_reload_last: options.link_reload_after_float_restores,
        link_reload_after_pop: options.link_reload_after_pop,
        helper,
        multiple: options.use_lmw_stmw || options.early_frame,
        ..if options.early_frame {
            // (The reservation counts once registers are saved.)
            FloatFrame::early(
                &saved_float,
                &saved,
                if saved.is_empty() && saved_float.is_empty() {
                    pcode.frame_local_bytes
                } else {
                    pcode.frame_local_bytes.max(pcode.reserved_local_bytes)
                },
            )
        } else {
            FloatFrame::new(&saved_float, paired, &saved, pcode.frame_local_bytes)
        }
    };
    let plan = mwcc_vreg::FramePlan::with_local_region(saved, pcode.frame_local_bytes);
    let custom = !saved_float.is_empty() || helper || options.early_frame;
    let prologue = || if custom { float_frame.prologue() } else { plan.prologue() };
    let epilogue = || if custom || float_frame.link_reload_last { float_frame.epilogue() } else { plan.epilogue() };
    // A leaf frame neither saves nor restores the link register.
    let keep = |instruction: &Instruction| {
        makes_calls
            || !matches!(
                instruction,
                Instruction::MoveFromLinkRegister { .. }
                    | Instruction::MoveToLinkRegister { .. }
                    | Instruction::StoreWord { s: 0, a: 1, .. }
                    | Instruction::LoadWord { d: 0, a: 1, .. }
            )
    };

    let wrap = |instructions: Vec<Instruction>| -> Vec<PInstr> {
        instructions
            .into_iter()
            .map(|instruction| {
                let target = match &instruction {
                    Instruction::BranchAndLink { target } => Some(target.clone()),
                    _ => None,
                };
                // `stmw`/`lmw` read or write every register from rN up.
                let multiple = match instruction {
                    Instruction::StoreMultipleWord { s, .. } => Some((s, true)),
                    Instruction::LoadMultipleWord { d, .. } => Some((d, false)),
                    _ => None,
                };
                let mut wrapped = PInstr::new(instruction);
                if let Some((lowest, store)) = multiple {
                    // (They also issue serially: scheduled in place.)
                    wrapped.flags.serialize = true;
                    let registers = (lowest..32).map(mwcc_pcode::Register::general);
                    if store {
                        wrapped.implicit_uses.extend(registers);
                    } else {
                        wrapped.implicit_defs.extend(registers);
                    }
                }
                if let Some(target) = target {
                    wrapped.relocation = Some(mwcc_pcode::AttachedRelocation {
                        kind: mwcc_machine_code::RelocationKind::Rel24,
                        target: mwcc_machine_code::RelocationTarget::External(target),
                    });
                }
                wrapped
            })
            .collect()
    };
    // A loop back to the entry block must not re-run the prologue: the
    // entry gets a block of its own.
    if framed && pcode.blocks.iter().any(|block| block.successors.contains(&0)) && !toggle("MWCC_PCODE_NO_ENTRY_SPLIT") {
        for block in &mut pcode.blocks {
            for successor in &mut block.successors {
                *successor += 1;
            }
            for instruction in &mut block.instructions {
                if let Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. } = &mut instruction.instruction {
                    *target += 1;
                }
            }
        }
        for (targets, _) in &mut pcode.jump_tables {
            for target in targets.iter_mut() {
                *target += 1;
            }
        }
        let weight = pcode.blocks[0].weight;
        pcode.blocks.insert(0, mwcc_pcode::Block { instructions: Vec::new(), successors: vec![1], weight });
    }
    let exit = pcode.blocks.len() - 1;
    let mut framed_blocks = vec![false; pcode.blocks.len()];
    // Code that never returns (an endless loop) has no epilogue.
    let mut epilogue_length = 0;
    let exit_reached = exit == 0 || (0..exit).any(|block| pcode.blocks[block].successors.contains(&exit));
    if framed && !exit_reached {
        let mut entry = wrap(prologue().into_iter().filter(|i| keep(i)).collect());
        entry.append(&mut pcode.blocks[0].instructions);
        pcode.blocks[0].instructions = entry;
        framed_blocks[0] = true;
    } else if !exit_reached {
    } else if framed {
        let mut entry = wrap(prologue().into_iter().filter(|i| keep(i)).collect());
        entry.append(&mut pcode.blocks[0].instructions);
        pcode.blocks[0].instructions = entry;
        let mut epilogue: Vec<Instruction> = epilogue().into_iter().filter(|i| keep(i)).collect();
        if options.unoptimized && !helper {
            // Unscheduled, the saved registers come back before the LR reload.
            if let Some(position) = epilogue.iter().position(|i| matches!(i, Instruction::LoadWord { d: 0, a: 1, .. })) {
                let reload = epilogue.remove(position);
                let after = epilogue
                    .iter()
                    .rposition(|i| {
                        matches!(
                            i,
                            Instruction::LoadWord { a: 1, .. }
                                | Instruction::LoadFloatDouble { a: 1, .. }
                                | Instruction::PairedSingleQuantizedLoad { a: 1, .. }
                        )
                    })
                    .map_or(position, |last| last + 1);
                epilogue.insert(after, reload);
            }
        }
        let mut epilogue = wrap(epilogue);
        for instruction in &mut epilogue {
            // The restore helper's base stays at the call.
            if matches!(instruction.instruction, Instruction::AddImmediate { d: 11, a: 1, .. }) {
                instruction.flags.serialize = true;
            }
        }
        epilogue_length = epilogue.len();
        pcode.blocks[exit].instructions.extend(epilogue);
        framed_blocks[0] = true;
        framed_blocks[exit] = true;
    } else {
        pcode.blocks[exit]
            .instructions
            .push(PInstr::new(Instruction::BranchToLinkRegister));
    }
    // MWCC's "MERGING EPILOGUE, PROLOGUE": the return block joins a sole
    // fall-through predecessor, so final scheduling sees one block.
    if exit > 0 {
        let predecessors: Vec<usize> = (0..exit)
            .filter(|&block| pcode.blocks[block].successors.contains(&exit))
            .collect();
        if let [predecessor] = predecessors.as_slice() {
            if *predecessor == exit - 1 && !ends_in_branch(&pcode.blocks[exit - 1]) {
                let mut exit_block = pcode.blocks.pop().expect("exit block");
                pcode.blocks[exit - 1].instructions.append(&mut exit_block.instructions);
                pcode.blocks[exit - 1].successors.retain(|&successor| successor != exit);
                let exit_framed = framed_blocks.pop().expect("exit flag");
                framed_blocks[exit - 1] |= exit_framed;
            }
        }
    }
    // GC/3.x restores saved GPRs in order before `mtlr`, and before the LR
    // reload too when body code (not a call or branch) leads into the
    // epilogue.
    if epilogue_length > 0
        && !float_frame.generals.is_empty()
        && !helper
        && options.schedule
        && options.link_reload_after_float_restores
    {
        let instructions = &mut pcode.blocks.last_mut().expect("exit block").instructions;
        let start = instructions.len() - epilogue_length;
        let led = start > 0 && {
            let previous = &instructions[start - 1].instruction;
            !previous.is_call() && !matches!(previous, Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. })
        };
        // (Otherwise the LR reload leads them.)
        if !led {
            if let Some(reload) = instructions[start..]
                .iter()
                .position(|i| matches!(i.instruction, Instruction::LoadWord { d: 0, a: 1, .. }))
            {
                let reload = instructions.remove(start + reload);
                instructions.insert(start, reload);
            }
        }
        for instruction in instructions[start..].iter_mut() {
            match instruction.instruction {
                Instruction::LoadWord { d: 0, a: 1, .. } if led => instruction.flags.serialize = true,
                Instruction::LoadWord { a: 1, .. } => instruction.flags.in_order = true,
                Instruction::MoveToLinkRegister { .. } => instruction.flags.in_order = true,
                _ => {}
            }
        }
    }
    // Blocks that received frame code are rescheduled; the first pass's order
    // stands elsewhere.
    if options.schedule && !toggle("MWCC_PCODE_NO_FINAL_SCHEDULE") {
        for (block, contents) in pcode.blocks.iter_mut().enumerate() {
            if framed_blocks[block] || toggle("MWCC_PCODE_FINAL_SCHEDULE") {
                schedule::schedule_block(&mut contents.instructions, false);
            }
        }
    }
    let exit_block = pcode.blocks.len() - 1;
    let frameless_exit = !framed
        && !options.unoptimized
        && matches!(
            pcode.blocks[exit_block].instructions.as_slice(),
            [only] if matches!(only.instruction, Instruction::BranchToLinkRegister)
        );

    // Drop jumps to the next block and return inline from a frameless function.
    // (A target reaches the exit through empty blocks.)
    let exits = |pcode: &PCodeFunction, target: usize| {
        target <= exit_block && (target..exit_block).all(|b| pcode.blocks[b].instructions.is_empty())
    };
    for block in 0..pcode.blocks.len() {
        let last = pcode.blocks[block].instructions.last().map(|instruction| instruction.instruction.clone());
        if let Some(Instruction::BranchConditionalForward { options, condition_bit, target }) = last {
            // A conditional branch to a bare `blr` is a conditional return.
            if frameless_exit && options != 16 && exits(&pcode, target) && !toggle("MWCC_PCODE_EXIT_ONLY_DIRECT") {
                pcode.blocks[block].instructions.last_mut().expect("branch").instruction =
                    Instruction::BranchConditionalToLinkRegister { options, condition_bit };
            }
            continue;
        }
        let Some(Instruction::Branch { target }) = last else {
            continue;
        };
        if target > block && (block + 1..target).all(|b| pcode.blocks[b].instructions.is_empty()) {
            pcode.blocks[block].instructions.pop();
        } else if frameless_exit && target == exit_block {
            pcode.blocks[block].instructions.last_mut().expect("branch").instruction =
                Instruction::BranchToLinkRegister;
        }
    }
    // A conditional branch over a bare `blr` to the code right after it is
    // the opposite conditional return (`ble L; blr; L:` -> `bgtlr`).
    if frameless_exit && !toggle("MWCC_PCODE_NO_SKIPPED_RETURN") {
        for block in 0..pcode.blocks.len().saturating_sub(2) {
            let Some(Instruction::BranchConditionalForward { options, condition_bit, target }) =
                pcode.blocks[block].instructions.last().map(|instruction| instruction.instruction.clone())
            else {
                continue;
            };
            let inverted = match options {
                12 => 4,
                4 => 12,
                _ => continue,
            };
            let returns = matches!(pcode.blocks[block + 1].instructions.as_slice(),
                [only] if matches!(only.instruction, Instruction::BranchToLinkRegister));
            if returns && target == block + 2 {
                pcode.blocks[block].instructions.last_mut().expect("branch").instruction =
                    Instruction::BranchConditionalToLinkRegister { options: inverted, condition_bit };
                pcode.blocks[block + 1].instructions.clear();
            }
        }
    }
    let mut starts = Vec::with_capacity(pcode.blocks.len() + 1);
    let mut position = 0;
    for block in &pcode.blocks {
        starts.push(position);
        position += block.instructions.len();
    }
    starts.push(position);

    // Incoming stack arguments sit past this function's frame.
    let frame_size: i16 = if framed {
        prologue()
            .iter()
            .find_map(|instruction| match instruction {
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset } => Some(-offset),
                _ => None,
            })
            .unwrap_or(0)
    } else {
        0
    };
    for block in &mut pcode.blocks {
        for instruction in &mut block.instructions {
            if instruction.displacement_symbol.as_deref() == Some("@@incoming") {
                instruction.displacement_symbol = None;
                match &mut instruction.instruction {
                    Instruction::LoadWord { offset, .. }
                    | Instruction::LoadByteZero { offset, .. }
                    | Instruction::LoadHalfwordAlgebraic { offset, .. }
                    | Instruction::LoadHalfwordZero { offset, .. } => *offset += frame_size,
                    Instruction::AddImmediate { immediate, .. } => *immediate += frame_size,
                    _ => {}
                }
            }
        }
    }
    let mut instructions: Vec<Instruction> = Vec::new();
    let mut relocations: Vec<Relocation> = Vec::new();
    let mut deferred_displacements = Vec::new();
    for block in &pcode.blocks {
        for instruction in &block.instructions {
            if let Some(symbol) = &instruction.displacement_symbol {
                deferred_displacements.push(mwcc_machine_code::DeferredDisplacement {
                    instruction_index: instructions.len(),
                    target: match symbol.strip_prefix("@@const").and_then(|index| index.parse().ok()) {
                        Some(index) => mwcc_machine_code::DeferredDisplacementTarget::Constant(index),
                        None => mwcc_machine_code::DeferredDisplacementTarget::Symbol(symbol.clone()),
                    },
                });
            }
            if let Some(relocation) = &instruction.relocation {
                relocations.push(Relocation {
                    instruction_index: instructions.len(),
                    kind: relocation.kind,
                    target: relocation.target.clone(),
                });
            }
            let mut emitted = instruction.instruction.clone();
            match &mut emitted {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => {
                    *target = starts[*target];
                }
                _ => {}
            }
            instructions.push(emitted);
        }
    }
    // Anything still virtual is an internal error: refuse the function
    // rather than emit (or crash encoding) a register field >= 32.
    for block in &pcode.blocks {
        for instruction in &block.instructions {
            for class in [mwcc_pcode::Class::General, mwcc_pcode::Class::Float] {
                if instruction.uses(class).iter().chain(instruction.defs(class).iter()).any(|&r| r >= 32) {
                    return Err(mwcc_core::Diagnostic::error(format!(
                        "PCode internal: uncolored register in {:?} (not yet supported)",
                        instruction.instruction
                    )));
                }
            }
        }
    }
    let mut output = MachineFunction::new(pcode.name.clone());
    for &(bits, width) in &pcode.pool {
        output.intern_constant(bits, width);
    }
    output.instructions = instructions;
    output.relocations = relocations;
    output.deferred_displacements = deferred_displacements;
    output.string_literals = pcode.strings.clone();
    for (bytes, alignment) in &pcode.rodata_images {
        output.anonymous_rodata.push(mwcc_machine_code::AnonymousRodata {
            bytes: bytes.clone(),
            comment_alignment: *alignment,
            static_slot_prefix_bump: None,
            anonymous_offset: 0,
        });
    }
    for (targets, anonymous_offset) in &pcode.jump_tables {
        output.jump_tables.push(mwcc_machine_code::JumpTable {
            entries: targets.iter().map(|&block| (starts[block] * 4) as u32).collect(),
            anonymous_offset: *anonymous_offset,
        });
    }
    Ok(output)
}

/// A copy `mr v,rN` from a physical register (an incoming argument or a
/// call result) disappears before scheduling when every use of `v` can read
/// rN directly: all uses sit in the copy's block before rN or `v` is
/// redefined. Otherwise the copy and its uses stay as they are.
fn propagate_physical_copies(pcode: &mut PCodeFunction) -> bool {
    use mwcc_pcode::Class;
    use mwcc_vreg::RegisterRole;
    let mut changed = false;
    for class in [Class::General, Class::Float] {
        // `mr rN,v` right after `mr v,rN` (neither redefined) is redundant.
        for block in &mut pcode.blocks {
            let mut equal: Vec<(u32, u32)> = Vec::new();
            let mut redundant = vec![false; block.instructions.len()];
            for (index, instruction) in block.instructions.iter().enumerate() {
                if let Some((destination, source)) = instruction.copy(class) {
                    if destination < 32 && equal.contains(&(source, destination)) {
                        redundant[index] = true;
                        continue;
                    }
                }
                for defined in instruction.defs(class) {
                    equal.retain(|&(v, p)| v != defined && p != defined);
                }
                if instruction.instruction.is_call() {
                    equal.clear();
                }
                if let Some((v, p)) = instruction.copy(class) {
                    if v >= 32 && p < 32 && p != 0 {
                        equal.push((v, p));
                    }
                }
            }
            let mut index = 0;
            block.instructions.retain(|_| {
                index += 1;
                !redundant[index - 1]
            });
        }
        let mut total_uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for instruction in pcode.blocks.iter().flat_map(|block| block.instructions.iter()) {
            for used in instruction.uses(class) {
                *total_uses.entry(used).or_default() += 1;
            }
        }
        // Pass 1: which copies would lose every use.
        let mut reachable_uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        let mut candidates: Vec<(u32, u32)> = Vec::new();
        for block in &pcode.blocks {
            let mut active: Vec<(u32, u32)> = Vec::new();
            for instruction in &block.instructions {
                let written = instruction.defs(class);
                for used in instruction.uses(class) {
                    // (A use by an in-place update of `v` itself cannot
                    // read rN.)
                    if active.iter().any(|&(v, _)| v == used) && !written.contains(&used) {
                        *reachable_uses.entry(used).or_default() += 1;
                    }
                }
                for defined in instruction.defs(class) {
                    active.retain(|&(v, p)| v != defined && p != defined);
                }
                if instruction.instruction.is_call() {
                    active.clear();
                }
                if let Some((v, p)) = instruction.copy(class) {
                    if v >= 32 && p < 32 && p != 0 {
                        active.push((v, p));
                        candidates.push((v, p));
                    }
                }
            }
        }
        let eliminated: Vec<(u32, u32)> = candidates
            .into_iter()
            .filter(|(v, _)| {
                // One definition only, and every use reachable from it.
                let definitions = pcode
                    .blocks
                    .iter()
                    .flat_map(|block| block.instructions.iter())
                    .filter(|instruction| instruction.defs(class).contains(v))
                    .count();
                definitions == 1
                    && reachable_uses.get(v).copied().unwrap_or(0) == total_uses.get(v).copied().unwrap_or(0)
            })
            .collect();
        // Copies whose register never conflicts with rN (they coalesce with
        // it anyway): uses in the copy's block read rN; the copy stays for
        // uses elsewhere.
        let partial: Vec<(u32, u32)> = if toggle("MWCC_PCODE_NO_PARTIAL_COPYPROP") {
            Vec::new()
        } else {
            let mut seen = std::collections::HashSet::new();
            pcode
                .blocks
                .iter()
                .flat_map(|block| block.instructions.iter())
                .filter_map(|instruction| instruction.copy(class))
                .filter(|&(v, p)| v >= 32 && p < 32 && p != 0 && seen.insert(v))
                .filter(|copy| !eliminated.contains(copy))
                .filter(|&(v, p)| {
                    let definitions = pcode
                        .blocks
                        .iter()
                        .flat_map(|block| block.instructions.iter())
                        .filter(|instruction| instruction.defs(class).contains(&v))
                        .count();
                    definitions == 1 && !interferes_with_physical(pcode, class, v, p)
                })
                .collect()
        };
        if eliminated.is_empty() && partial.is_empty() {
            continue;
        }
        changed |= !eliminated.is_empty();
        // (A copy reading an eliminated entry copy is the entry copy now.)
        let entry_copies: Vec<u32> = pcode
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .filter(|instruction| instruction.flags.entry_copy)
            .filter_map(|instruction| instruction.copy(class))
            .filter(|copy| eliminated.contains(copy))
            .map(|(v, _)| v)
            .collect();
        // (Into a physical register: only an argument a later call reads.)
        let argument_copies: Vec<Vec<bool>> = pcode
            .blocks
            .iter()
            .map(|block| {
                block
                    .instructions
                    .iter()
                    .enumerate()
                    .map(|(index, instruction)| {
                        instruction.copy(class).is_some_and(|(destination, _)| {
                            destination < 32
                                && block.instructions[index + 1..]
                                    .iter()
                                    .take_while(|later| !later.defs(class).contains(&destination) || later.instruction.is_call())
                                    .any(|later| {
                                        later.instruction.is_call()
                                            && later.implicit_uses.iter().any(|r| r.class == class && r.number == destination)
                                    })
                        })
                    })
                    .collect()
            })
            .collect();
        // Pass 2: rewrite the uses and drop the copies.
        for (block_index, block) in pcode.blocks.iter_mut().enumerate() {
            let mut active: Vec<(u32, u32)> = Vec::new();
            for (instruction_index, instruction) in block.instructions.iter_mut().enumerate() {
                // Partially propagated copies leave other copies reading `v`.
                let skip_partial = instruction.copy(class).is_some();
                let written = instruction.defs(class);
                let active_here: Vec<(u32, u32)> = active
                    .iter()
                    .copied()
                    .filter(|copy| !(skip_partial && partial.contains(copy)))
                    .filter(|(v, _)| !written.contains(v))
                    .collect();
                let active = &mut active;
                if !active_here.is_empty() {
                    if let Some((destination, source)) = instruction.copy(class) {
                        if (destination >= 32 || argument_copies[block_index][instruction_index])
                            && entry_copies.contains(&source)
                            && active_here.iter().any(|&(v, _)| v == source)
                        {
                            instruction.flags.entry_copy = true;
                        }
                    }
                    let active = &active_here;
                    mwcc_vreg::for_each_register(&mut instruction.instruction, |role, operand_class, field| {
                        if role == RegisterRole::Use && operand_class == class {
                            if let Some(&(_, physical)) = active.iter().find(|&&(v, _)| v == *field) {
                                *field = physical;
                            }
                        }
                    });
                    for register in &mut instruction.implicit_uses {
                        if register.class == class {
                            if let Some(&(_, physical)) = active.iter().find(|&&(v, _)| v == register.number) {
                                register.number = physical;
                            }
                        }
                    }
                }
                for defined in instruction.defs(class) {
                    active.retain(|&(v, p)| v != defined && p != defined);
                }
                if instruction.instruction.is_call() {
                    active.clear();
                }
                if let Some(copy) = instruction.copy(class) {
                    if eliminated.contains(&copy) || partial.contains(&copy) {
                        active.push(copy);
                    }
                }
            }
            block.instructions.retain(|instruction| {
                !instruction.copy(class).is_some_and(|copy| eliminated.contains(&copy))
            });
        }
    }
    changed
}

/// A copy `mr v,rN` that stayed (v outlives rN): after scheduling, uses of
/// `v` in its block read rN until either is redefined. (Not a `v` used in
/// other blocks or across a call.)
/// `add x,a,b; l* d,0(x)` with `x` read only by the load: `l*x d,a,b`.
fn fold_indexed_loads(pcode: &mut PCodeFunction) {
    let general = mwcc_pcode::Class::General;
    let mut reads: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for instruction in pcode.blocks.iter().flat_map(|block| &block.instructions) {
        for register in instruction.uses(general) {
            *reads.entry(register).or_default() += 1;
        }
    }
    for block in &mut pcode.blocks {
        let mut index = 0;
        while index < block.instructions.len() {
            let Instruction::Add { d: sum, a, b } = block.instructions[index].instruction else {
                index += 1;
                continue;
            };
            if sum < 32 || a == 0 || reads.get(&sum) != Some(&1) {
                index += 1;
                continue;
            }
            let mut reader = None;
            for later in index + 1..block.instructions.len() {
                let instruction = &block.instructions[later];
                if instruction.uses(general).contains(&sum) {
                    reader = Some(later);
                    break;
                }
                if instruction.defs(general).iter().any(|&register| register == a || register == b) {
                    break;
                }
            }
            let Some(later) = reader else {
                index += 1;
                continue;
            };
            let load = &mut block.instructions[later];
            let indexed = match load.instruction {
                Instruction::LoadWord { d, a: base, offset: 0 } if base == sum => Some(Instruction::LoadWordIndexed { d, a, b }),
                Instruction::LoadByteZero { d, a: base, offset: 0 } if base == sum => Some(Instruction::LoadByteZeroIndexed { d, a, b }),
                Instruction::LoadHalfwordZero { d, a: base, offset: 0 } if base == sum => Some(Instruction::LoadHalfwordZeroIndexed { d, a, b }),
                Instruction::LoadHalfwordAlgebraic { d, a: base, offset: 0 } if base == sum => {
                    Some(Instruction::LoadHalfwordAlgebraicIndexed { d, a, b })
                }
                Instruction::LoadFloatSingle { d, a: base, offset: 0 } if base == sum => Some(Instruction::LoadFloatSingleIndexed { d, a, b }),
                Instruction::LoadFloatDouble { d, a: base, offset: 0 } if base == sum => Some(Instruction::LoadFloatDoubleIndexed { d, a, b }),
                _ => None,
            };
            match indexed {
                Some(instruction) if load.relocation.is_none() => {
                    load.instruction = instruction;
                    load.not_r0.push(a);
                    block.instructions.remove(index);
                }
                _ => index += 1,
            }
        }
    }
}

fn forward_physical_reads(pcode: &mut PCodeFunction) {
    use mwcc_pcode::Class;
    use mwcc_vreg::RegisterRole;
    for class in [Class::General, Class::Float] {
        let mut blocks_using: std::collections::HashMap<u32, std::collections::HashSet<usize>> = Default::default();
        for (index, block) in pcode.blocks.iter().enumerate() {
            for instruction in &block.instructions {
                for used in instruction.uses(class) {
                    blocks_using.entry(used).or_default().insert(index);
                }
            }
        }
        for (block_index, block) in pcode.blocks.iter_mut().enumerate() {
            let local = |v: u32, from: usize, instructions: &[mwcc_pcode::PInstr]| {
                if blocks_using.get(&v).is_some_and(|blocks| blocks.iter().any(|&b| b != block_index)) {
                    return false;
                }
                let last_use = instructions.iter().rposition(|instruction| instruction.uses(class).contains(&v));
                last_use.map_or(true, |last| !instructions[from..last].iter().any(|instruction| instruction.instruction.is_call()))
            };
            // A copy whose value stays block-local forwards every read; one
            // that lives on forwards only reads as a memory base.
            let eligible: Vec<Option<bool>> = (0..block.instructions.len())
                .map(|index| match block.instructions[index].copy(class) {
                    Some((v, p)) if v >= 32 && p < 32 && p != 0 => {
                        if local(v, index, &block.instructions) {
                            Some(false)
                        } else {
                            (!toggle("MWCC_PCODE_NO_BASE_FORWARD")).then_some(true)
                        }
                    }
                    _ => None,
                })
                .collect();
            let mut active: Vec<(u32, u32, bool)> = Vec::new();
            for (index, instruction) in block.instructions.iter_mut().enumerate() {
                if !active.is_empty() && instruction.copy(class).is_none() {
                    // (Not a register the instruction also writes: an
                    // in-place update such as `rlwimi`.)
                    let written = instruction.defs(class);
                    let memory_bases: Vec<u32> = if matches!(
                        instruction.instruction,
                        Instruction::AddImmediate { .. } | Instruction::AddImmediateShifted { .. }
                    ) {
                        Vec::new()
                    } else {
                        instruction.not_r0.clone()
                    };
                    let active: Vec<(u32, u32, bool)> = active
                        .iter()
                        .copied()
                        .filter(|(v, _, base_only)| !written.contains(v) && (!base_only || memory_bases.contains(v)))
                        .collect();
                    let active = &active;
                    mwcc_vreg::for_each_register(&mut instruction.instruction, |role, operand_class, field| {
                        if role == RegisterRole::Use && operand_class == class {
                            if let Some(&(_, physical, _)) = active.iter().find(|&&(v, _, _)| v == *field) {
                                *field = physical;
                            }
                        }
                    });
                }
                for defined in instruction.defs(class) {
                    active.retain(|&(v, p, _)| v != defined && p != defined);
                }
                if instruction.instruction.is_call() {
                    active.clear();
                }
                if let (Some((v, p)), Some(base_only)) = (instruction.copy(class), eligible[index]) {
                    active.push((v, p, base_only));
                }
            }
        }
    }
}

/// Whether virtual `v` is live where physical `p` is written (other than by
/// a copy of `v`), so the two cannot share a register.
fn interferes_with_physical(pcode: &PCodeFunction, class: mwcc_pcode::Class, v: u32, p: u32) -> bool {
    let blocks = pcode.blocks.len();
    let mut live_in = vec![false; blocks];
    let live_out = |live_in: &[bool], block: usize| {
        pcode.blocks[block].successors.iter().any(|&successor| live_in[successor])
    };
    let mut changed = true;
    while changed {
        changed = false;
        for block in (0..blocks).rev() {
            let mut live = live_out(&live_in, block);
            for instruction in pcode.blocks[block].instructions.iter().rev() {
                if instruction.defs(class).contains(&v) {
                    live = false;
                }
                if instruction.uses(class).contains(&v) {
                    live = true;
                }
            }
            if live && !live_in[block] {
                live_in[block] = true;
                changed = true;
            }
        }
    }
    for block in 0..blocks {
        let mut live = live_out(&live_in, block);
        for instruction in pcode.blocks[block].instructions.iter().rev() {
            let writes_p = instruction.defs(class).contains(&p)
                || class == mwcc_pcode::Class::General && instruction.instruction.is_call() && (3..=12).contains(&p);
            if live && writes_p && instruction.copy(class) != Some((p, v)) {
                return true;
            }
            if instruction.defs(class).contains(&v) {
                live = false;
            }
            if instruction.uses(class).contains(&v) {
                live = true;
            }
        }
    }
    false
}

/// Rename each live-range web (definitions joined by a common use) of a
/// multiply-defined virtual register to its own register, as MWCC's
/// allocator builds webs: a reassigned parameter's new value is a new web.
fn split_webs(pcode: &mut PCodeFunction) {
    use mwcc_pcode::Class;
    use mwcc_vreg::RegisterRole;
    let calls = pcode.blocks.iter().any(|block| block.instructions.iter().any(|instruction| instruction.instruction.is_call()))
        && !toggle("MWCC_PCODE_WEBS_SPLIT_INCOMING_UPDATES");
    for class in [Class::General, Class::Float] {
        // Definition sites (register, block, index) of registers defined more than once.
        let mut sites: Vec<(u32, usize, usize)> = Vec::new();
        for (b, block) in pcode.blocks.iter().enumerate() {
            for (i, instruction) in block.instructions.iter().enumerate() {
                for defined in instruction.defs(class) {
                    if defined >= 32 {
                        sites.push((defined, b, i));
                    }
                }
            }
        }
        let mut counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for &(register, _, _) in &sites {
            *counts.entry(register).or_default() += 1;
        }
        sites.retain(|(register, _, _)| counts[register] > 1);
        if sites.is_empty() {
            continue;
        }
        let count = sites.len();
        let blocks = pcode.blocks.len();
        let site_at = |register: u32, block: usize, index: usize| {
            sites.iter().position(|&(r, b, i)| r == register && b == block && i == index)
        };
        // Apply one instruction's definitions to a reaching-definitions state.
        let define = |state: &mut Vec<bool>, register: u32, site: usize| {
            for (other, &(r, _, _)) in sites.iter().enumerate() {
                if r == register {
                    state[other] = false;
                }
            }
            state[site] = true;
        };
        let mut entry: Vec<Vec<bool>> = vec![vec![false; count]; blocks];
        let mut changed = true;
        while changed {
            changed = false;
            for block in 0..blocks {
                let mut state = entry[block].clone();
                for (i, instruction) in pcode.blocks[block].instructions.iter().enumerate() {
                    for defined in instruction.defs(class) {
                        if let Some(site) = site_at(defined, block, i) {
                            define(&mut state, defined, site);
                        }
                    }
                }
                for &successor in &pcode.blocks[block].successors {
                    for site in 0..count {
                        if state[site] && !entry[successor][site] {
                            entry[successor][site] = true;
                            changed = true;
                        }
                    }
                }
            }
        }
        // Union the definitions that reach a common use.
        let mut parent: Vec<usize> = (0..count).collect();
        fn find(parent: &mut [usize], x: usize) -> usize {
            let mut root = x;
            while parent[root] != root {
                root = parent[root];
            }
            parent[x] = root;
            root
        }
        let mut uses: Vec<(usize, usize, u32, usize)> = Vec::new();
        for block in 0..blocks {
            let mut state = entry[block].clone();
            for (i, instruction) in pcode.blocks[block].instructions.iter().enumerate() {
                for used in instruction.uses(class) {
                    let reaching: Vec<usize> =
                        (0..count).filter(|&site| state[site] && sites[site].0 == used).collect();
                    if let Some(&first) = reaching.first() {
                        for &other in &reaching[1..] {
                            let (a, b) = (find(&mut parent, first), find(&mut parent, other));
                            parent[b] = a;
                        }
                        uses.push((block, i, used, first));
                    }
                }
                for defined in instruction.defs(class) {
                    if let Some(site) = site_at(defined, block, i) {
                        // Study switch: keep an in-place update (`add t,t,c`)
                        // in its web. MWCC splits `x = a + 1; x <<= 1` and
                        // `lis; addi`, but not every accumulator.
                        // `rlwimi` reads and writes one field: always one web.
                        let tied = match &instruction.instruction {
                            Instruction::RotateAndMaskInsert { .. } => true,
                            // An in-place extension continues its value's web.
                            Instruction::ExtendSignByte { a, s } | Instruction::ExtendSignHalfword { a, s } => a == s,
                            _ => instruction.flags.continues_web
                                || instruction.flags.in_place && instruction.uses(class).contains(&defined),
                        };
                        // An in-place update with another register operand
                        // (`add t,t,b`) keeps its accumulator; one with a
                        // constant (`addi t,t,1`, `slwi t,t,1`) splits.
                        // Only within a block: an update whose input comes
                        // from an earlier block starts a new web.
                        let uses = instruction.uses(class);
                        let local_input = (0..count).all(|other| {
                            !(state[other] && sites[other].0 == defined) || sites[other].1 == block
                        });
                        let register_update = uses.contains(&defined)
                            && uses.iter().any(|&used| used != defined)
                            && local_input
                            && !toggle("MWCC_PCODE_WEBS_NO_REGISTER_TIE");
                        if tied
                            || register_update
                            || toggle("MWCC_PCODE_WEBS_TIE_INPLACE")
                                && instruction.uses(class).contains(&defined)
                        {
                            for other in 0..count {
                                let (_, other_block, other_index) = sites[other];
                                // A parameter's incoming value is its own web
                                // (but, where calls keep it in a saved
                                // register, a register update continues it).
                                let incoming = pcode.blocks[other_block].instructions[other_index]
                                    .copy(class)
                                    .is_some_and(|(_, source)| source < 32);
                                if state[other] && sites[other].0 == defined && (tied || !incoming || (calls && register_update)) {
                                    let (a, b) = (find(&mut parent, site), find(&mut parent, other));
                                    parent[b] = a;
                                }
                            }
                        }
                        define(&mut state, defined, site);
                    }
                }
            }
        }
        // The web holding a register's first definition keeps its number.
        let mut web_register: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        for site in 0..count {
            let root = find(&mut parent, site);
            if web_register.contains_key(&root) {
                continue;
            }
            let register = sites[site].0;
            let first = (0..count).find(|&s| sites[s].0 == register).expect("a site");
            let first_root = find(&mut parent, first);
            let assigned = if first_root == root {
                register
            } else {
                let fresh = pcode.fresh(class);
                if register < pcode.coalesce_first(class) && !toggle("MWCC_PCODE_WEBS_JOIN_WINDOW") {
                    pcode.outside_window.push((class, fresh));
                }
                fresh
            };
            web_register.insert(root, assigned);
        }
        let renamed: Vec<u32> = (0..count).map(|site| web_register[&find(&mut parent, site)]).collect();
        for (site, &(register, b, i)) in sites.iter().enumerate() {
            if renamed[site] != register {
                let instruction = &mut pcode.blocks[b].instructions[i];
                mwcc_vreg::for_each_register(&mut instruction.instruction, |role, operand_class, field| {
                    if role == RegisterRole::Define && operand_class == class && *field == register {
                        *field = renamed[site];
                    }
                });
            }
        }
        for (b, i, register, site) in uses {
            let target = renamed[site];
            if target != register {
                let instruction = &mut pcode.blocks[b].instructions[i];
                mwcc_vreg::for_each_register(&mut instruction.instruction, |role, operand_class, field| {
                    if role == RegisterRole::Use && operand_class == class && *field == register {
                        *field = target;
                    }
                });
                for used in &mut instruction.implicit_uses {
                    if used.class == class && used.number == register {
                        used.number = target;
                    }
                }
                if class == Class::General {
                    for base in &mut instruction.not_r0 {
                        if *base == register {
                            *base = target;
                        }
                    }
                }
            }
        }
    }
}

/// MWCC's peephole: `addi rX,rB,sym@l` followed by a zero-displacement
/// access through rX becomes the access with `sym@l(rB)` when rX is not used
/// again — unless the access is a load into rX itself (before GC/3.0).
fn fold_absolute_displacements(pcode: &mut PCodeFunction, into_own_base: bool) {
    use mwcc_pcode::Class;
    let live_out = general_live_out(pcode);
    for (number, block) in pcode.blocks.iter_mut().enumerate() {
        let mut index = 0;
        while index + 1 < block.instructions.len() {
            let (address, base, relocation) = match (&block.instructions[index].instruction, &block.instructions[index].relocation) {
                (Instruction::AddImmediate { d, a, immediate: 0 }, Some(relocation))
                    if relocation.kind == RelocationKind::Addr16Lo && *a != 0 =>
                {
                    (*d, *a, relocation.clone())
                }
                _ => {
                    index += 1;
                    continue;
                }
            };
            // The address's next use, with neither register redefined before
            // it (the peephole runs before final scheduling).
            let next = (index + 1..block.instructions.len()).find(|&at| {
                let instruction = &block.instructions[at];
                instruction.uses(Class::General).contains(&address)
                    || instruction.defs(Class::General).contains(&address)
                    || instruction.defs(Class::General).contains(&base)
            });
            let Some(at) = next.filter(|&at| {
                at == index + 1 || !toggle("MWCC_PCODE_ADJACENT_ABSOLUTE_FOLD")
            }) else {
                index += 1;
                continue;
            };
            let access = &block.instructions[at];
            // (Before GC/3.0 a load folds only an `addi` that kept its
            // source register: `addi r,r,sym@l`.)
            let loads = access.defs(Class::General).len() + access.defs(Class::Float).len() > 0
                && !matches!(access.instruction, Instruction::StoreWordWithUpdate { .. });
            let late_stores = toggle("MWCC_PCODE_LATE_STORE_FOLD");
            if (loads || late_stores) && !into_own_base && address != base && !toggle("MWCC_PCODE_FOLD_ANY_ADDI") {
                index += 1;
                continue;
            }
            // (A section-anchor access keeps its own displacement.)
            if access.displacement_symbol.is_some()
                || !access.uses(Class::General).contains(&address)
                || (at != index + 1 && access.defs(Class::General).contains(&base) && !access.uses(Class::General).contains(&base))
            {
                index += 1;
                continue;
            }
            // (Read again before being redefined, or live out; not when the
            // access itself redefines it.)
            let later_use = !(access.defs(Class::General).contains(&address) && !toggle("MWCC_PCODE_FOLD_REDEFINED_ADDRESS_USE")) && {
                let mut read = None;
                for instruction in &block.instructions[at + 1..] {
                    if instruction.uses(Class::General).contains(&address) {
                        read = Some(true);
                        break;
                    }
                    if instruction.defs(Class::General).contains(&address) {
                        read = Some(false);
                        break;
                    }
                }
                read.unwrap_or(live_out[number] & (1 << address) != 0)
            };
            let folded = match access.instruction.clone() {
                Instruction::LoadWord { d, a, offset: 0 } if a == address && (d != address || into_own_base) => {
                    Some(Instruction::LoadWord { d, a: base, offset: 0 })
                }
                Instruction::LoadHalfwordZero { d, a, offset: 0 } if a == address && (d != address || into_own_base) => {
                    Some(Instruction::LoadHalfwordZero { d, a: base, offset: 0 })
                }
                Instruction::LoadHalfwordAlgebraic { d, a, offset: 0 } if a == address && (d != address || into_own_base) => {
                    Some(Instruction::LoadHalfwordAlgebraic { d, a: base, offset: 0 })
                }
                Instruction::LoadByteZero { d, a, offset: 0 } if a == address && (d != address || into_own_base) => {
                    Some(Instruction::LoadByteZero { d, a: base, offset: 0 })
                }
                Instruction::LoadFloatSingle { d, a, offset: 0 } if a == address => {
                    Some(Instruction::LoadFloatSingle { d, a: base, offset: 0 })
                }
                Instruction::LoadFloatDouble { d, a, offset: 0 } if a == address => {
                    Some(Instruction::LoadFloatDouble { d, a: base, offset: 0 })
                }
                Instruction::StoreWord { s, a, offset: 0 } if a == address && s != address => {
                    Some(Instruction::StoreWord { s, a: base, offset: 0 })
                }
                Instruction::StoreHalfword { s, a, offset: 0 } if a == address && s != address => {
                    Some(Instruction::StoreHalfword { s, a: base, offset: 0 })
                }
                Instruction::StoreByte { s, a, offset: 0 } if a == address && s != address => {
                    Some(Instruction::StoreByte { s, a: base, offset: 0 })
                }
                _ => None,
            };
            // With the address still needed (GC <= 2.7): a store at offset 0
            // through the address register itself takes the update form,
            // which leaves the full address behind (`lis r; stwu s,@l(r)`).
            let updated = match access.instruction.clone() {
                _ if into_own_base || address != base || toggle("MWCC_PCODE_NO_UPDATE_STORES") => None,
                Instruction::StoreWord { s, a, offset: 0 } if a == address && s != address => {
                    Some(Instruction::StoreWordWithUpdate { s, a, offset: 0 })
                }
                Instruction::StoreHalfword { s, a, offset: 0 } if a == address && s != address => {
                    Some(Instruction::StoreHalfwordWithUpdate { s, a, offset: 0 })
                }
                Instruction::StoreByte { s, a, offset: 0 } if a == address && s != address => {
                    Some(Instruction::StoreByteWithUpdate { s, a, offset: 0 })
                }
                Instruction::StoreFloatSingle { s, a, offset: 0 } if a == address => {
                    Some(Instruction::StoreFloatSingleWithUpdate { s, a, offset: 0 })
                }
                Instruction::StoreFloatDouble { s, a, offset: 0 } if a == address => {
                    Some(Instruction::StoreFloatDoubleWithUpdate { s, a, offset: 0 })
                }
                // (A word load too: `lis r; lwzu d,@l(r)`.)
                Instruction::LoadWord { d, a, offset: 0 } if a == address && d != address && !toggle("MWCC_PCODE_NO_UPDATE_FIRST_LOADS") => {
                    Some(Instruction::LoadWordWithUpdate { d, a, offset: 0 })
                }
                _ => None,
            };
            match (folded, updated) {
                (_, Some(instruction)) if later_use && (at == index + 1 || !toggle("MWCC_PCODE_ADJACENT_UPDATE_STORES")) => {
                    block.instructions[at].instruction = instruction;
                    block.instructions[at].relocation = Some(relocation);
                    block.instructions.remove(index);
                }
                (Some(instruction), _) if !later_use => {
                    block.instructions[at].instruction = instruction;
                    block.instructions[at].relocation = Some(relocation);
                    block.instructions.remove(index);
                }
                _ => index += 1,
            }
        }
    }
}

/// `addi vA,vB,sym@l` whose one use is a store through vA at offset 0: the
/// store takes `sym@l(vB)` (on virtual registers, before coloring).
fn fold_absolute_stores(pcode: &mut PCodeFunction) {
    use mwcc_pcode::Class;
    let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for block in &pcode.blocks {
        for instruction in &block.instructions {
            for register in instruction.uses(Class::General) {
                *uses.entry(register).or_default() += 1;
            }
        }
    }
    for block in &mut pcode.blocks {
        let mut index = 0;
        while index < block.instructions.len() {
            let (address, base, relocation) = match (&block.instructions[index].instruction, &block.instructions[index].relocation) {
                (Instruction::AddImmediate { d, a, immediate: 0 }, Some(relocation))
                    if relocation.kind == RelocationKind::Addr16Lo && *a != 0 && *d >= 32 && uses.get(d) == Some(&1) =>
                {
                    (*d, *a, relocation.clone())
                }
                _ => {
                    index += 1;
                    continue;
                }
            };
            let next = (index + 1..block.instructions.len()).find(|&at| {
                let instruction = &block.instructions[at];
                instruction.uses(Class::General).contains(&address) || instruction.defs(Class::General).contains(&base)
            });
            let folded = next.and_then(|at| {
                let instruction = match block.instructions[at].instruction.clone() {
                    Instruction::StoreWord { s, a, offset: 0 } if a == address && s != address => Instruction::StoreWord { s, a: base, offset: 0 },
                    Instruction::StoreHalfword { s, a, offset: 0 } if a == address && s != address => Instruction::StoreHalfword { s, a: base, offset: 0 },
                    Instruction::StoreByte { s, a, offset: 0 } if a == address && s != address => Instruction::StoreByte { s, a: base, offset: 0 },
                    Instruction::StoreFloatSingle { s, a, offset: 0 } if a == address => Instruction::StoreFloatSingle { s, a: base, offset: 0 },
                    Instruction::StoreFloatDouble { s, a, offset: 0 } if a == address => Instruction::StoreFloatDouble { s, a: base, offset: 0 },
                    _ => return None,
                };
                (block.instructions[at].displacement_symbol.is_none()).then_some((at, instruction))
            });
            match folded {
                Some((at, instruction)) => {
                    block.instructions[at].instruction = instruction;
                    block.instructions[at].relocation = Some(relocation);
                    block.instructions[at].not_r0.retain(|&register| register != address);
                    block.instructions[at].not_r0.push(base);
                    block.instructions.remove(index);
                }
                None => index += 1,
            }
        }
    }
}

/// Physical GPRs live out of each block (bit per register).
fn general_live_out(pcode: &PCodeFunction) -> Vec<u32> {
    use mwcc_pcode::Class;
    let bits = |registers: Vec<u32>| registers.into_iter().filter(|&r| r < 32).fold(0u32, |mask, r| mask | 1 << r);
    // Per block: registers read before written, and registers written.
    let summary: Vec<(u32, u32)> = pcode
        .blocks
        .iter()
        .map(|block| {
            let (mut used, mut defined) = (0u32, 0u32);
            for instruction in &block.instructions {
                used |= bits(instruction.uses(Class::General)) & !defined;
                defined |= bits(instruction.defs(Class::General));
            }
            (used, defined)
        })
        .collect();
    let mut live_in = vec![0u32; pcode.blocks.len()];
    let mut live_out = vec![0u32; pcode.blocks.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for block in (0..pcode.blocks.len()).rev() {
            let successors = &pcode.blocks[block].successors;
            // The function's exit reads its result (r3, r3:r4) and the
            // registers live everywhere.
            let result = match pcode.returns {
                mwcc_pcode::ReturnRegisters::General => 1 << 3,
                mwcc_pcode::ReturnRegisters::GeneralPair => (1 << 3) | (1 << 4),
                mwcc_pcode::ReturnRegisters::None | mwcc_pcode::ReturnRegisters::Float => 0,
            };
            let exit = if successors.is_empty() { result | bits(pcode.exit_uses.clone()) } else { 0 };
            let out = successors.iter().fold(exit, |mask, &s| mask | live_in[s]);
            let inside = summary[block].0 | (out & !summary[block].1);
            if out != live_out[block] || inside != live_in[block] {
                live_out[block] = out;
                live_in[block] = inside;
                changed = true;
            }
        }
    }
    live_out
}

fn ends_in_branch(block: &Block) -> bool {
    matches!(
        block.instructions.last().map(|instruction| &instruction.instruction),
        Some(Instruction::Branch { .. })
    )
}

/// Remove blocks unreachable from the entry, renumbering successors and
/// branch targets. The exit block is always kept last.
fn prune_unreachable(pcode: &mut PCodeFunction) {
    let count = pcode.blocks.len();
    let exit = count - 1;
    let mut reachable = vec![false; count];
    let mut stack = vec![0];
    while let Some(block) = stack.pop() {
        if std::mem::replace(&mut reachable[block], true) {
            continue;
        }
        stack.extend(pcode.blocks[block].successors.iter().copied());
    }
    reachable[exit] = true;
    if reachable.iter().all(|&kept| kept) {
        return;
    }
    let mut renumber = vec![usize::MAX; count];
    let mut next = 0;
    for block in 0..count {
        if reachable[block] {
            renumber[block] = next;
            next += 1;
        }
    }
    for (targets, _) in &mut pcode.jump_tables {
        for target in targets.iter_mut() {
            *target = renumber[*target];
        }
    }
    let blocks = std::mem::take(&mut pcode.blocks);
    for (index, mut block) in blocks.into_iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        block.successors = block.successors.iter().map(|&successor| renumber[successor]).collect();
        for instruction in &mut block.instructions {
            match &mut instruction.instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => {
                    *target = renumber[*target];
                }
                _ => {}
            }
        }
        pcode.blocks.push(block);
    }
}

/// `MWCC_PCODE_DUMP=<function>` prints PCode at each stage boundary, labeled
/// like MWCC's own dumps.
fn dump(pcode: &PCodeFunction, stage: &str) {
    if std::env::var("MWCC_PCODE_DUMP").is_ok_and(|name| name == pcode.name || name == "1") {
        eprintln!("== {} {stage}", pcode.name);
        for (index, block) in pcode.blocks.iter().enumerate() {
            eprintln!("  block {index} -> {:?}", block.successors);
            for instruction in &block.instructions {
                eprintln!("    {:?}", instruction.instruction);
            }
        }
    }
}

/// Model-study switches (not part of the compiler's behavior).
pub(crate) fn toggle(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// A frame with saved FPRs: the linkage area, the local region, then from
/// the top of the frame each saved FPR (16 bytes with its paired-single
/// half: `psq_st` above `stfd`; 8 bytes otherwise), then the saved GPRs.
struct FloatFrame {
    /// The GC/1.0-1.2.5n prologue and epilogue shapes.
    early: bool,
    /// GC/1.1p1's LR reload after the pop.
    link_reload_after_pop: bool,
    link_reload_last: bool,
    /// The GPRs go through `_savegpr_N`/`_restgpr_N` (r11 = top of their area).
    helper: bool,
    /// ... or through `stmw`/`lmw` (`-use_lmw_stmw on`).
    multiple: bool,
    frame_size: i16,
    floats: Vec<(u32, i16, Option<i16>)>,
    generals: Vec<(u32, i16)>,
}

impl FloatFrame {
    fn new(floats: &[u32], paired: bool, generals: &[u32], local_bytes: i16) -> FloatFrame {
        let per_float: i16 = if paired { 16 } else { 8 };
        let float_area = per_float * floats.len() as i16;
        let frame_size = ((8 + local_bytes + float_area + 4 * generals.len() as i16 + 15) / 16) * 16;
        let floats = floats
            .iter()
            .enumerate()
            .map(|(k, &register)| {
                let top = frame_size - per_float * k as i16;
                if paired {
                    (register, top - 16, Some(top - 8))
                } else {
                    (register, top - 8, None)
                }
            })
            .collect();
        let generals = generals
            .iter()
            .enumerate()
            .map(|(k, &register)| (register, frame_size - float_area - 4 * (k as i16 + 1)))
            .collect();
        FloatFrame {
            early: false,
            link_reload_after_pop: false,
            link_reload_last: false,
            helper: false,
            multiple: false,
            frame_size,
            floats,
            generals,
        }
    }

    /// GC/1.0-1.2.5n: the locals end 8-aligned, then the save area (FPRs
    /// at the top, GPRs below) rounded to 8.
    fn early(floats: &[u32], generals: &[u32], local_bytes: i16) -> FloatFrame {
        let base = (8 + local_bytes + 7) / 8 * 8;
        let float_area = 8 * floats.len() as i16;
        let frame_size = base + (float_area + 4 * generals.len() as i16 + 7) / 8 * 8;
        let floats = floats.iter().enumerate().map(|(k, &register)| (register, frame_size - 8 * (k as i16 + 1), None)).collect();
        let generals = generals
            .iter()
            .enumerate()
            .map(|(k, &register)| (register, frame_size - float_area - 4 * (k as i16 + 1)))
            .collect();
        FloatFrame {
            early: true,
            link_reload_after_pop: false,
            link_reload_last: false,
            helper: false,
            multiple: true,
            frame_size,
            floats,
            generals,
        }
    }

    fn prologue(&self) -> Vec<Instruction> {
        let mut instructions = if self.early {
            vec![
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -self.frame_size },
            ]
        } else {
            vec![
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -self.frame_size },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: self.frame_size + 4 },
            ]
        };
        for &(register, double, paired) in &self.floats {
            instructions.push(Instruction::StoreFloatDouble { s: register, a: 1, offset: double });
            if let Some(offset) = paired {
                instructions.push(Instruction::PairedSingleQuantizedStore { s: register, a: 1, offset, w: 0, i: 0 });
            }
        }
        if self.helper && self.multiple {
            let (lowest, offset) = *self.generals.last().expect("saved registers");
            instructions.push(Instruction::StoreMultipleWord { s: lowest, a: 1, offset });
            return instructions;
        }
        if self.helper {
            instructions.extend(self.helper_call("_savegpr"));
            return instructions;
        }
        for &(register, offset) in &self.generals {
            instructions.push(Instruction::StoreWord { s: register, a: 1, offset });
        }
        instructions
    }

    /// `addi r11,r1,top; bl name_N`.
    fn helper_call(&self, name: &str) -> [Instruction; 2] {
        let (lowest, offset) = *self.generals.last().expect("saved registers");
        [
            Instruction::AddImmediate { d: 11, a: 1, immediate: offset + 4 * self.generals.len() as i16 },
            Instruction::BranchAndLink { target: format!("{name}_{lowest}") },
        ]
    }

    /// Each FPR restores its paired half then its double, except that the
    /// final double reload follows the LR reload.
    fn epilogue(&self) -> Vec<Instruction> {
        if self.early {
            // `lwz r0; lfd…; lwz…/lmw; addi r1; mtlr; blr` (GC/1.1p1:
            // `…; addi r1; lwz r0,4(r1); mtlr; blr`).
            let mut instructions = if self.link_reload_after_pop {
                Vec::new()
            } else {
                vec![Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 }]
            };
            for &(register, double, _) in &self.floats {
                instructions.push(Instruction::LoadFloatDouble { d: register, a: 1, offset: double });
            }
            if self.helper {
                let (lowest, offset) = *self.generals.last().expect("saved registers");
                instructions.push(Instruction::LoadMultipleWord { d: lowest, a: 1, offset });
            } else {
                for &(register, offset) in &self.generals {
                    instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
                }
            }
            instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
            if self.link_reload_after_pop {
                instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 4 });
            }
            instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            instructions.push(Instruction::BranchToLinkRegister);
            return instructions;
        }
        let mut instructions = Vec::new();
        let last = self.floats.len().wrapping_sub(1);
        for (k, &(register, double, paired)) in self.floats.iter().enumerate() {
            if let Some(offset) = paired {
                instructions.push(Instruction::PairedSingleQuantizedLoad { d: register, a: 1, offset, w: 0, i: 0 });
            }
            if k == last && !self.link_reload_last && !self.helper {
                instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            }
            instructions.push(Instruction::LoadFloatDouble { d: register, a: 1, offset: double });
        }
        if self.helper && self.multiple {
            let (lowest, offset) = *self.generals.last().expect("saved registers");
            instructions.push(Instruction::LoadMultipleWord { d: lowest, a: 1, offset });
            instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        } else if self.helper {
            // The helper call clobbers LR: the reload follows it.
            instructions.extend(self.helper_call("_restgpr"));
            instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        } else {
            // (GC/3.x reloads LR after every saved register.)
            if !self.link_reload_last && self.floats.is_empty() {
                instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            }
            for &(register, offset) in &self.generals {
                instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
            if self.link_reload_last {
                instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            }
        }
        instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        instructions.push(Instruction::BranchToLinkRegister);
        instructions
    }
}

/// GC/1.0-1.2.5n: a pointer loaded from memory and used only to read and
/// then rewrite one field (`gp->b = gp->b | x`) reads it with the update
/// form, and the store follows at 0: `lhzu r3,2(r4); ...; sth r0,0(r4)`.
fn update_loads(pcode: &mut PCodeFunction) {
    use mwcc_pcode::Class;
    let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    let mut defs: std::collections::HashMap<u32, Vec<bool>> = std::collections::HashMap::new();
    for block in &pcode.blocks {
        for instruction in &block.instructions {
            for register in instruction.uses(Class::General) {
                *uses.entry(register).or_default() += 1;
            }
            for register in instruction.defs(Class::General) {
                // (A pointer read from a global: `lwz B,gp`.)
                defs.entry(register).or_default().push(
                    matches!(instruction.instruction, Instruction::LoadWord { .. })
                        && (instruction.relocation.is_some() || instruction.displacement_symbol.is_some()),
                );
            }
        }
    }
    let first_virtual = 32;
    for block in &mut pcode.blocks {
        let count = block.instructions.len();
        for x in 0..count {
            let (d, base, offset) = match block.instructions[x].instruction {
                Instruction::LoadWord { d, a, offset }
                | Instruction::LoadHalfwordZero { d, a, offset }
                | Instruction::LoadByteZero { d, a, offset } => (d, a, offset),
                _ => continue,
            };
            if base < first_virtual
                || d == base
                || uses.get(&base) != Some(&2)
                || defs.get(&base).map(Vec::as_slice) != Some(&[true][..])
                || pcode.exit_uses.contains(&base)
            {
                continue;
            }
            let width = |instruction: &Instruction| match instruction {
                Instruction::LoadWord { .. } | Instruction::StoreWord { .. } => 4,
                Instruction::LoadHalfwordZero { .. } | Instruction::StoreHalfword { .. } => 2,
                _ => 1,
            };
            let Some(y) = (x + 1..count).find(|&y| block.instructions[y].uses(Class::General).contains(&base)) else { continue };
            let same = match block.instructions[y].instruction {
                Instruction::StoreWord { a, offset: o, .. }
                | Instruction::StoreHalfword { a, offset: o, .. }
                | Instruction::StoreByte { a, offset: o, .. } => a == base && o == offset,
                _ => false,
            };
            if !same
                || block.instructions[y].flags.compound
                || width(&block.instructions[x].instruction) != width(&block.instructions[y].instruction)
                || block.instructions[x].relocation.is_some()
                || block.instructions[x].displacement_symbol.is_some()
                || block.instructions[y].displacement_symbol.is_some()
                || block.instructions[y].uses(Class::General).iter().filter(|&&r| r == base).count() != 1
            {
                continue;
            }
            let updated = match &block.instructions[x].instruction {
                Instruction::LoadWord { d, a, offset } => Instruction::LoadWordWithUpdate { d: *d, a: *a, offset: *offset },
                Instruction::LoadHalfwordZero { d, a, offset } => Instruction::LoadHalfZeroWithUpdate { d: *d, a: *a, offset: *offset },
                Instruction::LoadByteZero { d, a, offset } => Instruction::LoadByteZeroWithUpdate { d: *d, a: *a, offset: *offset },
                _ => continue,
            };
            block.instructions[x].instruction = updated;
            match &mut block.instructions[y].instruction {
                Instruction::StoreWord { offset, .. } | Instruction::StoreHalfword { offset, .. } | Instruction::StoreByte { offset, .. } => *offset = 0,
                _ => {}
            }
        }
    }
}

/// `mr rD,rS` (or an early `addi rD,rS,0`) followed by `cmpwi rD,0`, with
/// nothing between that touches rD or cr0, becomes `mr. rD,rS`.
/// (GC/3.x instead tests the copy's source: `cmpwi rS,0`.)
fn move_records(pcode: &mut PCodeFunction, record: bool) {
    use mwcc_pcode::Class;
    for block in &mut pcode.blocks {
        let mut index = 0;
        while index < block.instructions.len() {
            // (A logical compare with 0 too when only equality is tested:
            // the block's branch reads cr0's EQ bit alone.)
            let equality_only = || {
                let rest = &block.instructions[index + 1..];
                let reads_cr = rest.iter().filter(|i| format!("{:?}", i.instruction).contains("condition_bit") || matches!(i.instruction, Instruction::MoveFromConditionRegister { .. })).count();
                reads_cr == 1
                    && matches!(rest.last().map(|i| &i.instruction), Some(Instruction::BranchConditionalForward { condition_bit: 2, .. } | Instruction::BranchConditionalToLinkRegister { condition_bit: 2, .. }))
                    && !toggle("MWCC_PCODE_NO_LOGICAL_MOVE_RECORDS")
            };
            let tested = match block.instructions[index].instruction {
                Instruction::CompareWordImmediate { a, immediate: 0 } => a,
                Instruction::CompareLogicalWordImmediate { a, immediate: 0 } if equality_only() => a,
                _ => {
                    index += 1;
                    continue;
                }
            };
            let mut at = index;
            let mut found = None;
            while at > 0 {
                at -= 1;
                let candidate = &block.instructions[at];
                if candidate.defs(Class::General).contains(&tested) {
                    found = match candidate.instruction {
                        Instruction::Or { a, s, b } if a == tested && s == b && s != a => Some((a, s)),
                        Instruction::AddImmediate { d, a, immediate: 0 } if d == tested && a != 0 && a != d => Some((d, a)),
                        _ => None,
                    };
                    break;
                }
                let name = format!("{:?}", candidate.instruction);
                if name.contains("Record") || name.starts_with("Compare") || candidate.instruction.is_call() || name.starts_with("Branch") {
                    break;
                }
            }
            if let Some((d, s)) = found {
                if !record {
                    // (Only a plain copy whose source still holds the value.)
                    let unchanged = (at + 1..index).all(|k| !block.instructions[k].defs(Class::General).contains(&s));
                    if matches!(block.instructions[at].instruction, Instruction::Or { .. }) && unchanged {
                        block.instructions[index].instruction = match block.instructions[index].instruction {
                            Instruction::CompareLogicalWordImmediate { .. } => Instruction::CompareLogicalWordImmediate { a: s, immediate: 0 },
                            _ => Instruction::CompareWordImmediate { a: s, immediate: 0 },
                        };
                    }
                    index += 1;
                    continue;
                }
                block.instructions[at].instruction = Instruction::OrRecord { a: d, s, b: s };
                block.instructions.remove(index);
                continue;
            }
            index += 1;
        }
    }
}

/// A copy into a register that already holds the value (`mr r31,r3; ...;
/// mr r3,r31` with neither changed) is dropped, also into a fall-through
/// successor reached only from the copy's block.
fn remove_redundant_copies(pcode: &mut PCodeFunction) {
    use mwcc_pcode::Class;
    let count = pcode.blocks.len();
    let mut predecessors = vec![0usize; count];
    for block in &pcode.blocks {
        for &successor in &block.successors {
            predecessors[successor] += 1;
        }
    }
    let mut equal: Vec<(u32, u32)> = Vec::new();
    for index in 0..count {
        let continues = index > 0 && predecessors[index] == 1 && pcode.blocks[index - 1].successors.contains(&index);
        if !continues {
            equal.clear();
        }
        let block = &mut pcode.blocks[index];
        let mut keep = vec![true; block.instructions.len()];
        for (position, instruction) in block.instructions.iter().enumerate() {
            if let Instruction::Or { a, s, b } | Instruction::OrRecord { a, s, b } = instruction.instruction {
                let record = matches!(instruction.instruction, Instruction::OrRecord { .. });
                if s == b && a != s {
                    if !record && equal.iter().any(|&(x, y)| (x, y) == (a, s) || (x, y) == (s, a)) {
                        keep[position] = false;
                        continue;
                    }
                    equal.retain(|&(x, y)| x != a && y != a);
                    equal.push((a, s));
                    continue;
                }
            }
            if instruction.instruction.is_call() {
                // (A call changes the volatile registers.)
                let volatile = |r: u32| r == 0 || (3..=12).contains(&r);
                equal.retain(|&(x, y)| !volatile(x) && !volatile(y));
            }
            for register in instruction.defs(Class::General) {
                equal.retain(|&(x, y)| x != register && y != register);
            }
        }
        let mut position = 0;
        block.instructions.retain(|_| {
            let kept = keep[position];
            position += 1;
            kept
        });
    }
}

/// GC/1.0-1.2.5n: copies `mr d,s` whose value reaches a call with two or more
/// word arguments (before any other call) take the `addi d,s,0` form.
fn argument_addi_copies(pcode: &mut PCodeFunction) {
    use mwcc_pcode::Class;
    for block in &mut pcode.blocks {
        let mut converted = Vec::new();
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Or { a: d, s, b } = instruction.instruction else { continue };
            // (`addi d,0,0` would be `li d,0`: a copy out of r0 stays `mr`.)
            if s != b || d == s || s == 0 {
                continue;
            }
            // The next call, with neither register redefined before it.
            let mut carriers = vec![d, s];
            let mut variadic = false;
            let mut feeds = false;
            for (offset, later) in block.instructions[index + 1..].iter().enumerate() {
                if matches!(later.instruction, Instruction::ConditionRegisterClear { .. } | Instruction::ConditionRegisterSet { .. }) {
                    variadic = true;
                }
                if later.instruction.is_call() {
                    let arguments: Vec<u32> = later
                        .implicit_uses
                        .iter()
                        .filter(|register| register.class == Class::General)
                        .map(|register| register.number)
                        .collect();
                    if arguments.len() < 2 {
                        break;
                    }
                    // (An argument still holding an earlier call's result:
                    // the copies are `mr`.)
                    let call_result = {
                        let before = &block.instructions[..index + 1 + offset];
                        let definer = before.iter().rev().find(|i| i.defs(Class::General).contains(&3) || i.instruction.is_call());
                        arguments.contains(&3) && definer.is_some_and(|i| i.instruction.is_call())
                    };
                    // (The variadic marker anywhere since the previous call.)
                    let at = index + 1 + offset;
                    let start = block.instructions[..at].iter().rposition(|i| i.instruction.is_call()).map_or(0, |p| p + 1);
                    variadic |= block.instructions[start..at]
                        .iter()
                        .any(|i| matches!(i.instruction, Instruction::ConditionRegisterClear { .. } | Instruction::ConditionRegisterSet { .. }));
                    // (An argument computed into its register — not a copy
                    // nor a constant — keeps the copies `mr`.)
                    let computed = arguments.iter().any(|&register| {
                        block.instructions[start..at].iter().rev().find(|i| i.defs(Class::General).contains(&register)).is_some_and(|i| {
                            !matches!(
                                i.instruction,
                                Instruction::Or { s, b, .. } if s == b
                            ) && !matches!(i.instruction, Instruction::AddImmediate { a: 0, .. } | Instruction::AddImmediateShifted { a: 0, .. })
                                && !(matches!(i.instruction, Instruction::AddImmediate { .. }) && i.relocation.is_some())
                        })
                    }) || later.implicit_uses.iter().filter(|register| register.class == Class::Float).any(|register| {
                        // (A floating argument read from memory other than a
                        // constant pool.)
                        block.instructions[start..at].iter().rev().find(|i| i.defs(Class::Float).contains(&register.number)).is_some_and(|i| {
                            !matches!(i.instruction, Instruction::FloatMove { .. }) && i.relocation.is_none()
                        })
                    });
                    let computed = computed && !toggle("MWCC_PCODE_ADDI_COPIES_WITH_COMPUTED");
                    let into_argument = arguments.contains(&d) && carriers.contains(&d);
                    feeds = !computed
                        && if into_argument {
                            variadic || (s >= 14 && !call_result)
                        } else {
                            carriers.iter().any(|register| arguments.contains(register))
                        };
                    break;
                }
                // (A copy of the value into another register carries it.)
                if let Instruction::Or { a: copy, s: from, b } = later.instruction {
                    if from == b && carriers.contains(&from) {
                        carriers.push(copy);
                        continue;
                    }
                }
                let defs = later.defs(Class::General);
                carriers.retain(|register| !defs.contains(register));
                if carriers.is_empty() {
                    break;
                }
            }
            if feeds {
                converted.push(index);
            }
        }
        for index in converted {
            if let Instruction::Or { a, s, .. } = block.instructions[index].instruction {
                block.instructions[index].instruction = Instruction::AddImmediate { d: a, a: s, immediate: 0 };
            }
        }
    }
}
