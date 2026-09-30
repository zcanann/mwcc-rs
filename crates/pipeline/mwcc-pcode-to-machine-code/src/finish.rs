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
    /// Remove dead definitions while building interference (MWCC's
    /// `gDeleteDeadInstructions`).
    pub delete_dead: bool,
    /// Schedule for two integer units (IU2 takes simple integer operations):
    /// the machine model after GC/1.2.5, whose single-IU default model the
    /// decompilation documents.
    pub two_integer_units: bool,
    /// `-O0`: no copy propagation or live-range splitting before coloring.
    pub unoptimized: bool,
    /// Fold `addi rX,rB,sym@l` into a following zero-displacement access
    /// even when the load's destination is rX (GC/3.0+).
    pub fold_absolute_into_own_base: bool,
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
    schedule::FRAME_OBJECTS.with(|objects| *objects.borrow_mut() = pcode.frame_objects.clone());
    schedule::PRIVATE_FRAME_OBJECTS.with(|objects| *objects.borrow_mut() = pcode.private_frame_objects.clone());
    dump(&pcode, "INITIAL CODE");
    if !options.unoptimized && !toggle("MWCC_PCODE_NO_WEBS") {
        split_webs(&mut pcode);
    }
    if !options.unoptimized && !toggle("MWCC_PCODE_NO_COPYPROP") {
        // Eliminating one copy can expose another (`x = a` of a parameter).
        while propagate_physical_copies(&mut pcode) {}
    }
    if options.schedule && !toggle("MWCC_PCODE_NO_PRESCHEDULE") {
        for block in &mut pcode.blocks {
            schedule::schedule_block(&mut block.instructions, true);
        }
    }
    dump(&pcode, "AFTER INSTRUCTION SCHEDULING");
    let colors = coloring::color(&mut pcode, options.delete_dead)?;
    dump(&pcode, "AFTER REGISTER COLORING");
    if !options.unoptimized {
        fold_absolute_displacements(&mut pcode, options.fold_absolute_into_own_base);
    }
    if !colors.saved_float.is_empty() {
        return Err(mwcc_core::Diagnostic::error(
            "PCode frame: saved float registers are not yet supported",
        ));
    }
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
    let framed = makes_calls || !saved.is_empty() || pcode.frame_local_bytes > 0;
    let plan = mwcc_vreg::FramePlan::with_local_region(saved, pcode.frame_local_bytes);
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
        instructions.into_iter().map(PInstr::new).collect()
    };
    let exit = pcode.blocks.len() - 1;
    let mut framed_blocks = vec![false; pcode.blocks.len()];
    // Code that never returns (an endless loop) has no epilogue.
    let exit_reached = exit == 0 || (0..exit).any(|block| pcode.blocks[block].successors.contains(&exit));
    if framed && !exit_reached {
        let mut entry = wrap(plan.prologue().into_iter().filter(|i| keep(i)).collect());
        entry.append(&mut pcode.blocks[0].instructions);
        pcode.blocks[0].instructions = entry;
        framed_blocks[0] = true;
    } else if !exit_reached {
    } else if framed {
        let mut entry = wrap(plan.prologue().into_iter().filter(|i| keep(i)).collect());
        entry.append(&mut pcode.blocks[0].instructions);
        pcode.blocks[0].instructions = entry;
        let mut epilogue: Vec<Instruction> = plan.epilogue().into_iter().filter(|i| keep(i)).collect();
        if options.unoptimized {
            // Unscheduled, the saved registers come back before the LR reload.
            if let Some(position) = epilogue.iter().position(|i| matches!(i, Instruction::LoadWord { d: 0, a: 1, .. })) {
                let reload = epilogue.remove(position);
                let after = epilogue
                    .iter()
                    .rposition(|i| matches!(i, Instruction::LoadWord { a: 1, .. }))
                    .map_or(position, |last| last + 1);
                epilogue.insert(after, reload);
            }
        }
        pcode.blocks[exit].instructions.extend(wrap(epilogue));
        framed_blocks[0] = true;
        framed_blocks[exit] = true;
    } else if !pcode.ends_in_tail_call {
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
    for block in 0..pcode.blocks.len() {
        let last = pcode.blocks[block].instructions.last().map(|instruction| instruction.instruction.clone());
        if let Some(Instruction::BranchConditionalForward { options, condition_bit, target }) = last {
            // A conditional branch to a bare `blr` is a conditional return.
            if frameless_exit && target == exit_block {
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
    let mut starts = Vec::with_capacity(pcode.blocks.len() + 1);
    let mut position = 0;
    for block in &pcode.blocks {
        starts.push(position);
        position += block.instructions.len();
    }
    starts.push(position);

    let mut instructions: Vec<Instruction> = Vec::new();
    let mut relocations: Vec<Relocation> = Vec::new();
    for block in &pcode.blocks {
        for instruction in &block.instructions {
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
    output.string_literals = pcode.strings.clone();
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
                for used in instruction.uses(class) {
                    if active.iter().any(|&(v, _)| v == used) {
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
        // Pass 2: rewrite the uses and drop the copies.
        for block in &mut pcode.blocks {
            let mut active: Vec<(u32, u32)> = Vec::new();
            for instruction in &mut block.instructions {
                // Partially propagated copies leave other copies reading `v`.
                let skip_partial = instruction.copy(class).is_some();
                let active_here: Vec<(u32, u32)> = active
                    .iter()
                    .copied()
                    .filter(|copy| !(skip_partial && partial.contains(copy)))
                    .collect();
                let active = &mut active;
                if !active_here.is_empty() {
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
                            _ => instruction.flags.in_place && instruction.uses(class).contains(&defined),
                        };
                        if tied
                            || toggle("MWCC_PCODE_WEBS_TIE_INPLACE")
                                && instruction.uses(class).contains(&defined)
                        {
                            for other in 0..count {
                                let (_, other_block, other_index) = sites[other];
                                // A parameter's incoming value is its own web.
                                let incoming = pcode.blocks[other_block].instructions[other_index]
                                    .copy(class)
                                    .is_some_and(|(_, source)| source < 32);
                                if state[other] && sites[other].0 == defined && (tied || !incoming) {
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
    for block in &mut pcode.blocks {
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
            let access = &block.instructions[index + 1];
            let later_use = block.instructions[index + 2..].iter().any(|i| i.uses(Class::General).contains(&address))
                || pcode_live_out(block, address);
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
            match folded {
                Some(instruction) if !later_use => {
                    block.instructions[index + 1].instruction = instruction;
                    block.instructions[index + 1].relocation = Some(relocation);
                    block.instructions.remove(index);
                }
                _ => index += 1,
            }
        }
    }
}

/// Conservative: a register the block's successors might read.
fn pcode_live_out(block: &Block, register: u32) -> bool {
    // Absolute addresses are block-local temporaries; a physical register
    // 3 may carry a result out (the exit's return value).
    !block.successors.is_empty() && register == 3
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
