//! INITIAL CODE: instruction selection from the typed IRO representation to
//! PCode over virtual registers, as MWCC's `CodeGen_Generator` does before
//! any backend pass.
//!
//! The IR arrives normalized (folded, selects rewritten, idioms recognized);
//! this crate only chooses instructions. Virtual-register numbering follows
//! MWCC's object-preallocation walk (`CodeGen_PreallocateObjectRegisters`):
//! parameters, then declared locals in reverse declaration order, then the
//! coalescing window opens and temporaries follow in creation order.

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_iro::{
    is_float, is_general_word, is_narrow, is_unsigned, is_unsigned_narrow, promote, width, BinaryOp, Expr, ExprKind,
    Function, GlobalInfo, Idiom, IntrinsicOp, Place, Stmt, Type, UnaryOp, Unit, VarId, VariableKind,
};
use mwcc_machine_code::{Instruction, RelocationKind, RelocationTarget};
use mwcc_pcode::{AttachedRelocation, Block, Class, PCodeFunction, PInstr, Register, ReturnRegisters};

mod wide;
use wide::{is_wide, long_constant};

/// A lowered function.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub pcode: PCodeFunction,
    /// The function calls another (it needs a frame and the saved LR).
    pub makes_calls: bool,
}

const FIRST_GENERAL_ARGUMENT: u32 = 3;
const VOLATILE_GENERAL: [u32; 11] = [0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

fn unsupported(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("PCode lowering: {} (not yet supported)", what.into()))
}

/// Lower `function` to PCode. `returns_through_variable`: every return
/// assigns one return variable copied to r3 at the exit (MWCC's single
/// return point) — the source returns from more than its final value.
pub fn lower(
    function: &Function,
    returns_through_variable: bool,
    unit: &Unit<'_>,
    unoptimized: bool,
    tail_calls: bool,
    contract: bool,
) -> Compilation<Lowered> {
    let returns = match function.return_type {
        Type::Void => ReturnRegisters::None,
        ty if is_general_word(ty) => ReturnRegisters::General,
        ty if is_wide(ty) => ReturnRegisters::GeneralPair,
        Type::Float | Type::Double => ReturnRegisters::Float,
        other => return Err(unsupported(format!("return type {other:?}"))),
    };
    let mut lowerer = Lowerer {
        unit,
        function,
        pcode: PCodeFunction { strings: function.strings.clone(), ..PCodeFunction::new(function.name.clone(), returns) },
        registers: vec![None; function.variables.len()],
        registers_hi: vec![None; function.variables.len()],
        raw_narrow: function.variables.iter().map(|variable| variable.raw && is_narrow(variable.ty)).collect(),
        makes_calls: false,
        target: None,
        loaded_globals: HashMap::new(),
        restorable: Vec::new(),
        restore_violation: false,
        address_reuse: None,
        snapshots: HashMap::new(),
        restored_labels: Vec::new(),
        pending_branches: Vec::new(),
        pending_tables: Vec::new(),
        labels: Vec::new(),
        exit_label: Label(0),
        return_register: None,
        extended: HashMap::new(),
        constants: HashMap::new(),
        unoptimized,
        memory_base: 0,
        addressing: false,
        returned: 0,
        testing: 0,
        returning: false,
        homes: vec![None; function.variables.len()],
        loops: Vec::new(),
        known_constant: Vec::new(),
        tail_calls,
        all_tail_calls: false,
        common: HashMap::new(),
        contract,
        float_constants: HashMap::new(),
        loop_constants: Vec::new(),
        compound_store: false,
        address_bases: Vec::new(),
        frame_offsets: vec![None; function.variables.len()],
        anchored: anchored_objects(function, unit),
        constants_anchored: !unit.pool_small_data
            && !unit.unoptimized
            && pool_constant_count(&function.body) >= 3
            && !toggle("MWCC_PCODE_NO_RODATA_ANCHOR"),
        store_displacement: None,
        inserted: None,
        conversion_base: None,
        conversion_block: None,
        conversion_count: 0,
        rounding_slots: Vec::new(),
        frame_cursor: 8,
        reserved_end: 0,
        escaped_frame_objects: Vec::new(),
        escaping: escaping_frame_objects(&function.body, function.variables.len()),
        source_labels: HashMap::new(),
    };
    lowerer.lower_function(returns_through_variable)?;
    Ok(Lowered { pcode: lowerer.pcode, makes_calls: lowerer.makes_calls })
}

/// A branch target placed later in layout order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Label(usize);

struct Lowerer<'a, 'u> {
    unit: &'a Unit<'u>,
    function: &'a Function,
    pcode: PCodeFunction,
    /// Virtual register of each variable (temporaries on first assignment).
    registers: Vec<Option<u32>>,
    /// The high words of wide (`long long`) variables.
    registers_hi: Vec<Option<u32>>,
    /// A narrow parameter held as it arrived: each block re-extends it at
    /// its first use (MWCC does not extend once on entry).
    raw_narrow: Vec<bool>,
    makes_calls: bool,
    /// Destination requested for the next expression's final operation.
    target: Option<u32>,
    /// Loaded values of non-volatile globals still valid (IRO common
    /// subexpressions); cleared by stores, calls and branches.
    loaded_globals: HashMap<String, (u32, Type)>,
    /// Labels whose block may take the known values of a sole incoming
    /// branch (not loop heads or switch targets).
    restorable: Vec<bool>,
    /// Known values at each forward conditional branch, by label.
    snapshots: HashMap<usize, Known>,
    /// Labels placed with a branch's known values (no later branch may
    /// target them).
    restored_labels: Vec<usize>,
    restore_violation: bool,
    /// -O0 compound assignment: the place's address, reused by the value's
    /// load of the same place (keyed by the place).
    address_reuse: Option<(String, (u32, i16, Option<u32>, Option<AttachedRelocation>))>,
    /// Branches awaiting their target block: (block, instruction, label).
    pending_branches: Vec<(usize, usize, Label)>,
    /// Jump tables awaiting their labels' blocks: (dispatch block, entries).
    pending_tables: Vec<(usize, Vec<Label>, u32)>,
    /// Label targets once placed: label -> block.
    labels: Vec<Option<usize>>,
    exit_label: Label,
    /// The return variable when returns go through one.
    return_register: Option<u32>,
    /// Extensions of raw narrow parameters made in the current block.
    extended: HashMap<VarId, u32>,
    /// Constants materialized in the current block (reused, as IRO's CSE).
    constants: HashMap<i64, u32>,
    /// `-O0`: locals are register variables (r31 down), no CSE, and a
    /// parameter used at most once in a calling function lives in the frame.
    unoptimized: bool,
    /// The memory access base being lowered (by address): its sum computes
    /// the base before the index.
    memory_base: usize,
    /// The binary operation being lowered is a memory access base.
    addressing: bool,
    /// The integer return value being lowered (by address).
    returned: usize,
    /// Lowering a branch condition (its values are tested).
    testing: usize,
    /// The binary operation being lowered is the return value.
    returning: bool,
    /// Frame home (`offset(r1)`) of a parameter kept in memory.
    homes: Vec<Option<i16>>,
    /// Enclosing loops' (break, continue) labels.
    loops: Vec<(Label, Label)>,
    /// The variable the previous statement set to a constant (IRO knows a
    /// loop's first test from it).
    known_constant: Vec<(VarId, i64)>,
    /// A body that is one terminal call becomes a sibling branch.
    tail_calls: bool,
    /// Every call is in tail position (GC/3.x): each becomes a branch.
    all_tail_calls: bool,
    /// Values of simple operations computed in this block (IRO common
    /// subexpressions): key -> (register, type, variables read).
    common: HashMap<String, (u32, Type, Vec<VarId>)>,
    /// `-fp_contract on`: multiply-add fuses.
    contract: bool,
    /// Floating constants loaded in this block: (bits, width) -> register.
    float_constants: HashMap<(u64, u8), u32>,
    /// Constants loaded before the enclosing loops (integer and floating),
    /// available throughout them.
    loop_constants: Vec<(Vec<(i64, u32)>, Vec<((u64, u8), u32)>)>,
    /// Lowering the store of a compound update.
    compound_store: bool,
    /// Constants loaded as the high half of a constant memory address.
    address_bases: Vec<i64>,
    /// r1 offset of each frame variable.
    frame_offsets: Vec<Option<i16>>,
    /// Objects addressed through their section anchor in this function.
    anchored: HashMap<String, &'static str>,
    /// Pool constants (outside small data) address through `...rodata.0`:
    /// the function uses three or more.
    constants_anchored: bool,
    /// The anchored object a store's displacement completes with.
    store_displacement: Option<String>,
    /// An insert's value already computed (by address): -O0 evaluates a
    /// bit-field store's value before the unit's address.
    inserted: Option<(usize, u32)>,
    /// -O0 conversion slots: the area's start, and the block and count of
    /// the slots used in it.
    conversion_base: Option<u32>,
    conversion_block: Option<usize>,
    conversion_count: u32,
    /// Optimized GC/1.3-on float-to-integer slots, reused block by block.
    rounding_slots: Vec<i16>,
    /// Next free byte of the local area (r1-relative).
    frame_cursor: u32,
    /// The end of an early frame's reservation when it holds no objects.
    reserved_end: u32,
    /// Frame objects whose address the code computes.
    escaped_frame_objects: Vec<i16>,
    /// Variables whose address is used other than to load or store
    /// directly (by variable).
    escaping: Vec<bool>,
    /// Source labels (`goto` targets) by name.
    source_labels: HashMap<String, Label>,
}

impl Lowerer<'_, '_> {
    // ------------------------------------------------------------ blocks

    fn new_label(&mut self) -> Label {
        self.labels.push(None);
        self.restorable.push(true);
        Label(self.labels.len() - 1)
    }

    /// A source label (`goto` target): a join point.
    fn named_label(&mut self, name: &str) -> Label {
        if let Some(&label) = self.source_labels.get(name) {
            return label;
        }
        let label = self.new_join_label();
        self.source_labels.insert(name.to_owned(), label);
        label
    }

    /// A label whose block never inherits a branch's known values.
    fn new_join_label(&mut self) -> Label {
        let label = self.new_label();
        self.restorable[label.0] = false;
        label
    }

    fn known(&self) -> Known {
        Known {
            loaded_globals: self.loaded_globals.clone(),
            extended: self.extended.clone(),
            constants: self.constants.clone(),
            common: self.common.clone(),
            float_constants: self.float_constants.clone(),
        }
    }

    fn current_block(&self) -> usize {
        self.pcode.blocks.len() - 1
    }

    /// Start a new block in layout order; `falls_through` links the previous
    /// block to it.
    fn start_block(&mut self, falls_through: bool) -> usize {
        self.clear_block_caches();
        self.start_block_keeping(falls_through)
    }

    /// Values computed so far are available only where this block's code
    /// dominates: a fall-through successor with no other predecessor.
    /// After a store or call: loaded globals may have changed, except
    /// `const` ones (across calls only on some builds).
    fn forget_loaded_globals(&mut self, call: bool) {
        let globals = self.unit.globals;
        let keep_const = (!call || self.unit.const_globals_across_calls) && !toggle("MWCC_PCODE_NO_CONST_GLOBALS");
        self.loaded_globals.retain(|name, _| keep_const && globals.get(name).is_some_and(|global| global.is_const));
    }

    /// Known frame-object loads (after a store the key cannot place).
    fn forget_frame_loads(&mut self) {
        self.common.retain(|key, _| !key.starts_with("*F"));
    }

    fn clear_block_caches(&mut self) {
        // Loaded globals too: at a join a value loaded on one path only is
        // not available.
        self.loaded_globals.clear();
        self.extended.clear();
        self.constants.clear();
        self.common.clear();
        self.float_constants.clear();
        // (Except the constants loaded before an enclosing loop.)
        for (constants, float_constants) in &self.loop_constants {
            self.constants.extend(constants.iter().copied());
            self.float_constants.extend(float_constants.iter().copied());
        }
    }

    /// The floating constants a call-free loop uses, and the integer
    /// conversion's constants, are loaded before it (loop code motion).
    fn preload_loop_constants(&mut self, parts: &[&[Stmt]], condition: Option<&Expr>) -> Compilation<bool> {
        if self.unoptimized || toggle("MWCC_PCODE_NO_LOOP_CONSTANTS") || parts.iter().any(|part| makes_calls(part)) {
            return Ok(false);
        }
        let mut floats: Vec<(f64, Type)> = Vec::new();
        let mut conversions: Vec<bool> = Vec::new();
        let mut visit = |e: &Expr| {
            fn walk(e: &Expr, floats: &mut Vec<(f64, Type)>, conversions: &mut Vec<bool>) {
                match &e.kind {
                    ExprKind::Float(value) => {
                        if !floats.iter().any(|&(v, t)| v.to_bits() == value.to_bits() && t == e.ty) {
                            floats.push((*value, e.ty));
                        }
                    }
                    ExprKind::Convert(operand)
                        if is_float(e.ty)
                            && matches!(operand.ty, Type::Int | Type::UnsignedInt | Type::Char | Type::Short | Type::UnsignedChar | Type::UnsignedShort) =>
                    {
                        let signed = matches!(operand.ty, Type::Int | Type::Char | Type::Short);
                        if !conversions.contains(&signed) {
                            conversions.push(signed);
                        }
                    }
                    _ => {}
                }
                for child in expression_children(e) {
                    walk(child, floats, conversions);
                }
            }
            walk(e, &mut floats, &mut conversions);
        };
        // (In the order the loop's code uses them: the body, then the test.)
        for part in parts {
            for_each_statement_expression(part, &mut visit);
        }
        if let Some(condition) = condition {
            visit(condition);
        }
        if floats.is_empty() && conversions.is_empty() {
            return Ok(false);
        }
        let mut constants = Vec::new();
        let mut float_constants = Vec::new();
        for &signed in &conversions {
            let (high, _) = self.expression(&Expr::int(0x4330_0000))?;
            if !constants.iter().any(|&(value, _)| value == 0x4330_0000) {
                constants.push((0x4330_0000, high));
            }
            let magic = if signed { 4503601774854144.0 } else { 4503599627370496.0 };
            let (bias, _) = self.float_constant(magic, Type::Double, None)?;
            float_constants.push(((f64::to_bits(magic), 8u8), bias));
        }
        for (value, ty) in floats {
            if !matches!(ty, Type::Float | Type::Double) {
                continue;
            }
            let (register, _) = self.float_constant(value, ty, None)?;
            let key = if ty == Type::Float { (u64::from((value as f32).to_bits()), 4u8) } else { (value.to_bits(), 8u8) };
            float_constants.push((key, register));
        }
        self.loop_constants.push((constants, float_constants));
        Ok(true)
    }

    fn start_block_keeping(&mut self, falls_through: bool) -> usize {
        let previous = self.current_block();
        self.pcode.blocks.push(Block { weight: 1, ..Block::default() });
        let block = self.current_block();
        if falls_through {
            self.pcode.blocks[previous].successors.push(block);
        }
        block
    }

    /// Place `label` at a fresh block (or the current one if it is empty).
    fn place_label(&mut self, label: Label) {
        // A label is a join: nothing computed before it is known here,
        // unless its only way in is one conditional branch (the values
        // known there dominate it).
        let current = self.current_block();
        let incoming = self.pending_branches.iter().filter(|&&(_, _, pending)| pending == label).count();
        let entered = if self.pcode.blocks[current].instructions.is_empty() {
            (0..current).any(|block| self.pcode.blocks[block].successors.contains(&current))
        } else {
            !self.block_ends_in_jump(current)
        };
        let inherited = (incoming == 1 && !entered && self.restorable[label.0] && !toggle("MWCC_PCODE_NO_DOMINATOR_CSE"))
            .then(|| self.snapshots.remove(&label.0))
            .flatten();
        // (Reached only by falling through: what is known stays known.)
        let fall_through_only = incoming == 0
            && entered
            && self.restorable[label.0]
            && !self.unoptimized
            && !toggle("MWCC_PCODE_NO_FALLTHROUGH_CSE");
        let kept = fall_through_only.then(|| self.known());
        self.clear_block_caches();
        let block = if self.pcode.blocks[current].instructions.is_empty() && !self.block_is_target(current) {
            current
        } else {
            let falls_through = !self.block_ends_in_jump(current);
            self.start_block(falls_through)
        };
        self.labels[label.0] = Some(block);
        if let Some(known) = inherited {
            self.loaded_globals = known.loaded_globals;
            self.extended = known.extended;
            self.constants = known.constants;
            self.common = known.common;
            self.float_constants = known.float_constants;
            self.restored_labels.push(label.0);
        } else if let Some(known) = kept {
            self.loaded_globals = known.loaded_globals;
            self.extended = known.extended;
            self.constants = known.constants;
            self.common = known.common;
            self.float_constants = known.float_constants;
        }
    }

    /// Place `label` only when a branch already targets it; otherwise the
    /// code simply continues in the current block.
    fn place_if_targeted(&mut self, label: Label) {
        if self.pending_branches.iter().any(|&(_, _, pending)| pending == label) {
            self.place_label(label);
        }
    }

    fn block_is_target(&self, block: usize) -> bool {
        self.labels.iter().any(|placed| *placed == Some(block))
    }

    fn block_ends_in_jump(&self, block: usize) -> bool {
        matches!(
            self.pcode.blocks[block].instructions.last().map(|i| &i.instruction),
            Some(Instruction::Branch { .. })
        )
    }

    /// Emit a branch to `label`; every branch ends its block.
    fn branch(&mut self, instruction: Instruction, label: Label) {
        let conditional = !matches!(instruction, Instruction::Branch { .. });
        if self.restored_labels.contains(&label.0) {
            // A later branch into a block that assumed one way in.
            self.restore_violation = true;
        }
        if conditional && !self.unoptimized && self.labels[label.0].is_none() {
            let known = self.known();
            self.snapshots.insert(label.0, known);
        }
        let block = self.current_block();
        let position = self.pcode.blocks[block].instructions.len();
        self.emit_plain(instruction);
        self.pending_branches.push((block, position, label));
        if conditional && !self.unoptimized {
            // The fall-through block continues the extended block.
            self.start_block_keeping(true);
        } else {
            self.start_block(conditional);
        }
    }

    fn jump(&mut self, label: Label) {
        self.branch(Instruction::Branch { target: 0 }, label);
    }

    /// Resolve pending branches: targets become block indices and edges.
    fn resolve_branches(&mut self) -> Compilation<()> {
        for (block, position, label) in std::mem::take(&mut self.pending_branches) {
            let target = self.labels[label.0].ok_or_else(|| unsupported("unplaced label"))?;
            match &mut self.pcode.blocks[block].instructions[position].instruction {
                Instruction::Branch { target: field } | Instruction::BranchConditionalForward { target: field, .. } => {
                    *field = target
                }
                _ => unreachable!("pending branches are branches"),
            }
            if !self.pcode.blocks[block].successors.contains(&target) {
                self.pcode.blocks[block].successors.push(target);
            }
        }
        for (block, entries, offset) in std::mem::take(&mut self.pending_tables) {
            let mut targets = Vec::new();
            for label in entries {
                let target = self.labels[label.0].ok_or_else(|| unsupported("unplaced label"))?;
                if !self.pcode.blocks[block].successors.contains(&target) {
                    self.pcode.blocks[block].successors.push(target);
                }
                targets.push(target);
            }
            self.pcode.jump_tables.push((targets, offset));
        }
        Ok(())
    }

    // ------------------------------------------------------------ emission

    fn emit(&mut self, instruction: PInstr) {
        let block = self.pcode.blocks.len() - 1;
        self.pcode.blocks[block].instructions.push(instruction);
    }

    fn emit_plain(&mut self, instruction: Instruction) {
        self.emit(PInstr::new(instruction));
    }

    /// An instruction whose `base` operand may not be r0.
    fn emit_based(&mut self, instruction: Instruction, base: u32) {
        let mut instruction = PInstr::new(instruction);
        instruction.not_r0.push(base);
        self.emit(instruction);
    }

    fn temporary(&mut self) -> u32 {
        self.pcode.fresh(Class::General)
    }

    fn result(&mut self, target: Option<u32>) -> u32 {
        target.unwrap_or_else(|| self.temporary())
    }

    /// The register of a variable (a temporary's on first use).
    fn register(&mut self, variable: VarId) -> u32 {
        if let Some(register) = self.registers[variable] {
            return register;
        }
        let register = self.fresh(self.function.variables[variable].ty);
        self.registers[variable] = Some(register);
        register
    }

    /// A new virtual register of the class that holds `ty`.
    fn fresh(&mut self, ty: Type) -> u32 {
        self.pcode.fresh(if is_float(ty) { Class::Float } else { Class::General })
    }

    fn result_for(&mut self, ty: Type, target: Option<u32>) -> u32 {
        target.unwrap_or_else(|| self.fresh(ty))
    }

    /// A register copy in `ty`'s class.
    fn copy(&mut self, ty: Type, d: u32, s: u32) {
        self.emit_plain(if is_float(ty) { Instruction::FloatMove { d, b: s } } else { Instruction::Or { a: d, s, b: s } });
    }

    // ------------------------------------------------------------ function

    fn lower_function(&mut self, returns_through_variable: bool) -> Compilation<()> {
        let function = self.function;
        // Object preallocation: parameters, then locals in reverse
        // declaration order, then the return variable; then the window opens.
        let mut incoming = Vec::new();
        let calls = makes_calls(&function.body);
        if self.unoptimized {
            return self.lower_unoptimized(calls);
        }
        let (mut general_argument, mut float_argument) = (FIRST_GENERAL_ARGUMENT, 1);
        let mut wide_incoming = Vec::new();
        let mut stack_incoming: Vec<(u32, Type, i16)> = Vec::new();
        // (The outgoing argument area comes first.)
        self.frame_cursor += outgoing_argument_bytes(&function.body);
        // (A variadic definition saves r3-r10 and f1-f8 at r1+8.)
        if self.unit.variadic {
            if outgoing_argument_bytes(&function.body) > 0 {
                return Err(unsupported("a variadic definition with stack arguments"));
            }
            self.frame_cursor = 8 + 96;
            self.pcode.frame_objects.push((8, 104));
        }
        for id in 0..function.parameter_count {
            let ty = function.variables[id].ty;
            if is_wide(ty) {
                if general_argument % 2 == 0 {
                    general_argument += 1;
                }
                let (high, low) = self.wide_registers(id);
                wide_incoming.push((high, general_argument));
                wide_incoming.push((low, general_argument + 1));
                general_argument += 2;
                continue;
            }
            let register = self.fresh(ty);
            self.registers[id] = Some(register);
            self.raw_narrow[id] = is_narrow(ty)
                && !assigns(&function.body, id)
                && !(self.unit.narrow_parameters_extended && ty != Type::Char);
            let physical = if is_float(ty) {
                float_argument += 1;
                float_argument - 1
            } else {
                general_argument += 1;
                general_argument - 1
            };
            // (Past r10, the caller's argument area.)
            if !is_float(ty) && physical > LAST_GENERAL_ARGUMENT {
                stack_incoming.push((register, ty, 8 + 4 * (physical - LAST_GENERAL_ARGUMENT - 1) as i16));
                continue;
            }
            incoming.push((id, register, physical));
        }
        let locals: Vec<VarId> = (function.parameter_count..function.variables.len())
            .filter(|&id| function.variables[id].kind == VariableKind::Local)
            .collect();
        let early = self.unit.early_frame;
        // (Early frames: each register parameter's slot, in order, after
        // the outgoing argument area.)
        // (A parameter whose address is taken: its home is that slot.)
        let mut parameter_slots: HashMap<String, u32> = HashMap::new();
        if early {
            let mut words = 0;
            for id in 0..function.parameter_count {
                if !is_float(function.variables[id].ty) {
                    words += 1;
                    if words > 8 {
                        continue;
                    }
                }
                let (size, align) = function.variables[id].frame.unwrap_or_else(|| {
                    let width = width(function.variables[id].ty);
                    (width, width)
                });
                let offset = self.frame_cursor.div_ceil(align.max(1)) * align.max(1);
                if let Some(name) = function.variables[id].name.strip_suffix("$in") {
                    parameter_slots.insert(name.to_owned(), offset);
                }
                self.frame_cursor = offset + size;
            }
        }
        // Frame objects take slots by size class (the size rounded up to a
        // power of two), smallest first; within a class, in reverse
        // declaration order. (Early frames: reverse declaration order.)
        let mut ordered: Vec<VarId> = locals.iter().rev().copied().collect();
        if !early && !toggle("MWCC_PCODE_FRAME_DECLARATION_ORDER") {
            ordered.sort_by_key(|&id| function.variables[id].frame.map_or(0, |(size, _)| size.max(1).next_power_of_two()));
        }
        for id in ordered {
            match function.variables[id].frame {
                // (Early frames: a local with no register keeps a slot: one
                // never read, or set only to a constant.)
                None if early
                    && ((function.variables[id].initialized
                        && assignments(&function.body, id) <= 1)
                        || references(&function.body, id) == assignments(&function.body, id)
                        // (Or set once and read once: propagated into its use.)
                        || (assignments(&function.body, id) == 1
                            && references(&function.body, id) == 2
                            && propagated_into_next(&function.body, id)
                            && !toggle("MWCC_PCODE_NO_EARLY_PROPAGATED_SLOTS"))) =>
                {
                    let width = width(function.variables[id].ty);
                    self.frame_cursor = self.frame_cursor.div_ceil(width) * width + width;
                    self.registers[id] = Some(self.fresh(function.variables[id].ty));
                }
                Some((size, align)) if early && references(&function.body, id) == 0 => {
                    self.frame_cursor = self.frame_cursor.div_ceil(align.max(1)) * align.max(1) + size;
                }
                // An unreferenced frame object takes no slot.
                Some(_) if references(&function.body, id) == 0 => {}
                Some((size, _)) if early && parameter_slots.contains_key(&function.variables[id].name) && !toggle("MWCC_PCODE_EARLY_NEW_HOMES") => {
                    let start = parameter_slots[&function.variables[id].name] as i16;
                    self.frame_offsets[id] = Some(start);
                    self.pcode.frame_objects.push((start, start + size as i16));
                    self.pcode.variable_frame_objects += 1;
                }
                // Frame objects take slots in reverse declaration order.
                Some((size, align)) => {
                    let offset = self.frame_cursor.div_ceil(align.max(1)) * align.max(1);
                    self.frame_cursor = offset + size;
                    let start = i16::try_from(offset).map_err(|_| unsupported("a large frame"))?;
                    self.frame_offsets[id] = Some(start);
                    self.pcode.frame_objects.push((start, start + size as i16));
                    self.pcode.variable_frame_objects += 1;
                    // (A struct, not an array of them.)
                    if matches!(function.variables[id].ty, Type::Struct { size: struct_size, .. } if struct_size == size) {
                        self.pcode.struct_frame_objects.push(start);
                    }
                }
                None => self.registers[id] = Some(self.fresh(function.variables[id].ty)),
            }
        }
        if early && self.pcode.variable_frame_objects == 0 {
            self.reserved_end = self.frame_cursor;
        }
        // Loop cursors are numbered in creation order.
        if !toggle("MWCC_PCODE_LAZY_CURSORS") {
            for id in function.parameter_count..function.variables.len() {
                if function.variables[id].name.starts_with("@cursor") && self.registers[id].is_none() {
                    self.registers[id] = Some(self.fresh(function.variables[id].ty));
                }
            }
            // (Then the constants loops hoist.)
            for id in function.parameter_count..function.variables.len() {
                if function.variables[id].name.starts_with("@hoist")
                    && self.registers[id].is_none()
                    && !toggle("MWCC_PCODE_LAZY_HOISTS")
                {
                    self.registers[id] = Some(self.fresh(function.variables[id].ty));
                }
            }
        }
        // (A wide result goes straight to r3:r4.)
        if function.return_type != Type::Void && returns_through_variable && !is_wide(function.return_type) {
            self.return_register = Some(self.fresh(function.return_type));
        }
        self.pcode.begin_coalesce_window();

        self.pcode.referenced_general_parameters = (0..function.parameter_count)
            .filter(|&id| !is_float(function.variables[id].ty) && references(&function.body, id) > 0)
            .count();
        if self.unit.variadic {
            // `bne cr1,skip; stfd f1-f8,40..(r1); skip: stw r3-r10,8..(r1)`
            let skip = self.new_label();
            self.branch(Instruction::BranchConditionalForward { options: 4, condition_bit: 6, target: 0 }, skip);
            for register in 1..=8u32 {
                let mut save = PInstr::new(Instruction::StoreFloatDouble { s: register, a: 1, offset: 32 + 8 * register as i16 });
                save.flags.side_effect = true;
                self.emit(save);
            }
            self.place_label(skip);
            for register in 3..=10u32 {
                let mut save = PInstr::new(Instruction::StoreWord { s: register, a: 1, offset: 8 + 4 * (register as i16 - 3) });
                save.flags.side_effect = true;
                self.emit(save);
            }
        }
        for (virtual_register, physical) in wide_incoming {
            let mut copy = PInstr::new(Instruction::Or { a: virtual_register, s: physical, b: physical });
            copy.flags.entry_copy = true;
            self.emit(copy);
        }
        for (id, virtual_register, physical) in incoming {
            let ty = function.variables[id].ty;
            if is_float(ty) {
                self.emit_plain(Instruction::FloatMove { d: virtual_register, b: physical });
                continue;
            }
            // A narrow parameter that is assigned is re-extended on entry
            // (unless the build trusts the caller's extension).
            let trusted = self.unit.narrow_parameters_extended && ty != Type::Char;
            let ty = if self.raw_narrow[id] || trusted { Type::Int } else { ty };
            let mut copy = PInstr::new(extension(ty, virtual_register, physical));
            copy.flags.entry_copy = true;
            self.emit(copy);
        }
        for (register, ty, offset) in stack_incoming {
            // (The displacement grows by the frame's size once it is known.)
            let mut load = PInstr::new(match ty {
                Type::Char | Type::UnsignedChar => Instruction::LoadByteZero { d: register, a: 1, offset: offset + 3 },
                Type::Short => Instruction::LoadHalfwordAlgebraic { d: register, a: 1, offset: offset + 2 },
                Type::UnsignedShort => Instruction::LoadHalfwordZero { d: register, a: 1, offset: offset + 2 },
                _ => Instruction::LoadWord { d: register, a: 1, offset },
            });
            load.displacement_symbol = Some("@@incoming".to_owned());
            self.emit(load);
        }
        self.body_and_exit()
    }

    /// `-O0`: every variable lives in a fixed place for the whole function.
    /// A parameter of a leaf function stays in its argument register; in a
    /// function that calls, a parameter referenced at most once lives in the
    /// frame and the others are register variables, like declared locals
    /// (callee-saved, r31 down). Temporaries are colored around them.
    fn lower_unoptimized(&mut self, calls: bool) -> Compilation<()> {
        let words = (0..self.function.parameter_count).filter(|&id| !is_float(self.function.variables[id].ty)).count();
        if words > 8 || outgoing_argument_bytes(&self.function.body) > 0 {
            return Err(unsupported("-O0 stack-passed arguments"));
        }
        if self.unit.variadic {
            return Err(unsupported("a -O0 variadic definition"));
        }
        if self.function.variables.iter().any(|variable| is_wide(variable.ty)) || is_wide(self.function.return_type) {
            return Err(unsupported("-O0 wide values"));
        }
        let function = self.function;
        // (Early -O0 frames are not modeled; frameless leaves are.)
        if self.unit.branch_preserving && (calls || function.variables.iter().any(|variable| variable.frame.is_some())) {
            return Err(unsupported("-O0 frames on GC/1.2.5 and earlier"));
        }
        // Floating register variables take saved FPRs from f31 down, like
        // the GPR ones.
        let floating_variable = |id: usize| is_float(function.variables[id].ty);
        let mut home_bytes: i16 = 0;
        let mut home_slots: i16 = 0;
        // Register variables take r31 (f31) down: parameters the body
        // assigns, then locals (declaration order), then the other
        // parameters that need one.
        let mut register_parameters = Vec::new();
        let mut float_copies = Vec::new();
        let mut entry_copies = Vec::new();
        let (mut next_general, mut next_float) = (FIRST_GENERAL_ARGUMENT, 1);
        for id in 0..function.parameter_count {
            let argument = if floating_variable(id) {
                next_float += 1;
                next_float - 1
            } else {
                next_general += 1;
                next_general - 1
            };
            if !calls {
                self.registers[id] = Some(argument);
                self.raw_narrow[id] = is_narrow(function.variables[id].ty);
            } else if references(&function.body, id) == 0 && std::env::var_os("MWCC_PCODE_O0_UNUSED_HOMES").is_none() {
                // An unreferenced parameter is never stored.
            } else if references_weighted(&function.body, id, 2) <= 1 {
                // (A parameter used as an address takes a register.)
                // Homes are laid out in declaration order from r1+8.
                let size: i16 = if function.variables[id].ty == Type::Double { 8 } else { 4 };
                home_bytes = (home_bytes + size - 1) / size * size;
                self.homes[id] = Some(8 + home_bytes);
                home_bytes += size;
                home_slots += 1;
            } else {
                register_parameters.push((id, argument));
            }
        }
        let frame_objects = function.variables.iter().any(|variable| variable.frame.is_some());
        if frame_objects && home_slots > 0 && toggle("MWCC_PCODE_O0_NO_HOMES_WITH_FRAME") {
            return Err(unsupported("frame locals with parameter homes at -O0"));
        }
        // Frame objects take slots in reverse declaration order after the
        // homes (from r1+8).
        if home_slots > 0 {
            self.frame_cursor = 8 + home_bytes as u32;
        }
        // (By size class, as optimized.)
        let mut ordered: Vec<VarId> = (function.parameter_count..function.variables.len()).rev().collect();
        if !self.unit.early_frame && !toggle("MWCC_PCODE_FRAME_DECLARATION_ORDER") {
            ordered.sort_by_key(|&id| function.variables[id].frame.map_or(0, |(size, _)| size.max(1).next_power_of_two()));
        }
        for id in ordered {
            // (After the early builds an unreferenced one takes none.)
            if !self.unit.early_frame && references(&function.body, id) == 0 && !toggle("MWCC_PCODE_O0_UNUSED_FRAME_OBJECTS") {
                continue;
            }
            if let (VariableKind::Local, Some((size, align))) = (function.variables[id].kind, function.variables[id].frame) {
                let offset = self.frame_cursor.div_ceil(align.max(1)) * align.max(1);
                self.frame_cursor = offset + size;
                let start = i16::try_from(offset).map_err(|_| unsupported("a large frame"))?;
                self.frame_offsets[id] = Some(start);
                self.pcode.frame_objects.push((start, start + size as i16));
            }
        }
        // Most-referenced first (a use as an address counts twice); on a tie
        // locals precede parameters, then declaration order. Parameters in
        // registers only because they are dereferenced come last.
        let weight = |id: usize| references_weighted(&function.body, id, 2);
        let mut ranked: Vec<(usize, Option<u32>)> = register_parameters
            .iter()
            .filter(|p| references(&function.body, p.0) > 1)
            .map(|&(id, argument)| (id, Some(argument)))
            .chain(
                (function.parameter_count..function.variables.len())
                    .filter(|&id| function.variables[id].kind == VariableKind::Local && function.variables[id].frame.is_none())
                    // (An unreferenced local takes no register.)
                    .filter(|&id| references(&function.body, id) > 0 || toggle("MWCC_PCODE_O0_UNUSED_LOCAL_REGISTERS"))
                    .map(|id| (id, None)),
            )
            .collect();
        // (Tied parameters: the later declared first.)
        let reverse_parameters = !toggle("MWCC_PCODE_O0_PARAMETERS_IN_ORDER");
        ranked.sort_by_key(|&(id, argument)| {
            let order = if argument.is_some() && reverse_parameters { usize::MAX - id } else { id };
            (std::cmp::Reverse(weight(id)), argument.is_some(), order)
        });
        let order: Vec<(usize, Option<u32>)> = ranked
            .into_iter()
            .chain(
                register_parameters
                    .iter()
                    .filter(|p| references(&function.body, p.0) <= 1)
                    .map(|&(id, argument)| (id, Some(argument))),
            )
            .collect();
        let (mut next_saved, mut next_saved_float) = (31u32, 31u32);
        for (id, argument) in order {
            let register = if floating_variable(id) {
                if next_saved_float < 14 {
                    return Err(unsupported("more floating register variables than saved FPRs"));
                }
                next_saved_float -= 1;
                next_saved_float + 1
            } else {
                if next_saved < 14 {
                    return Err(unsupported("more register variables than callee-saved registers"));
                }
                next_saved -= 1;
                next_saved + 1
            };
            self.registers[id] = Some(register);
            // Narrow register variables hold their values unextended; each
            // read extends.
            self.raw_narrow[id] = is_narrow(function.variables[id].ty) && std::env::var_os("MWCC_PCODE_O0_EXTENDED_VARIABLES").is_none();
            match argument {
                Some(argument) if floating_variable(id) => float_copies.push((register, argument)),
                Some(argument) => entry_copies.push((register, argument)),
                None => {}
            }
        }
        if home_slots > 0 {
            self.frame_cursor = self.frame_cursor.max(8 + home_bytes as u32);
        }
        // (The local region rounds up to 8 bytes.)
        self.pcode.frame_local_bytes = if frame_objects {
            let bytes = (self.frame_cursor - 8) as i16;
            if toggle("MWCC_PCODE_O0_UNROUNDED_LOCALS") { bytes } else { (bytes + 7) / 8 * 8 }
        } else {
            ((home_bytes + 7) / 8) * 8
        };
        let float_registers: Vec<u32> = (0..function.variables.len())
            .filter(|&id| floating_variable(id))
            .filter_map(|id| self.registers[id])
            .collect();
        self.pcode.exit_uses = (0..function.variables.len())
            .filter(|&id| !floating_variable(id))
            .filter_map(|id| self.registers[id])
            .collect();
        self.pcode.exit_float_uses = float_registers;
        self.pcode.begin_coalesce_window();
        // Parameters arrive in declaration order: each stored to its home or
        // copied to its register variable.
        drop((entry_copies, float_copies));
        for id in 0..function.parameter_count {
            // The parameter's argument register in its class.
            let argument = if floating_variable(id) {
                1 + (0..id).filter(|&earlier| floating_variable(earlier)).count() as u32
            } else {
                FIRST_GENERAL_ARGUMENT + (0..id).filter(|&earlier| !floating_variable(earlier)).count() as u32
            };
            if let Some(offset) = self.homes[id] {
                self.emit_plain(store_instruction(function.variables[id].ty, argument, 1, offset));
            } else if let Some(register) = self.registers[id].filter(|&register| register != argument) {
                self.emit_plain(if floating_variable(id) {
                    Instruction::FloatMove { d: register, b: argument }
                } else {
                    Instruction::Or { a: register, s: argument, b: argument }
                });
            }
        }
        self.body_and_exit()
    }

    /// A body that is just a call whose result (if any) is the function's
    /// becomes `b callee` after marshaling the arguments.
    fn sibling_call(&mut self) -> Compilation<bool> {
        let function = self.function;
        let call = match function.body.as_slice() {
            [Stmt::Eval(call)] if function.return_type == Type::Void || !toggle("MWCC_PCODE_NO_FALLOFF_TAIL_CALLS") => call,
            [Stmt::SetReturn(call)] if call.ty == function.return_type => call,
            _ => return Ok(false),
        };
        let ExprKind::Call { arguments, .. } = &call.kind else { return Ok(false) };
        // (An argument that calls needs a frame: no sibling call.)
        if arguments.iter().any(|argument| format!("{:?}", argument.kind).contains("Call {")) {
            return Ok(false);
        }
        self.tail_call(call)?;
        self.resolve_branches()?;
        Ok(true)
    }

    /// `b callee` after marshaling the arguments of a call in tail position.
    fn tail_call(&mut self, call: &Expr) -> Compilation<()> {
        let ExprKind::Call { name, arguments } = &call.kind else { return Err(unsupported("tail call")) };
        if arguments.iter().filter(|argument| !is_float(argument.ty)).count() > 8 {
            return Err(unsupported("a tail call with stack arguments"));
        }
        if arguments.iter().any(|argument| is_float(argument.ty)) || is_float(call.ty) {
            return Err(unsupported("floating sibling call"));
        }
        // (A wide argument takes an odd-aligned pair.)
        let mut placed: Vec<(u32, u32)> = Vec::new();
        let mut general = FIRST_GENERAL_ARGUMENT;
        for argument in arguments {
            if is_wide(argument.ty) {
                general += 1 - general % 2;
                let (high, low) = self.wide(argument)?;
                placed.push((general, high));
                placed.push((general + 1, low));
                general += 2;
            } else {
                placed.push((general, self.expression(argument)?.0));
                general += 1;
            }
        }
        for &(register, value) in &placed {
            if value != register || !is_wide_call(call) {
                self.emit_plain(Instruction::Or { a: register, s: value, b: value });
            }
        }
        // A variadic callee still gets its CR1 marker (no floating arguments).
        if self.unit.variadic_callees.contains(name) && !toggle("MWCC_PCODE_NO_TAIL_VARIADIC_MARKER") {
            self.emit_plain(Instruction::ConditionRegisterClear { d: 6 });
        }
        let mut branch = PInstr::new(Instruction::BranchExternal { target: name.clone() });
        branch.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(name.clone()),
        });
        branch.implicit_uses = placed.iter().map(|&(register, _)| Register::general(register)).collect();
        self.emit(branch);
        self.start_block(false);
        Ok(())
    }

    fn body_and_exit(&mut self) -> Compilation<()> {
        let result = self.body_and_exit_inner();
        if self.unoptimized {
            // Conversion slots follow the parameter homes and frame objects.
            let used = (self.frame_cursor - 8) as i16;
            self.pcode.frame_local_bytes = self.pcode.frame_local_bytes.max(used);
        } else {
            self.pcode.frame_local_bytes = (self.frame_cursor - 8) as i16;
            // (The local region rounds up to 8 bytes.)
            if !toggle("MWCC_PCODE_UNROUNDED_LOCALS") {
                self.pcode.frame_local_bytes = (self.pcode.frame_local_bytes + 7) / 8 * 8;
            }
            // (A reservation alone needs no frame.)
            // (Unless arguments pass through the frame: the outgoing area,
            // or incoming stack arguments.)
            let words = (0..self.function.parameter_count).filter(|&id| !is_float(self.function.variables[id].ty)).count();
            if self.unit.early_frame
                && self.frame_cursor == self.reserved_end
                && outgoing_argument_bytes(&self.function.body) == 0
                && words <= 8
            {
                self.pcode.reserved_local_bytes = self.pcode.frame_local_bytes;
                self.pcode.frame_local_bytes = 0;
            }
            for &(start, _) in &self.pcode.frame_objects {
                if !self.escaped_frame_objects.contains(&start) && !self.pcode.private_frame_objects.contains(&start) {
                    self.pcode.private_frame_objects.push(start);
                }
            }
        }
        result
    }

    /// `switch`: MWCC's binary search over the case ranges, then the arms in
    /// order.
    fn switch(&mut self, value: &Expr, cases: &[(i64, usize)], arms: &[Vec<Stmt>], default: Option<usize>) -> Compilation<()> {
        if self.unoptimized && toggle("MWCC_PCODE_O0_NO_SWITCH") {
            return Err(unsupported("switch at -O0"));
        }
        if self.unit.switch_style == 2 {
            return Err(unsupported("switch (linear compare chains)"));
        }
        let in_place = self.unit.switch_style == 1;
        // (Cases compare as signed words: an unsigned case above INT_MAX
        // wraps.)
        let mut sorted: Vec<(i64, usize)> = cases
            .iter()
            .map(|&(value, arm)| match u32::try_from(value) {
                Ok(word) if !toggle("MWCC_PCODE_NO_WRAPPED_CASES") => (i64::from(word as i32), arm),
                _ => (value, arm),
            })
            .collect();
        sorted.sort_by_key(|&(value, _)| value);
        // (A case outside a 16-bit immediate compares with its constant in a
        // register.)
        let wide_cases = toggle("MWCC_PCODE_NO_WIDE_SWITCH_CASES");
        if sorted.iter().any(|&(value, _)| {
            if wide_cases {
                i16::try_from(value).is_err() || i16::try_from(value + 1).is_err()
            } else {
                i32::try_from(value).is_err() || i32::try_from(value + 1).is_err()
            }
        }) {
            return Err(unsupported("switch case outside a 16-bit immediate"));
        }
        let exit = self.new_join_label();
        let arm_labels: Vec<Label> = arms.iter().map(|_| self.new_join_label()).collect();
        // Segments: maximal runs of consecutive values with the same arm.
        let mut segments: Vec<(i64, i64, usize)> = Vec::new();
        for &(value, arm) in &sorted {
            match segments.last_mut() {
                Some(last) if last.1 == value - 1 && last.2 == arm => last.1 = value,
                _ => segments.push((value, value, arm)),
            }
        }
        let tree = SwitchTree { segments: &segments };
        let (x, _) = self.expression(value)?;
        let default_target = default.map_or(exit, |arm| arm_labels[arm]);
        let target = |arm: Option<usize>| arm.map_or(default_target, |arm| arm_labels[arm]);
        if tree.uses_table() && std::env::var_os("MWCC_PCODE_NO_JUMP_TABLES").is_none() {
            // Dispatch through a table indexed from 0 (small minimums) or
            // from the minimum.
            let (first, last) = (sorted[0].0, sorted[sorted.len() - 1].0);
            let base = if (0..=2).contains(&first) { 0 } else { first };
            let index = if base == 0 {
                x
            } else {
                let index = self.temporary();
                self.emit_based(Instruction::AddImmediate { d: index, a: x, immediate: i16::try_from(-base).map_err(|_| unsupported("table base"))? }, x);
                index
            };
            let limit = u16::try_from(last - base).map_err(|_| unsupported("table size"))?;
            self.emit_plain(Instruction::CompareLogicalWordImmediate { a: index, immediate: limit });
            self.branch(Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 0 }, default_target);
            let table = self.pcode.jump_tables.len() + self.pending_tables.len();
            let high = self.temporary();
            let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
            lis.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Ha, target: RelocationTarget::JumpTableAt(table) });
            self.emit(lis);
            // -O0 (unscheduled): the table's address, then the scaled
            // index; the entry loads in place.
            let in_place = in_place || self.unoptimized;
            let scaled = self.temporary();
            if !self.unoptimized {
                self.emit_plain(Instruction::ShiftLeftImmediate { a: scaled, s: index, shift: 2 });
            }
            let address = if in_place { high } else { self.temporary() };
            let mut addi = PInstr::new(Instruction::AddImmediate { d: address, a: high, immediate: 0 });
            addi.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: RelocationTarget::JumpTableAt(table) });
            addi.not_r0.push(high);
            addi.flags.in_place = in_place;
            self.emit(addi);
            if self.unoptimized {
                self.emit_plain(Instruction::ShiftLeftImmediate { a: scaled, s: index, shift: 2 });
            }
            let entry = if in_place { address } else { self.temporary() };
            let mut load = PInstr::new(Instruction::LoadWordIndexed { d: entry, a: address, b: scaled });
            load.not_r0.push(address);
            load.flags.in_place = in_place;
            self.emit(load);
            self.emit_plain(Instruction::MoveToCountRegister { s: entry });
            self.emit_plain(Instruction::BranchToCountRegister);
            let entries: Vec<Label> = (base..=last)
                .map(|value| target(sorted.iter().find(|&&(case, _)| case == value).map(|&(_, arm)| arm)))
                .collect();
            let block = self.current_block();
            let offset = arms.len() as u32 + 1 + u32::from(default.is_some());
            self.pending_tables.push((block, entries, offset));
            self.start_block(false);
        } else {
            // (Over the word's values.)
            let (low, high) = if toggle("MWCC_PCODE_UNBOUNDED_SWITCH_TREE") { (i64::MIN, i64::MAX) } else { (i64::from(i32::MIN), i64::from(i32::MAX)) };
            self.switch_node(&tree, x, low, high, &target)?;
        }
        let enclosing_next = self.loops.last().map_or(exit, |&(_, next)| next);
        self.loops.push((exit, enclosing_next));
        for (index, arm) in arms.iter().enumerate() {
            self.place_label(arm_labels[index]);
            for statement in arm {
                self.statement(statement)?;
            }
        }
        self.loops.pop();
        self.place_if_targeted(exit);
        Ok(())
    }

    /// One node of the decision tree for values in `[low, high]`.
    fn switch_node(
        &mut self,
        tree: &SwitchTree<'_>,
        x: u32,
        low: i64,
        high: i64,
        target: &dyn Fn(Option<usize>) -> Label,
    ) -> Compilation<()> {
        if let Some(only) = tree.single(low, high) {
            self.jump(target(only));
            return Ok(());
        }
        // BO 12: branch if the bit is set; BO 4: if clear. cr0: lt 0, eq 2.
        let compare = |selection: &mut Self, k: i64| -> Compilation<()> {
            match i16::try_from(k) {
                Ok(immediate) => selection.emit_plain(Instruction::CompareWordImmediate { a: x, immediate }),
                Err(_) => {
                    let constant = selection.temporary();
                    selection.load_constant(constant, k)?;
                    selection.emit_plain(Instruction::CompareWord { a: x, b: constant });
                }
            }
            Ok(())
        };
        match tree.pivot(low, high) {
            SwitchPivot::Split(at) => {
                // `cmpwi x,at; bge right` — values at or above `at` go right.
                compare(self, at)?;
                match tree.single(at, high) {
                    Some(right) => {
                        self.branch(Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 0 }, target(right));
                        self.switch_node(tree, x, low, at - 1, target)
                    }
                    None => {
                        let right = self.new_label();
                        self.branch(Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 0 }, right);
                        self.switch_node(tree, x, low, at - 1, target)?;
                        self.place_label(right);
                        self.switch_node(tree, x, at, high, target)
                    }
                }
            }
            SwitchPivot::Equal(v, arm) => {
                compare(self, v)?;
                self.branch(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 }, target(Some(arm)));
                let right = tree.single(v + 1, high);
                let left = tree.single(low, v - 1);
                match (left, right) {
                    (Some(l), Some(r)) if l == r => {
                        self.jump(target(r));
                        Ok(())
                    }
                    (_, Some(r)) => {
                        self.branch(Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 0 }, target(r));
                        self.switch_node(tree, x, low, v - 1, target)
                    }
                    (Some(l), None) => {
                        self.branch(Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 0 }, target(l));
                        self.switch_node(tree, x, v + 1, high, target)
                    }
                    (None, None) => {
                        let right = self.new_label();
                        self.branch(Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 0 }, right);
                        self.switch_node(tree, x, low, v - 1, target)?;
                        self.place_label(right);
                        self.switch_node(tree, x, v + 1, high, target)
                    }
                }
            }
        }
    }

    /// A base and displacement for `base + offset`: a displacement beyond 16
    /// bits adds its high half first (`addis t,base,hi; lwz d,lo(t)`).
    /// A memory access base.
    fn base_expression(&mut self, base: &Expr) -> Compilation<(u32, Type)> {
        let outer = std::mem::replace(&mut self.memory_base, base as *const Expr as usize);
        let result = self.expression(base);
        self.memory_base = outer;
        result
    }

    fn displacement(&mut self, base: u32, offset: i32) -> Compilation<(u32, i16)> {
        if let Ok(offset) = i16::try_from(offset) {
            return Ok((base, offset));
        }
        let low = offset as i16;
        let high = i16::try_from((offset - i32::from(low)) >> 16).map_err(|_| unsupported("large member offset"))?;
        let adjusted = self.temporary();
        self.emit_based(Instruction::AddImmediateShifted { d: adjusted, a: base, immediate: high }, base);
        Ok((adjusted, low))
    }

    /// A fresh 8-byte, 8-aligned slot for an integer/floating conversion
    /// (`rounding`: a float-to-integer one).
    fn conversion_slot(&mut self, rounding: bool) -> i16 {
        // -O0 reuses its conversion slots in each basic block (the k-th
        // conversion of a block takes the k-th slot).
        if self.unoptimized && !toggle("MWCC_PCODE_O0_FRESH_CONVERSION_SLOTS") {
            let base = *self.conversion_base.get_or_insert(self.frame_cursor.div_ceil(8) * 8);
            let block = self.current_block();
            let k = if self.conversion_block == Some(block) { self.conversion_count } else { 0 };
            self.conversion_block = Some(block);
            self.conversion_count = k + 1;
            let offset = base + 8 * k;
            self.frame_cursor = self.frame_cursor.max(offset + 8);
            if !self.pcode.frame_objects.contains(&(offset as i16, offset as i16 + 8)) {
                self.pcode.frame_objects.push((offset as i16, offset as i16 + 8));
                self.pcode.private_frame_objects.push(offset as i16);
            }
            return offset as i16;
        }
        // (Optimized GC/1.3-on code reuses its float-to-integer slots the
        // same way; integer-to-float ones are always fresh.)
        let reused = rounding && !self.unit.early_frame && !toggle("MWCC_PCODE_FRESH_ROUNDING_SLOTS");
        let k = if reused {
            let block = self.current_block();
            let k = if self.conversion_block == Some(block) { self.conversion_count } else { 0 };
            self.conversion_block = Some(block);
            self.conversion_count = k + 1;
            if let Some(&slot) = self.rounding_slots.get(k as usize) {
                return slot;
            }
            Some(k)
        } else {
            None
        };
        let offset = self.frame_cursor.div_ceil(8) * 8;
        self.frame_cursor = offset + 8;
        if k.is_some() {
            self.rounding_slots.push(offset as i16);
        }
        self.pcode.frame_objects.push((offset as i16, offset as i16 + 8));
        self.pcode.private_frame_objects.push(offset as i16);
        // (Its two words are stored independently.)
        if !self.unit.early_frame && !toggle("MWCC_PCODE_CONVERSION_WORDS_ORDERED") {
            self.pcode.struct_frame_objects.push(offset as i16);
        }
        offset as i16
    }

    fn body_and_exit_inner(&mut self) -> Compilation<()> {
        let function = self.function;
        // (A function with frame objects keeps its frame: no sibling call.)
        let outgoing = outgoing_argument_bytes(&function.body) > 0;
        if self.tail_calls && !self.unoptimized && !outgoing && self.pcode.frame_objects.is_empty() && self.sibling_call()? {
            return Ok(());
        }
        self.all_tail_calls = self.tail_calls
            && !self.unoptimized
            && !outgoing
            && makes_calls(&function.body)
            && !format!("{:?}", function.body).contains("LocalAddress")
            && only_tail_calls(&function.body, true, function.return_type);
        self.exit_label = self.new_label();
        for statement in &function.body {
            self.statement(statement)?;
        }
        // The exit block: the epilogue is generated here after coloring.
        let last = self.current_block();
        let falls_through = !self.block_ends_in_jump(last);
        // An empty last block (a join nothing follows) is the exit itself.
        let exit = if self.pcode.blocks[last].instructions.is_empty() {
            last
        } else {
            self.start_block(falls_through)
        };
        self.labels[self.exit_label.0] = Some(exit);
        if let Some(register) = self.return_register {
            // The copy to the result register joins all returns; the (empty)
            // return block that receives the epilogue follows it.
            let return_type = self.function.return_type;
            self.copy(return_type, result_register(return_type), register);
            self.start_block(true);
        }
        if self.restore_violation {
            return Err(unsupported("a branch into a block that inherited known values"));
        }
        self.resolve_branches()
    }

    fn statement(&mut self, statement: &Stmt) -> Compilation<()> {
        let known = std::mem::take(&mut self.known_constant);
        if let Stmt::Assign { variable, value } = statement {
            // (Other variables' assignments keep the constants known.)
            if !contains_call(value) && !toggle("MWCC_PCODE_KNOWN_ONLY_ADJACENT") {
                self.known_constant = known.iter().copied().filter(|&(known, _)| known != *variable).collect();
            }
            if let Some(value) = value.as_int() {
                self.known_constant.push((*variable, value));
            }
        }
        match statement {
            Stmt::Eval(call) | Stmt::SetReturn(call) | Stmt::Return(Some(call))
                if self.all_tail_calls && matches!(call.kind, ExprKind::Call { .. }) =>
            {
                self.tail_call(call)
            }
            Stmt::If { condition, then_body, else_body }
                if condition.as_int().is_some()
                    && !self.unoptimized
                    && !holds_label(if condition.as_int() != Some(0) { else_body } else { then_body }) =>
            {
                // IRO evaluates a constant condition: only one arm remains.
                let arm = if condition.as_int() != Some(0) { then_body } else { else_body };
                for statement in arm {
                    self.statement(statement)?;
                }
                Ok(())
            }
            Stmt::Loop { test_first, condition: Some(condition), body, step, effects }
                if condition.as_int() == Some(0) && !self.unoptimized && effects.is_empty() && !holds_label(body) =>
            {
                // A loop whose test is false: a do-while body runs once.
                if !*test_first {
                    let exit = self.new_label();
                    self.loops.push((exit, exit));
                    for statement in body.iter().chain(step) {
                        self.statement(statement)?;
                    }
                    self.loops.pop();
                    self.place_if_targeted(exit);
                }
                Ok(())
            }
            Stmt::Loop { condition, body, step, effects, .. }
                if condition.as_ref().is_none_or(|c| c.as_int().is_some_and(|k| k != 0)) =>
            {
                if !effects.is_empty() {
                    return Err(unsupported("a constant loop test with effects"));
                }
                // No test: the body repeats until a break.
                let top = self.new_join_label();
                let next = self.new_label();
                let exit = self.new_label();
                self.place_label(top);
                self.loops.push((exit, next));
                for statement in body {
                    self.statement(statement)?;
                }
                self.place_if_targeted(next);
                for statement in step {
                    self.statement(statement)?;
                }
                self.loops.pop();
                self.jump(top);
                self.place_if_targeted(exit);
                Ok(())
            }
            Stmt::Loop { test_first, condition, body, step, effects } => {
                // (-O3 counts them in CTR, -O4 unrolls them; -O1/-O2 keep
                // the plain loop.)
                // (A loop the unroller did not take stays a plain loop.)
                if self.unit.strength_reduction
                    && effects.is_empty()
                    && counted(condition.as_ref(), body, step)
                    && !makes_calls(body)
                    && toggle("MWCC_PCODE_REFUSE_COUNTED_LOOPS")
                {
                    return Err(unsupported("counted loop (unrolling not modeled)"));
                }
                let top = self.new_join_label();
                let next = self.new_label();
                let test = self.new_label();
                let exit = self.new_label();
                // IRO drops the entry jump when the first test is known true.
                let first_test_true = self.unit.strength_reduction
                    && effects.is_empty()
                    && condition.as_ref().is_some_and(|condition| initially_true(condition, &known));
                let preloaded = self.preload_loop_constants(&[body, step, effects], condition.as_ref())?;
                if *test_first && condition.is_some() && !first_test_true {
                    self.jump(test);
                }
                self.place_label(top);
                self.loops.push((exit, next));
                for statement in body {
                    self.statement(statement)?;
                }
                self.place_if_targeted(next);
                for statement in step {
                    self.statement(statement)?;
                }
                self.loops.pop();
                self.place_if_targeted(test);
                // The condition's effects run before each test.
                for statement in effects {
                    self.statement(statement)?;
                }
                match condition {
                    // A loop never repeated (`do ... while (0)`) has no test.
                    Some(condition) if condition.as_int() == Some(0) => {}
                    Some(condition) => self.branch_on(condition, true, top)?,
                    None => self.jump(top),
                }
                if preloaded {
                    self.loop_constants.pop();
                }
                self.place_label(exit);
                Ok(())
            }
            Stmt::Counted { count, guard, body } => {
                // `mtctr count` (and the guard), the body, `bdnz`.
                let top = self.new_join_label();
                let exit = self.new_label();
                let preloaded = self.preload_loop_constants(&[body], None)?;
                let (counter, _) = self.expression(count)?;
                // (A loop's `mtctr` keeps its place, last before the loop.)
                let mut mtctr = PInstr::new(Instruction::MoveToCountRegister { s: counter });
                mtctr.flags.serialize = true;
                self.emit(mtctr);
                if let Some(guard) = guard {
                    self.branch_on(guard, false, exit)?;
                }
                self.place_label(top);
                self.loops.push((exit, top));
                for statement in body {
                    self.statement(statement)?;
                }
                self.loops.pop();
                self.branch(Instruction::BranchConditionalForward { options: 16, condition_bit: 0, target: 0 }, top);
                if preloaded {
                    self.loop_constants.pop();
                }
                self.place_label(exit);
                Ok(())
            }
            Stmt::Switch { value, cases, arms, default } => self.switch(value, cases, arms, *default),
            Stmt::Break => {
                let (exit, _) = *self.loops.last().ok_or_else(|| unsupported("break outside a loop"))?;
                self.jump(exit);
                Ok(())
            }
            Stmt::Goto(name) => {
                let label = self.named_label(name);
                self.jump(label);
                Ok(())
            }
            Stmt::Label(name) => {
                let label = self.named_label(name);
                self.place_label(label);
                Ok(())
            }
            Stmt::Continue => {
                let (_, next) = *self.loops.last().ok_or_else(|| unsupported("continue outside a loop"))?;
                self.jump(next);
                Ok(())
            }
            Stmt::Assign { variable, value } if is_wide(self.function.variables[*variable].ty) => {
                self.assign_wide(*variable, value)
            }
            Stmt::Assign { variable, value } => self.assign(*variable, value),
            Stmt::Eval(value) => match &value.kind {
                ExprKind::Call { name, arguments } => self.call(name, arguments, Type::Void, None).map(|_| ()),
                // A discarded value without effects (`(void)x;`).
                ExprKind::Int(_) | ExprKind::Var(_) => Ok(()),
                // `__builtin_va_info(&ap)`: the named register counts, the
                // caller's argument area and the register save area.
                ExprKind::Idiom(Idiom::Unary(IntrinsicOp::VaInfo, address)) => {
                    let (a, _) = self.expression(address)?;
                    let parameters = 0..self.function.parameter_count;
                    let generals = parameters.clone().filter(|&id| !is_float(self.function.variables[id].ty)).count() as i64;
                    let floats = parameters.filter(|&id| is_float(self.function.variables[id].ty)).count() as i64;
                    let counts = self.temporary();
                    self.load_constant(counts, (generals << 24) | (floats << 16))?;
                    self.emit_based(Instruction::StoreWord { s: counts, a, offset: 0 }, a);
                    let incoming = self.temporary();
                    let mut addi = PInstr::new(Instruction::AddImmediate { d: incoming, a: 1, immediate: 8 });
                    addi.displacement_symbol = Some("@@incoming".to_owned());
                    self.emit(addi);
                    self.emit_based(Instruction::StoreWord { s: incoming, a, offset: 4 }, a);
                    let saved = self.temporary();
                    self.emit_plain(Instruction::AddImmediate { d: saved, a: 1, immediate: 8 });
                    self.emit_based(Instruction::StoreWord { s: saved, a, offset: 8 }, a);
                    Ok(())
                }
                // A barrier: kept, in order against everything.
                ExprKind::Idiom(Idiom::Unary(
                    op @ (IntrinsicOp::Synchronize | IntrinsicOp::InstructionSynchronize | IntrinsicOp::EnforceInOrderIo),
                    _,
                )) => {
                    let mut barrier = PInstr::new(match op {
                        IntrinsicOp::Synchronize => Instruction::Synchronize,
                        IntrinsicOp::InstructionSynchronize => Instruction::InstructionSynchronize,
                        _ => Instruction::EnforceInOrderIo,
                    });
                    barrier.flags.side_effect = true;
                    barrier.flags.serialize = true;
                    self.emit(barrier);
                    Ok(())
                }
                // A discarded volatile read: loaded into a scratch register
                // (and kept).
                ExprKind::Load { .. } | ExprKind::Global(_) => {
                    self.expression(value)?;
                    let block = self.current_block();
                    if let Some(load) = self.pcode.blocks[block].instructions.last_mut() {
                        load.flags.side_effect = true;
                    }
                    Ok(())
                }
                _ => Err(unsupported("expression statement")),
            },
            Stmt::Store { place, ty, value, .. } if is_wide(*ty) => {
                self.forget_frame_loads();
                self.store_wide(place, value)
            }
            Stmt::Store { place, ty, value, compound } => {
                self.compound_store = *compound;
                let stored = self.store(place, *ty, value);
                self.compound_store = false;
                stored
            }
            // `if (c) break;` / `if (c) continue;`: one branch on `c`.
            Stmt::If { condition, then_body, else_body }
                if else_body.is_empty()
                    && matches!(then_body.as_slice(), [Stmt::Break | Stmt::Continue])
                    && !self.loops.is_empty()
                    && !toggle("MWCC_PCODE_NO_DIRECT_BREAK") =>
            {
                let (exit, next) = *self.loops.last().expect("checked");
                let label = if matches!(then_body[0], Stmt::Break) { exit } else { next };
                self.branch_on(condition, true, label)
            }
            // `if (c) goto l;`: one branch on `c` to the label.
            Stmt::If { condition, then_body, else_body }
                if else_body.is_empty() && matches!(then_body.as_slice(), [Stmt::Goto(_)]) =>
            {
                let Stmt::Goto(name) = &then_body[0] else { unreachable!() };
                let label = self.named_label(name);
                self.branch_on(condition, true, label)
            }
            // `if (c) return;`: one branch on `c` to the exit.
            Stmt::If { condition, then_body, else_body }
                if else_body.is_empty()
                    && matches!(then_body.as_slice(), [Stmt::Return(None)])
                    // (A `&&`/`||` condition keeps its `b exit` block.)
                    && !matches!(condition.kind, ExprKind::Binary(BinaryOp::LogicalAnd | BinaryOp::LogicalOr, ..))
                    && !toggle("MWCC_PCODE_NO_DIRECT_RETURN") =>
            {
                let exit = self.exit_label;
                self.branch_on(condition, true, exit)
            }
            Stmt::If { condition, then_body, else_body } => {
                let otherwise = self.new_label();
                self.branch_on(condition, false, otherwise)?;
                for statement in then_body {
                    self.statement(statement)?;
                }
                if else_body.is_empty() {
                    self.place_label(otherwise);
                } else {
                    let join = self.new_label();
                    if !self.block_ends_in_jump(self.current_block()) {
                        self.jump(join);
                    }
                    self.place_label(otherwise);
                    for statement in else_body {
                        self.statement(statement)?;
                    }
                    self.place_label(join);
                }
                Ok(())
            }
            Stmt::SetReturn(value) => self.return_value(value),
            Stmt::Return(value) => {
                if let Some(value) = value {
                    self.return_value(value)?;
                }
                let exit = self.exit_label;
                self.jump(exit);
                Ok(())
            }
        }
    }

    fn return_value(&mut self, value: &Expr) -> Compilation<()> {
        let return_type = self.function.return_type;
        if is_wide(return_type) {
            return self.return_wide(value);
        }
        // A truth value returned narrow is still masked to the type.
        // (Also through a conversion to the narrow type: `(u8)(a == 1)`,
        // except a C++ `bool` on the early builds.)
        let truth_kind = |e: &Expr| {
            matches!(&e.kind, ExprKind::Binary(op, ..) if op.is_comparison())
                || matches!(&e.kind, ExprKind::Unary(UnaryOp::LogicalNot, _))
        };
        let truth = truth_kind(value)
            || (matches!(&value.kind, ExprKind::Convert(inner) if truth_kind(inner)) && !(self.unit.cxx && self.unit.early_frame) && !toggle("MWCC_PCODE_NO_CONVERTED_TRUTH"));
        // A `bool` result is already 0/1; other narrow results mask it.
        // (Or a value whose bounds fit the unsigned narrow type.)
        let bounded = is_unsigned_narrow(return_type) && !truth && !toggle("MWCC_PCODE_NO_BOUNDED_RETURNS") && {
            SUM_BOUNDS.with(|flag| flag.set(self.unit.branch_preserving));
            let mut inner = value;
            while let ExprKind::Convert(operand) = &inner.kind {
                if !matches!(operand.ty, Type::Int | Type::UnsignedInt) {
                    break;
                }
                inner = operand;
            }
            max_value(inner).is_some_and(|max| max < 1u64 << (8 * width(return_type)))
        };
        let fits = (mwcc_syntax_trees_to_iro_fits(value, return_type) || bounded)
            && !(truth && is_narrow(return_type) && !self.unit.returns_bool && std::env::var_os("MWCC_PCODE_TRUTH_FITS").is_none());
        if let Some(destination) = self.return_register {
            let raw = self.is_raw(value);
            let (register, ty) = self.expression_with_target(value, Some(destination))?;
            let (register, _) = if fits {
                (register, ty)
            } else {
                self.convert_value(register, ty, raw, return_type, Some(destination))?
            };
            if register != destination {
                self.copy(return_type, destination, register);
            }
            return Ok(());
        }
        if is_float(return_type) {
            // (-O0 computes the value straight into f1.)
            let direct = (self.unoptimized && value.ty == return_type && !toggle("MWCC_PCODE_O0_FLOAT_RETURN_COPY")).then_some(1);
            let (register, _) = self.expression_with_target(value, direct)?;
            if register != 1 {
                self.copy(return_type, 1, register);
            }
            return Ok(());
        }
        // (Optimized, an arithmetic result is computed into r3 as well.)
        let computed = matches!(&value.kind, ExprKind::Binary(op, ..) if !op.is_comparison() && !matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr))
            && !format!("{value:?}").contains("Call {")
            && value.ty == return_type
            && !toggle("MWCC_PCODE_NO_DIRECT_RETURNS");
        let direct = (self.unoptimized || computed).then_some(3);
        // A returned signed byte load extends in place (`lbz r3; extsb r3,r3`).
        if !self.unoptimized
            && !fits
            && value.ty == Type::Char
            && !is_narrow(return_type)
            && matches!(value.kind, ExprKind::Load { .. } | ExprKind::Global(_))
            && self.is_raw(value)
            && std::env::var_os("MWCC_PCODE_NO_INPLACE_EXTSB").is_none()
        {
            let (register, _) = self.expression(value)?;
            self.emit_plain(Instruction::ExtendSignByte { a: register, s: register });
            self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
            return Ok(());
        }
        // The final instruction targets r3: the conversion when there is one.
        let converts = !fits && is_narrow(return_type) && value.ty != return_type;
        let raw = self.is_raw(value);
        let outer = std::mem::replace(&mut self.returned, if converts { 0 } else { value as *const Expr as usize });
        let lowered = self.expression_with_target(value, if converts { None } else { direct });
        self.returned = outer;
        let (register, ty) = lowered?;
        // A truth value (`srwi t,s,n`) masked to an unsigned narrow type is
        // one rotate-and-mask.
        // C masks to an unsigned narrow type; in C++ the `bool` truth value
        // zero-extends from a byte into any narrow type.
        let cxx = self.unit.cxx;
        // (A `signed char` result sign-extends the byte instead.)
        if !fits
            && truth
            && is_narrow(return_type)
            && (is_unsigned_narrow(return_type) || (cxx && return_type != Type::Char))
            && !self.unoptimized
        {
            let block = self.current_block();
            if let Some(last) = self.pcode.blocks[block].instructions.last_mut() {
                if let Instruction::ShiftRightLogicalImmediate { a, s, shift } = last.instruction {
                    if a == register {
                        let bits = if cxx { 8 } else { 8 * width(return_type) as u8 };
                        let begin = (32 - bits).max(shift);
                        last.instruction = Instruction::RotateAndMask { a: 3, s, shift: 32 - shift, begin, end: 31 };
                        return Ok(());
                    }
                }
            }
        }
        let (register, _) =
            if fits { (register, ty) } else { self.convert_value(register, ty, raw, return_type, direct)? };
        if register != 3 {
            self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
        }
        Ok(())
    }

    fn assign(&mut self, variable: VarId, value: &Expr) -> Compilation<()> {
        if let Some(offset) = self.homes[variable] {
            self.forget_frame_loads();
            let (source, _) = self.expression(value)?;
            self.emit_plain(store_instruction(self.function.variables[variable].ty, source, 1, offset));
            return Ok(());
        }
        self.common.retain(|_, (_, _, read)| !read.contains(&variable));
        // A temporary assigned once takes the register its value is
        // computed into (numbered after its operands').
        let variable_type = self.function.variables[variable].ty;
        if self.registers[variable].is_none()
            && !self.unoptimized
            && self.function.variables[variable].kind == VariableKind::Temporary
            && !is_narrow(variable_type)
            && !is_wide(variable_type)
            && assignments(&self.function.body, variable) == 1
            && !toggle("MWCC_PCODE_EAGER_TEMPORARIES")
        {
            let class = if is_float(variable_type) { Class::Float } else { Class::General };
            let mark = self.pcode.register_count(class);
            let (source, source_type) = self.expression(value)?;
            if source >= mark && source_type == variable_type && !self.registers.contains(&Some(source)) {
                self.registers[variable] = Some(source);
            } else {
                let destination = self.register(variable);
                if source != destination {
                    self.copy(variable_type, destination, source);
                }
            }
            return Ok(());
        }
        let destination = self.register(variable);
        let stale: Vec<String> = self
            .loaded_globals
            .iter()
            .filter(|(_, (register, _))| *register == destination)
            .map(|(global, _)| global.clone())
            .collect();
        for global in stale {
            self.loaded_globals.remove(&global);
        }
        // -O0 raw variables take narrow values as they are, and updates of
        // themselves (`i++`, `i += n`) unextended.
        let kept_raw = (self.unoptimized || self.function.variables[variable].raw)
            && self.raw_narrow[variable]
            && (match &value.kind {
                ExprKind::Var(_) | ExprKind::Load { .. } | ExprKind::Global(_) => value.ty == variable_type,
                // (A floating value converted to the variable's type.)
                ExprKind::Convert(inner) if is_float(inner.ty) => value.ty == variable_type,
                ExprKind::Binary(_, left, right) => {
                    matches!(unpromoted(left).kind, ExprKind::Var(id) if id == variable) && right.as_int().is_some()
                }
                _ => false,
            });
        let raw = self.is_raw(value);
        // (A raw value assigned to a narrow variable is extended when
        // optimized; -O0 extends at each read instead.)
        let narrow = is_narrow(variable_type)
            && (!mwcc_syntax_trees_to_iro_fits(value, variable_type)
                || (raw && !self.unoptimized && matches!(value.kind, ExprKind::Load { .. } | ExprKind::Global(_) | ExprKind::Convert(_))))
            && !kept_raw;
        // (A narrow value of the variable's own type needs no conversion.)
        let converts = narrow && (value.ty != variable_type || raw || toggle("MWCC_PCODE_NO_NARROW_ASSIGN_TARGET"));
        // -O0: a raw loaded byte lands in the variable and extends in place.
        if self.unoptimized
            && narrow
            && raw
            && matches!(value.kind, ExprKind::Load { .. } | ExprKind::Global(_))
            && mwcc_iro::width(value.ty) <= mwcc_iro::width(variable_type)
            && !toggle("MWCC_PCODE_O0_RAW_LOAD_TEMPORARY")
        {
            let (loaded, loaded_type) = self.expression_with_target(value, Some(destination))?;
            self.emit_plain(extension(loaded_type, destination, loaded));
            return Ok(());
        }
        let (source, source_type) =
            self.expression_with_target(value, if converts { None } else { Some(destination) })?;
        if narrow && (source_type != variable_type || raw) {
            let raw_load = raw && !self.unoptimized && matches!(value.kind, ExprKind::Load { .. } | ExprKind::Global(_) | ExprKind::Convert(_));
            let (converted, _) = if raw_load && source_type == variable_type && !toggle("MWCC_PCODE_RAW_ASSIGN_UNEXTENDED") {
                // A raw value of the variable's own type is extended as it.
                self.emit_plain(extension(variable_type, destination, source));
                (destination, variable_type)
            } else {
                // (A raw narrower load widens as itself.)
                let raw = raw && matches!(value.kind, ExprKind::Load { .. } | ExprKind::Global(_));
                self.convert_value(source, source_type, raw, variable_type, Some(destination))?
            };
            if converted != destination {
                self.emit_plain(Instruction::Or { a: destination, s: converted, b: converted });
            }
            return Ok(());
        }
        if source != destination {
            self.copy(variable_type, destination, source);
        }
        Ok(())
    }

    // ------------------------------------------------------------ branches

    /// Branch to `label` when `condition` evaluates to `when`.
    /// Branch to `label` when `condition` evaluates to `when` (its values
    /// computed as tested ones).
    fn branch_on(&mut self, condition: &Expr, when: bool, label: Label) -> Compilation<()> {
        self.testing += 1;
        let result = self.branch_on_inner(condition, when, label);
        self.testing -= 1;
        result
    }

    fn branch_on_inner(&mut self, condition: &Expr, when: bool, label: Label) -> Compilation<()> {
        match &condition.kind {
            ExprKind::Binary(BinaryOp::LogicalAnd, left, right) => {
                if when {
                    let skip = self.new_label();
                    self.branch_on(left, false, skip)?;
                    self.branch_on(right, true, label)?;
                    self.place_label(skip);
                } else {
                    self.branch_on(left, false, label)?;
                    self.branch_on(right, false, label)?;
                }
                Ok(())
            }
            ExprKind::Binary(BinaryOp::LogicalOr, left, right) => {
                if when {
                    self.branch_on(left, true, label)?;
                    self.branch_on(right, true, label)?;
                } else {
                    let taken = self.new_label();
                    self.branch_on(left, true, taken)?;
                    self.branch_on(right, false, label)?;
                    self.place_label(taken);
                }
                Ok(())
            }
            ExprKind::Unary(UnaryOp::LogicalNot, operand) => self.branch_on(operand, !when, label),
            ExprKind::Binary(op, left, right) if op.is_comparison() => {
                let (bit, true_when_set) = self.compare(*op, left, right)?;
                // BO 12: branch if the bit is set; BO 4: if it is clear.
                let options = if when == true_when_set { 12 } else { 4 };
                self.branch(Instruction::BranchConditionalForward { options, condition_bit: bit, target: 0 }, label);
                Ok(())
            }
            // Truth of a wide value: `value != 0`.
            _ if is_wide(condition.ty) => {
                let zero = Expr { kind: ExprKind::Int(0), ty: condition.ty };
                let test = Expr::binary(BinaryOp::NotEqual, condition.clone(), zero, Type::Int);
                self.branch_on(&test, when, label)
            }
            // Truth of a floating value: `value != 0.0`.
            _ if is_float(condition.ty) => {
                let zero = Expr { kind: ExprKind::Float(0.0), ty: condition.ty };
                let test = Expr::binary(BinaryOp::NotEqual, condition.clone(), zero, Type::Int);
                self.branch_on(&test, when, label)
            }
            _ => {
                // Truth of a value: compare against zero.
                let (register, ty) = self.expression(condition)?;
                if !self.record_form(register) {
                    // An unsigned value compares logically.
                    // (An unsigned narrow value too.)
                    // (GC/3.x tests every word's truth signed.)
                    let unsigned = is_unsigned(promote(ty)) && !self.unit.signed_promoted_truth
                        || (is_unsigned_narrow(ty) && !toggle("MWCC_PCODE_NARROW_SIGNED_TRUTH"))
                        // (A raw unsigned narrow value extended for the test.)
                        || (is_unsigned_narrow(unpromoted(condition).ty)
                            && (self.is_raw(unpromoted(condition)) || !self.unit.signed_promoted_truth)
                            && !toggle("MWCC_PCODE_PROMOTED_SIGNED_TRUTH"));
                    self.emit_plain(if unsigned && std::env::var_os("MWCC_PCODE_SIGNED_TRUTH").is_none() {
                        Instruction::CompareLogicalWordImmediate { a: register, immediate: 0 }
                    } else {
                        Instruction::CompareWordImmediate { a: register, immediate: 0 }
                    });
                }
                let options = if when { 4 } else { 12 };
                self.branch(Instruction::BranchConditionalForward { options, condition_bit: 2, target: 0 }, label);
                Ok(())
            }
        }
    }

    /// Emit a compare for `left op right`; returns the cr0 bit to test and
    /// whether the relation holds when that bit is set.
    fn compare(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Compilation<(u8, bool)> {
        if is_wide(left.ty) || is_wide(right.ty) {
            return self.wide_compare(op, left, right);
        }
        if is_float(left.ty) || is_float(right.ty) {
            let (a, _) = self.expression(left)?;
            let (b, _) = self.expression(right)?;
            self.emit_plain(match op {
                BinaryOp::Equal | BinaryOp::NotEqual => Instruction::FloatCompareUnordered { a, b },
                _ => Instruction::FloatCompareOrdered { a, b },
            });
            // `<=` / `>=` fold the equal bit in: `cror eq,lt|gt,eq`.
            return Ok(match op {
                BinaryOp::LessEqual | BinaryOp::GreaterEqual => {
                    let a = if op == BinaryOp::LessEqual { 0 } else { 1 };
                    self.emit_plain(Instruction::ConditionRegisterOr { d: 2, a, b: 2 });
                    (2, true)
                }
                _ => comparison(op),
            });
        }
        // A constant on the left compares mirrored so it can be an immediate.
        let (op, left, right) = if left.as_int().is_some() && right.as_int().is_none() {
            (op.mirror(), right, left)
        } else if !self.unoptimized && equality_variable_first(op, left, right) {
            (op, right, left)
        } else {
            (op, left, right)
        };
        // (An operand that calls is evaluated first: the right one when both do.)
        let early = self.call_operand_first(right)?;
        let (a, left_type) = self.expression(left)?;
        let non_negative = right.as_int().map_or(is_unsigned_narrow(unpromoted(right).ty), |value| value >= 0);
        let unsigned = is_unsigned(promote(left_type))
            || is_unsigned(promote(right.ty))
            || is_unsigned_narrow(unpromoted(left).ty) && non_negative;
        // A compare of a just-computed value with 0 is its record form.
        let equality = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual);
        if right.as_int() == Some(0) && (!unsigned || equality) && early.is_none() && self.record_form(a) {
            return Ok(comparison(op));
        }
        // GC/3.x compares a word with zero for equality signed.
        let unsigned = unsigned && !(equality && right.as_int() == Some(0) && self.unit.signed_promoted_truth);
        match (right.as_int(), unsigned) {
            (Some(value), false) if i16::try_from(value).is_ok() => {
                self.emit_plain(Instruction::CompareWordImmediate { a, immediate: value as i16 });
            }
            (Some(value), true) if u16::try_from(value).is_ok() => {
                self.emit_plain(Instruction::CompareLogicalWordImmediate { a, immediate: value as u16 });
            }
            _ => {
                let b = match early {
                    Some(b) => b,
                    None => self.expression(right)?.0,
                };
                self.emit_plain(if unsigned {
                    Instruction::CompareLogicalWord { a, b }
                } else {
                    Instruction::CompareWord { a, b }
                });
            }
        }
        Ok(comparison(op))
    }

    /// A comparison's right operand evaluated ahead of the left when it
    /// calls (MWCC evaluates the operand with a call first, the right one
    /// when both do).
    fn call_operand_first(&mut self, right: &Expr) -> Compilation<Option<u32>> {
        if !contains_call(right) || toggle("MWCC_PCODE_COMPARE_IN_ORDER") {
            return Ok(None);
        }
        Ok(Some(self.expression(right)?.0))
    }

    /// Turn the current block's last instruction into its record form when it
    /// defines `register` (so cr0 compares that result with 0).
    fn record_form(&mut self, register: u32) -> bool {
        if std::env::var_os("MWCC_PCODE_NO_RECORD").is_some() || self.unoptimized {
            return false;
        }
        let block = self.current_block();
        let Some(last) = self.pcode.blocks[block].instructions.last_mut() else { return false };
        use Instruction::*;
        let record = match last.instruction.clone() {
            Add { d, a, b } if d == register => AddRecord { d, a, b },
            SubtractFrom { d, a, b } if d == register => SubtractFromRecord { d, a, b },
            Negate { d, a } if d == register => NegateRecord { d, a },
            Xor { a, s, b } if a == register => XorRecord { a, s, b },
            Or { a, s, b } if a == register && s != b => OrRecord { a, s, b },
            And { a, s, b } if a == register => AndRecord { a, s, b },
            ExtendSignByte { a, s } if a == register => ExtendSignByteRecord { a, s },
            ExtendSignHalfword { a, s } if a == register => ExtendSignHalfwordRecord { a, s },
            ClearLeftImmediate { a, s, clear } if a == register => ClearLeftImmediateRecord { a, s, clear },
            RotateAndMask { a, s, shift, begin, end } if a == register => RotateAndMaskRecord { a, s, shift, begin, end },
            ShiftRightAlgebraicImmediate { a, s, shift } if a == register => {
                ShiftRightAlgebraicImmediateRecord { a, s, shift }
            }
            AndImmediateRecord { a, .. } if a == register => return true,
            AddImmediate { d, a, immediate } if d == register && a != 0 => AddImmediateCarryingRecord { d, a, immediate },
            _ => return false,
        };
        last.instruction = record;
        true
    }

    // ------------------------------------------------------------ values

    fn expression_with_target(&mut self, expression: &Expr, target: Option<u32>) -> Compilation<(u32, Type)> {
        self.target = target;
        let evaluated = self.expression(expression);
        self.target = None;
        evaluated
    }

    /// Evaluate a word-sized integer/pointer expression into a register.
    fn expression(&mut self, expression: &Expr) -> Compilation<(u32, Type)> {
        let ExprKind::Load { base, .. } = &expression.kind else { return self.expression_body(expression) };
        // A load through a pointer reaching no volatile storage is marked
        // (offsets disambiguate it, GC/3.x).
        if !self.const_pointee(base) && self.shareable_pointer(base).is_some() {
            let (block, start) = (self.current_block(), self.pcode.blocks[self.current_block()].instructions.len());
            let (result, ty) = self.expression_body(expression)?;
            if self.current_block() == block {
                let instructions = &mut self.pcode.blocks[block].instructions[start..];
                if let Some(load) = instructions.iter_mut().rev().find(|instruction| load_destination(&instruction.instruction) == Some(result)) {
                    load.flags.nonvolatile_base = true;
                }
            }
            return Ok((result, ty));
        }
        if !self.const_pointee(base) {
            return self.expression_body(expression);
        }
        let (block, start) = (self.current_block(), self.pcode.blocks[self.current_block()].instructions.len());
        let (result, ty) = self.expression_body(expression)?;
        // A load through a `const T *` is ordered against no store.
        if self.current_block() == block {
            let instructions = &mut self.pcode.blocks[block].instructions[start..];
            if let Some(load) = instructions.iter_mut().rev().find(|instruction| {
                load_destination(&instruction.instruction) == Some(result)
            }) {
                load.flags.read_only = true;
            }
        }
        Ok((result, ty))
    }

    /// Whether an address is based on a `const T *` variable.
    fn const_pointee(&self, base: &Expr) -> bool {
        if toggle("MWCC_PCODE_CONST_LOADS_ORDERED") {
            return false;
        }
        match &base.kind {
            ExprKind::Var(id) => self.unit.const_pointers.contains(&self.function.variables[*id].name),
            ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, _) => self.const_pointee(left),
            _ => false,
        }
    }

    fn expression_body(&mut self, expression: &Expr) -> Compilation<(u32, Type)> {
        let ty = expression.ty;
        if is_wide(ty) {
            self.target = None;
            return Err(unsupported("a wide value in a word context"));
        }
        if let ExprKind::Convert(operand) = &expression.kind {
            if is_wide(operand.ty) {
                self.target = None;
                if is_float(ty) {
                    return Err(unsupported("a wide value converted to floating point"));
                }
                let (_, low) = self.wide(operand)?;
                // (Narrower, the low word converts like a word.)
                if is_narrow(ty) {
                    return self.convert_value(low, Type::UnsignedInt, false, ty, None);
                }
                return Ok((low, ty));
            }
        }
        // (A wide comparison's value; `!wide` is `wide == 0`.)
        match &expression.kind {
            ExprKind::Binary(op, left, right) if op.is_comparison() && (is_wide(left.ty) || is_wide(right.ty)) => {
                self.target = None;
                return Ok((self.wide_compare_value(*op, left, right)?, ty));
            }
            ExprKind::Unary(UnaryOp::LogicalNot, operand) if is_wide(operand.ty) => {
                self.target = None;
                let zero = Expr { kind: ExprKind::Int(0), ty: operand.ty };
                return Ok((self.wide_compare_value(BinaryOp::Equal, operand, &zero)?, ty));
            }
            _ => {}
        }
        let target = self.target.take();
        match &expression.kind {
            ExprKind::Float(value) => self.float_constant(*value, ty, target),
            ExprKind::StringAddress(index) => {
                // `@@strN` is resolved to the pooled `@N` per unit.
                let placeholder = || RelocationTarget::External(format!("@@str{index}"));
                if let Some(anchor) = self.anchored.get(&format!("@@str{index}")).copied() {
                    // `addi d,anchor,<offset of the literal>`
                    let base = self.anchor_register(anchor);
                    let d = self.result(target);
                    let mut addi = PInstr::new(Instruction::AddImmediate { d, a: base, immediate: 0 });
                    addi.not_r0.push(base);
                    addi.displacement_symbol = Some(format!("@@str{index}"));
                    self.emit(addi);
                    return Ok((d, ty));
                }
                if self.small_string(expression) {
                    let d = self.result(target);
                    let mut li = PInstr::new(Instruction::AddImmediate { d, a: 0, immediate: 0 });
                    li.relocation = Some(AttachedRelocation { kind: RelocationKind::EmbSda21, target: placeholder() });
                    self.emit(li);
                    return Ok((d, ty));
                }
                let high = self.temporary();
                let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
                lis.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Ha, target: placeholder() });
                self.emit(lis);
                let d = self.result(target);
                let mut addi = PInstr::new(Instruction::AddImmediate { d, a: high, immediate: 0 });
                addi.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: placeholder() });
                addi.not_r0.push(high);
                self.emit(addi);
                Ok((d, ty))
            }
            // A large constant image's address (`lis; addi` of its `.rodata`
            // blob); small ones are read through the pool by the block copy.
            ExprKind::Image(index) => {
                let image = &self.function.images[*index];
                if self.small_image(image.len()) {
                    return Err(unsupported("address of a small image"));
                }
                let blob = self.rodata_blob(*index);
                let high = self.temporary();
                let target_blob = || RelocationTarget::AnonymousRodataAt(blob);
                let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
                lis.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Ha, target: target_blob() });
                self.emit(lis);
                let d = self.result(target);
                let mut addi = PInstr::new(Instruction::AddImmediate { d, a: high, immediate: 0 });
                addi.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: target_blob() });
                addi.not_r0.push(high);
                self.emit(addi);
                Ok((d, ty))
            }
            ExprKind::LocalAddress(id) => {
                let offset = self.frame_offsets[*id].ok_or_else(|| unsupported("address of a register variable"))?;
                let key = format!("&v{id}");
                if target.is_none() && !toggle("MWCC_PCODE_NO_ADDRESS_CSE") {
                    if let Some(&(register, ty, _)) = self.common.get(&key) {
                        return Ok((register, ty));
                    }
                }
                let d = self.result(target);
                self.emit_plain(Instruction::AddImmediate { d, a: 1, immediate: offset });
                self.escaped_frame_objects.push(offset);
                if target.is_none() {
                    self.common.insert(key, (d, ty, Vec::new()));
                }
                Ok((d, ty))
            }
            ExprKind::Load { base, index: None, offset } if matches!(base.kind, ExprKind::LocalAddress(_)) => {
                let ExprKind::LocalAddress(id) = base.kind else { unreachable!() };
                // (Reused until the object is stored, or a call or store
                // could reach it.)
                let cached = !self.unoptimized && !toggle("MWCC_PCODE_NO_FRAME_LOAD_CSE") && target.is_none_or(|t| t >= 32);
                let kind = if is_general_word(ty) && !is_narrow(ty) { "word".to_owned() } else { format!("{ty:?}") };
                let key = format!("*F{id}:{offset}:{kind}");
                if cached && self.target_variable(target).is_none() {
                    if let Some(&(register, _, _)) = self.common.get(&key) {
                        return Ok((register, ty));
                    }
                }
                let slot = self.frame_offsets[id].ok_or_else(|| unsupported("frame slot"))?;
                let offset = i16::try_from(i32::from(slot) + *offset).map_err(|_| unsupported("a large frame"))?;
                let result = self.load(ty, 1, offset, None, target)?;
                if cached && result.0 >= 32 {
                    let read: Vec<VarId> = self.target_variable(target).into_iter().collect();
                    self.common.insert(key, (result.0, result.1, read));
                }
                Ok(result)
            }
            ExprKind::Binary(op, left, right) if op.is_comparison() && (is_float(left.ty) || is_float(right.ty)) => {
                self.float_comparison_value(*op, left, right, target)
            }
            ExprKind::Binary(op, left, right) if is_float(ty) => self.float_binary(*op, left, right, ty, target),
            ExprKind::Unary(UnaryOp::Negate, operand) if is_float(ty) => {
                let (source, _) = self.expression(operand)?;
                let d = self.result_for(ty, target);
                self.emit_plain(Instruction::FloatNegate { d, b: source });
                Ok((d, ty))
            }
            ExprKind::Convert(operand) if is_float(ty) || is_float(operand.ty) => self.float_convert(operand, ty, target),
            ExprKind::Int(value) => {
                if target.is_none() && !self.unoptimized && std::env::var_os("MWCC_PCODE_NO_CONSTANT_CSE").is_none() {
                    if let Some(&register) = self.constants.get(value) {
                        return Ok((register, ty));
                    }
                    let register = self.temporary();
                    self.load_constant(register, *value)?;
                    self.constants.insert(*value, register);
                    return Ok((register, ty));
                }
                let register = self.result(target);
                self.load_constant(register, *value)?;
                Ok((register, ty))
            }
            ExprKind::Var(id) if self.homes[*id].is_some() => {
                let offset = self.homes[*id].expect("checked");
                self.load(ty, 1, offset, None, target)
            }
            ExprKind::Var(id) => Ok((self.register(*id), ty)),
            ExprKind::Global(name) => {
                let global = self.unit.globals[name];
                let reuse = !global.is_volatile && !self.unoptimized;
                if reuse {
                    if let Some(&loaded) = self.loaded_globals.get(name) {
                        return Ok(loaded);
                    }
                }
                let loaded = self.load_global(name, global, target)?;
                if reuse {
                    self.loaded_globals.insert(name.clone(), loaded);
                }
                Ok(loaded)
            }
            ExprKind::GlobalAddress(name) => {
                // A repeated address reuses its register in this block.
                let key = format!("&@{name}");
                let shared = target.is_none() && !self.unoptimized && !toggle("MWCC_PCODE_NO_GLOBAL_ADDRESS_CSE");
                if shared {
                    if let Some(&(register, ty, _)) = self.common.get(&key) {
                        return Ok((register, ty));
                    }
                }
                let global = self.unit.globals[name];
                let external = || RelocationTarget::External(name.clone());
                let d = if let Some(anchor) = self.anchored.get(name).copied() {
                    // `addi d,anchor,<offset of the object>`
                    let base = self.anchor_register(anchor);
                    let d = self.result(target);
                    let mut addi = PInstr::new(Instruction::AddImmediate { d, a: base, immediate: 0 });
                    addi.not_r0.push(base);
                    addi.displacement_symbol = Some(name.clone());
                    self.emit(addi);
                    d
                } else if global.small_data {
                    let d = self.result(target);
                    let mut li = PInstr::new(Instruction::AddImmediate { d, a: 0, immediate: 0 });
                    li.relocation = Some(AttachedRelocation { kind: RelocationKind::EmbSda21, target: external() });
                    self.emit(li);
                    d
                } else {
                    // (GC/3.x keeps a memory base's halves in one register.)
                    let tied = self.unit.tied_halves && std::ptr::eq(expression, self.memory_base as *const Expr);
                    self.absolute_address_into(name, target, tied)
                };
                if shared {
                    self.common.insert(key, (d, ty, Vec::new()));
                }
                Ok((d, ty))
            }
            ExprKind::Binary(op, left, right) if op.is_comparison() => {
                self.comparison_value(*op, left, right, target)
            }
            ExprKind::Binary(op, left, right) => {
                // A repeated simple operation reuses its value in this block.
                let key = (!self.unoptimized && target.is_none()).then(|| common_key(expression)).flatten();
                if let Some((key, _)) = &key {
                    if let Some(&(register, ty, _)) = self.common.get(key) {
                        return Ok((register, ty));
                    }
                }
                self.addressing = std::ptr::eq(expression, self.memory_base as *const Expr);
                self.returning = std::ptr::eq(expression, self.returned as *const Expr);
                let result = self.binary(*op, left, right, ty, target)?;
                if let Some((key, variables)) = key {
                    self.common.insert(key, (result.0, result.1, variables));
                }
                Ok(result)
            }
            // A conditional value: each arm computed into one register on
            // its own path.
            ExprKind::Select { condition, when_true, when_false }
                if is_float(ty) == is_float(when_true.ty)
                    && is_float(ty) == is_float(when_false.ty)
                    && !toggle("MWCC_PCODE_NO_BRANCH_SELECT") =>
            {
                // (After the early builds, `c ? 1 : 0` is c's truth value.)
                if !self.unoptimized
                    && !self.unit.branch_preserving
                    && !is_float(ty)
                    && !is_float(condition.ty)
                    && when_true.as_int() == Some(1)
                    && when_false.as_int() == Some(0)
                    && !toggle("MWCC_PCODE_NO_SELECT_TRUTH")
                {
                    let truth = match &condition.kind {
                        ExprKind::Binary(op, ..) if op.is_comparison() => (**condition).clone(),
                        _ => Expr::binary(BinaryOp::NotEqual, (**condition).clone(), Expr::int(0), Type::Int),
                    };
                    let (value, _) = self.expression_with_target(&truth, target)?;
                    return Ok((value, ty));
                }
                // (After the early builds, `x < 0 ? -1 : 0` is the sign mask.)
                if !self.unoptimized && !self.unit.branch_preserving && ty == Type::Int && !toggle("MWCC_PCODE_NO_SIGN_SELECT") {
                    let sign_of = match (&condition.kind, when_true.as_int(), when_false.as_int()) {
                        (ExprKind::Binary(BinaryOp::Less, x, zero), Some(-1), Some(0))
                        | (ExprKind::Binary(BinaryOp::GreaterEqual, x, zero), Some(0), Some(-1))
                            if zero.as_int() == Some(0) && x.ty == Type::Int =>
                        {
                            Some(x)
                        }
                        _ => None,
                    };
                    if let Some(x) = sign_of {
                        let mask = Expr::binary(BinaryOp::ShiftRight, (**x).clone(), Expr::typed_int(31, Type::Int), Type::Int);
                        return self.expression_with_target(&mask, target);
                    }
                }
                let d = self.result_for(ty, target);
                // (An arm that is the destination itself: only the other
                // arm, under the condition that selects it.)
                let destination_variable = |e: &Expr| {
                    unpromoted(e).as_var().is_some_and(|id| self.registers[id] == Some(d))
                };
                if !self.unoptimized && !is_float(ty) && !toggle("MWCC_PCODE_NO_SELF_ARM") {
                    let kept = if destination_variable(when_true) {
                        Some((true, when_false))
                    } else if destination_variable(when_false) {
                        Some((false, when_true))
                    } else {
                        None
                    };
                    if let Some((skip_when, other)) = kept {
                        let join = self.new_label();
                        self.branch_on(condition, skip_when, join)?;
                        let (value, _) = self.expression_with_target(other, Some(d))?;
                        if value != d {
                            self.copy(ty, d, value);
                        }
                        self.place_label(join);
                        return Ok((d, ty));
                    }
                }
                // Optimized, one arm is computed before the branch: the
                // else arm, unless only the then arm is constant.
                // (Not floating selects, nor arms that call.)
                if !self.unoptimized
                    && !is_float(ty)
                    && !contains_call(when_true)
                    && !contains_call(when_false)
                    && !toggle("MWCC_PCODE_SELECT_TWO_PATHS")
                {
                    let constant = |e: &Expr| e.as_int().is_some() || matches!(e.kind, ExprKind::Float(_));
                    let then_first = constant(when_true) && !constant(when_false);
                    let (first, second) = if then_first { (when_true, when_false) } else { (when_false, when_true) };
                    // (Only a cheap arm is hoisted — a constant, a variable or
                    // one operation on those — and not past an arm that loads.)
                    let leaf = |e: &Expr| e.as_int().is_some() || unpromoted(e).as_var().is_some();
                    let cheap = |e: &Expr| match &e.kind {
                        ExprKind::Binary(_, a, b) => leaf(a) && leaf(b),
                        _ => leaf(e),
                    };
                    let loads = |e: &Expr| format!("{:?}", e.kind).contains("Load {") || format!("{:?}", e.kind).contains("Global(");
                    let both_variables = unpromoted(when_true).as_var().is_some() && unpromoted(when_false).as_var().is_some();
                    // (Nor when the condition or the other arm reads the
                    // destination's variable: the hoisted arm would clobber it.)
                    let reads_destination = |e: &Expr| {
                        (0..self.function.variables.len()).any(|id| self.registers[id] == Some(d) && e.mentions(id))
                    };
                    if !(cheap(first) && !loads(second)) || both_variables || reads_destination(condition) || reads_destination(second) {
                        return self.two_path_select(condition, when_true, when_false, ty, d);
                    }
                    let join = self.new_label();
                    let (value, _) = self.expression_with_target(first, Some(d))?;
                    if value != d {
                        self.copy(ty, d, value);
                    }
                    // (Skip the other arm when the hoisted one is the value.)
                    self.branch_on(condition, then_first, join)?;
                    let (value, _) = self.expression_with_target(second, Some(d))?;
                    if value != d {
                        self.copy(ty, d, value);
                    }
                    self.place_label(join);
                    return Ok((d, ty));
                }
                self.two_path_select(condition, when_true, when_false, ty, d)
            }
            ExprKind::Select { .. } => Err(unsupported("conditional expression")),
            ExprKind::Idiom(idiom) => self.idiom(idiom, target),
            // (GC/1.0-1.2.5n: `!x` counts x's zeros directly.)
            ExprKind::Unary(UnaryOp::LogicalNot, operand)
                if self.unit.branch_preserving && !matches!(&operand.kind, ExprKind::Binary(op, ..) if op.is_comparison()) =>
            {
                let (value, _) = self.expression(operand)?;
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: value });
                self.shift_out(n, 5, target)
            }
            ExprKind::Unary(UnaryOp::LogicalNot, operand) => {
                let inverted = match &operand.kind {
                    ExprKind::Binary(op, left, right) if op.is_comparison() => {
                        Expr::binary(op.invert(), (**left).clone(), (**right).clone(), Type::Int)
                    }
                    _ => Expr::binary(BinaryOp::Equal, (**operand).clone(), Expr::int(0), Type::Int),
                };
                self.expression_with_target(&inverted, target)
            }
            // `-(x REL 0)`: the relation's sign mask.
            ExprKind::Unary(UnaryOp::Negate, operand)
                if matches!(&operand.kind, ExprKind::Binary(relation, x, zero)
                    if matches!(relation, BinaryOp::NotEqual | BinaryOp::Equal | BinaryOp::Greater | BinaryOp::LessEqual | BinaryOp::GreaterEqual | BinaryOp::Less)
                        && zero.as_int() == Some(0)
                        && x.ty == Type::Int)
                    && !self.unoptimized
                    && !toggle("MWCC_PCODE_NO_RELATION_MASK") =>
            {
                let ExprKind::Binary(relation, x, _) = &operand.kind else { unreachable!() };
                let (a, _) = self.expression(x)?;
                let d = self.result(target);
                self.relation_mask(*relation, a, d);
                Ok((d, Type::Int))
            }
            // `~(a | b)`, `~(a & b)`, `~(a ^ b)`: `nor`, `nand`, `eqv`.
            ExprKind::Unary(UnaryOp::BitNot, operand)
                if matches!(&operand.kind, ExprKind::Binary(op @ (BinaryOp::BitOr | BinaryOp::BitAnd | BinaryOp::BitXor), left, right)
                    if (right.as_int().is_none() || *op == BinaryOp::BitOr) && left.as_int().is_none())
                    && !toggle("MWCC_PCODE_NO_NOR") =>
            {
                let ExprKind::Binary(op, left, right) = &operand.kind else { unreachable!() };
                let (a, _) = self.expression(left)?;
                let (b, _) = self.expression(right)?;
                let destination = self.result(target);
                self.emit_plain(match op {
                    BinaryOp::BitOr => Instruction::Nor { a: destination, s: a, b },
                    BinaryOp::BitAnd => Instruction::Nand { a: destination, s: a, b },
                    _ => Instruction::Eqv { a: destination, s: a, b },
                });
                Ok((destination, ty))
            }
            ExprKind::Unary(op, operand) => {
                let (source, _) = self.expression(operand)?;
                let destination = self.result(target);
                self.emit_plain(match op {
                    UnaryOp::Negate => Instruction::Negate { d: destination, a: source },
                    _ => Instruction::Nor { a: destination, s: source, b: source },
                });
                Ok((destination, ty))
            }
            ExprKind::Load { base, index: None, offset } if base.as_int().is_some() => {
                let (high, low) = split_address(base.as_int().expect("checked") + i64::from(*offset))?;
                let a = self.address_high(high)?;
                self.load(ty, a, low, None, target)
            }
            // A load through a pointer to non-volatile storage is reused
            // until a store or call (or the pointer changes).
            // (Loaded into a variable, it is that variable's value until
            // the variable changes.)
            ExprKind::Load { base, index: None, offset }
                if self.shareable_pointer(base).is_some()
                    && (target.is_none()
                        || self.target_variable(target).is_some()
                        // (A virtual destination holds it as well.)
                        || (target.is_some_and(|t| t >= 32) && !self.unoptimized && !toggle("MWCC_PCODE_NO_VIRTUAL_TARGET_LOAD_CSE"))
                        || (self.common.contains_key(&load_key(self.shareable_pointer(base).expect("checked"), *offset, ty))
                            && !toggle("MWCC_PCODE_NO_TARGETED_LOAD_CSE"))) =>
            {
                let id = self.shareable_pointer(base).expect("checked");
                let key = load_key(id, *offset, ty);
                if self.target_variable(target).is_none() {
                    if let Some(&(register, ty, _)) = self.common.get(&key) {
                        return Ok((register, ty));
                    }
                }
                let variable = self.target_variable(target);
                let (a, _) = self.expression(base)?;
                let (a, displacement) = self.displacement(a, *offset)?;
                let result = self.load(ty, a, displacement, None, target)?;
                // (Not when it lands in a physical register: that one is
                // reused freely.)
                if result.0 >= 32 {
                    let mut read = vec![id];
                    read.extend(variable);
                    self.common.insert(key, (result.0, result.1, read));
                }
                Ok(result)
            }
            // The value of `a[i] op= v` loads through the store's address.
            ExprKind::Load { base, index, offset }
                if self.address_reuse.as_ref().is_some_and(|(key, _)| *key == place_key(base, index.as_deref(), *offset)) =>
            {
                let (_, (a, displacement, b, relocation)) = self.address_reuse.clone().expect("checked");
                match b {
                    Some(b) => self.indexed_load(ty, a, b, target),
                    None => self.load(ty, a, displacement, relocation, target),
                }
            }
            // A small-data object's first member loads through `@sda21`.
            ExprKind::Load { base, index: None, offset: 0 } if self.anchored_object(base).is_some() => {
                let (name, anchor) = self.anchored_object(base).expect("checked");
                self.anchored_load(ty, anchor, &name, target)
            }
            ExprKind::Load { base, index: None, offset: 0 } if self.small_data_object(base).is_some() => {
                let name = self.small_data_object(base).expect("checked").to_owned();
                let relocation = AttachedRelocation { kind: RelocationKind::EmbSda21, target: RelocationTarget::External(name) };
                self.load(ty, 0, 0, Some(relocation), target)
            }
            ExprKind::Load { base, index: None, offset } if self.unoptimized && self.unoptimized_indexed(base).is_some() => {
                let (a, b, displacement) = self.unoptimized_address(base, *offset)?;
                match b {
                    Some(b) => self.indexed_load(ty, a, b, target),
                    None => self.load(ty, a, i16::try_from(displacement).map_err(|_| unsupported("large member offset"))?, None, target),
                }
            }
            ExprKind::Load { base, index, offset } => {
                let result = self.memory_load(base, index.as_deref(), *offset, ty, target)?;
                // (An access inside a known global names that object.)
                if let Some(object) = root_global(base).filter(|_| !self.unit.early_frame && !toggle("MWCC_PCODE_NO_GLOBAL_OBJECTS")) {
                    self.tag_access(result.0, object);
                }
                Ok(result)
            }
            ExprKind::Convert(operand)
                if is_unsigned_narrow(ty)
                    // (-O0 narrows a bit-field's value regardless.)
                    && !(self.unoptimized && bit_field_extraction(operand) && !toggle("MWCC_PCODE_O0_FITTING_FIELDS"))
                    && {
                        SUM_BOUNDS.with(|flag| flag.set(self.unit.branch_preserving));
                        max_value(operand).is_some_and(|max| max < 1u64 << (8 * width(ty)))
                    } => {
                let (source, _) = self.expression_with_target(operand, target)?;
                Ok((source, ty))
            }
            // `(u8)(x >> n)`: one rotate keeping the low bits.
            ExprKind::Convert(operand)
                if is_unsigned_narrow(ty)
                    && !self.unoptimized
                    && matches!(&operand.kind, ExprKind::Binary(BinaryOp::ShiftRight, x, n)
                        if x.ty == Type::UnsignedInt && n.as_int().is_some_and(|n| (1..32).contains(&n)))
                    && !toggle("MWCC_PCODE_NO_NARROW_SHIFT_ROTATE") =>
            {
                let ExprKind::Binary(_, x, n) = &operand.kind else { unreachable!() };
                let n = n.as_int().unwrap_or(0) as u8;
                let (x, _) = self.expression(x)?;
                let d = self.result(target);
                let bits = 8 * width(ty) as u8;
                self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift: 32 - n, begin: 32 - bits, end: 31 });
                Ok((d, ty))
            }
            ExprKind::Convert(operand) => {
                // A promotion of a raw narrow parameter: extended once per block.
                if let ExprKind::Var(id) = operand.kind {
                    if self.raw_narrow[id] && !is_narrow(ty) {
                        if let Some(&extended) = self.extended.get(&id).filter(|_| !self.unoptimized) {
                            return Ok((extended, ty));
                        }
                        // -O0: right after the extension that set it, the
                        // variable is read as it is.
                        if self.unoptimized {
                            let variable_register = self.register(id);
                            let block = self.current_block();
                            if self.pcode.blocks[block].instructions.last().is_some_and(|last| {
                                matches!(last.instruction,
                                    Instruction::ExtendSignHalfword { a, .. } | Instruction::ExtendSignByte { a, .. }
                                    | Instruction::ClearLeftImmediate { a, .. } if a == variable_register)
                            }) {
                                return Ok((variable_register, ty));
                            }
                        }
                        let raw = self.register(id);
                        let extended = self.temporary();
                        self.emit_plain(extension(operand.ty, extended, raw));
                        self.extended.insert(id, extended);
                        return Ok((extended, ty));
                    }
                }
                let raw = self.is_raw(operand);
                // A signed byte load extends in place, one value in two
                // instructions (`lbz rD; extsb rD,rD`).
                if raw
                    && operand.ty == Type::Char
                    && !is_narrow(ty)
                    && matches!(operand.kind, ExprKind::Load { .. } | ExprKind::Global(_))
                    // Into a destination register (a returned value).
                    && (target.is_some() || std::env::var_os("MWCC_PCODE_INPLACE_EXTSB").is_some())
                    && std::env::var_os("MWCC_PCODE_NO_INPLACE_EXTSB").is_none()
                {
                    let (source, _) = self.expression_with_target(operand, target)?;
                    self.emit_plain(Instruction::ExtendSignByte { a: source, s: source });
                    return Ok((source, ty));
                }
                // A widening that emits nothing (of a word, or of a narrow
                // load already extended) computes into the target.
                if target.is_some()
                    && is_general_word(ty)
                    && !is_narrow(ty)
                    && ((is_general_word(operand.ty) && !is_narrow(operand.ty))
                        || (is_narrow(operand.ty) && !raw && matches!(operand.kind, ExprKind::Load { .. } | ExprKind::Global(_)))
                        // (A narrowing conversion leaves its value extended.)
                        || (is_narrow(operand.ty) && !raw && matches!(&operand.kind, ExprKind::Convert(inner) if !is_float(inner.ty))
                            && !toggle("MWCC_PCODE_NO_NARROWED_TARGET")))
                    && !toggle("MWCC_PCODE_NO_CONVERT_TARGET")
                {
                    let (source, _) = self.expression_with_target(operand, target)?;
                    return Ok((source, ty));
                }
                let (source, source_type) = self.expression(operand)?;
                // (Only a loaded byte is already zero-extended: a raw
                // parameter's register is not.)
                if raw
                    && source_type == Type::Char
                    && ty == Type::UnsignedChar
                    && !matches!(operand.kind, ExprKind::Load { .. } | ExprKind::Global(_))
                {
                    let destination = self.result(target);
                    self.emit_plain(extension(Type::UnsignedChar, destination, source));
                    return Ok((destination, ty));
                }
                self.convert_value(source, source_type, raw, ty, target)
            }
            ExprKind::Call { name, arguments } => {
                let register = self.call(name, arguments, ty, target)?;
                Ok((register, ty))
            }
        }
    }

    /// All ones when `a REL 0` holds, else zero.
    fn relation_mask(&mut self, relation: BinaryOp, a: u32, mask: u32) {
        match relation {
            BinaryOp::Less => {
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: a, shift: 31 });
            }
            BinaryOp::Greater | BinaryOp::LessEqual => {
                let negated = self.temporary();
                self.emit_plain(Instruction::Negate { d: negated, a });
                let combined = self.temporary();
                self.emit_plain(if relation == BinaryOp::Greater {
                    Instruction::AndComplement { a: combined, s: negated, b: a }
                } else {
                    Instruction::OrComplement { a: combined, s: a, b: negated }
                });
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: combined, shift: 31 });
            }
            BinaryOp::NotEqual => {
                let negated = self.temporary();
                self.emit_plain(Instruction::Negate { d: negated, a });
                let combined = self.temporary();
                self.emit_plain(Instruction::Or { a: combined, s: negated, b: a });
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: combined, shift: 31 });
            }
            BinaryOp::Equal => {
                let zeros = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: zeros, s: a });
                let flag = self.temporary();
                self.emit_plain(Instruction::RotateAndMask { a: flag, s: zeros, shift: 27, begin: 31, end: 31 });
                self.emit_plain(Instruction::Negate { d: mask, a: flag });
            }
            BinaryOp::GreaterEqual => {
                let sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: a, shift: 31 });
                self.emit_based(Instruction::AddImmediate { d: mask, a: sign, immediate: -1 }, sign);
            }
            _ => unreachable!("sign mask relations"),
        }
    }

    /// A select as two paths, each arm computed into `d`.
    fn two_path_select(&mut self, condition: &Expr, when_true: &Expr, when_false: &Expr, ty: Type, d: u32) -> Compilation<(u32, Type)> {
        let otherwise = self.new_label();
        let join = self.new_label();
        self.branch_on(condition, false, otherwise)?;
        let (value, _) = self.expression_with_target(when_true, Some(d))?;
        if value != d {
            self.copy(ty, d, value);
        }
        self.jump(join);
        self.place_label(otherwise);
        let (value, _) = self.expression_with_target(when_false, Some(d))?;
        if value != d {
            self.copy(ty, d, value);
        }
        self.place_label(join);
        Ok((d, ty))
    }

    fn convert(&mut self, source: u32, from: Type, to: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        self.convert_value(source, from, false, to, target)
    }

    /// Convert a value of `from` (a raw, unextended narrow value when `raw`)
    /// to `to`: a narrowing extends unless the type already matches; a
    /// widening extends only a raw value.
    fn convert_value(
        &mut self,
        source: u32,
        from: Type,
        raw: bool,
        to: Type,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        let zero_extended_load = raw && from == Type::Char && to == Type::UnsignedChar;
        let extend_as = if is_narrow(to) && is_narrow(from) && raw && mwcc_iro::width(from) < mwcc_iro::width(to) {
            // A raw narrower value extends as itself (`s8` to `s16`: extsb).
            Some(from)
        } else if is_narrow(to) {
            (from != to && !zero_extended_load).then_some(to)
        } else if is_general_word(to) && is_narrow(from) && raw {
            Some(from)
        } else {
            None
        };
        let Some(extend_as) = extend_as else { return Ok((source, to)) };
        let destination = self.result(target);
        self.emit_plain(extension(extend_as, destination, source));
        Ok((destination, to))
    }

    /// Whether a narrow expression's register holds its value unextended: a
    /// signed byte load (`lbz`) or a parameter used as it arrived.
    fn is_raw(&self, expression: &Expr) -> bool {
        match &expression.kind {
            ExprKind::Load { .. } | ExprKind::Global(_) => expression.ty == Type::Char,
            // A narrow call result is extended by the caller.
            ExprKind::Call { .. } => is_narrow(expression.ty) && !toggle("MWCC_PCODE_EXTENDED_CALL_RESULTS"),
            ExprKind::Var(id) => self.raw_narrow[*id] || self.homes[*id].is_some() && expression.ty == Type::Char,
            // A same-type conversion is a no-op: still raw.
            ExprKind::Convert(operand) if operand.ty == expression.ty => self.is_raw(operand),
            // A floating value converted to a narrow type is its raw word.
            ExprKind::Convert(operand) => is_float(operand.ty) && is_narrow(expression.ty),
            _ => false,
        }
    }

    fn load_constant(&mut self, register: u32, value: i64) -> Compilation<()> {
        let value = i32::try_from(value)
            .or_else(|_| u32::try_from(value).map(|value| value as i32))
            .map_err(|_| unsupported("64-bit constant"))?;
        if let Ok(small) = i16::try_from(value) {
            self.emit_plain(Instruction::AddImmediate { d: register, a: 0, immediate: small });
            return Ok(());
        }
        let low = value as i16;
        let high = ((value - i32::from(low)) >> 16) as i16;
        self.emit_plain(Instruction::AddImmediateShifted { d: register, a: 0, immediate: high });
        if low != 0 {
            let mut addi = PInstr::new(Instruction::AddImmediate { d: register, a: register, immediate: low });
            addi.not_r0.push(register);
            self.emit(addi);
        }
        Ok(())
    }

    fn indexed_load(&mut self, ty: Type, a: u32, b: u32, target: Option<u32>) -> Compilation<(u32, Type)> {
        // A signed byte loads raw (`lbz`); promotion extends it.
        let extends = false;
        let d = if extends { self.temporary() } else { self.result_for(ty, target) };
        let instruction = match ty {
            Type::Float => Instruction::LoadFloatSingleIndexed { d, a, b },
            Type::Double => Instruction::LoadFloatDoubleIndexed { d, a, b },
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                Instruction::LoadWordIndexed { d, a, b }
            }
            Type::Short => Instruction::LoadHalfwordAlgebraicIndexed { d, a, b },
            Type::UnsignedShort => Instruction::LoadHalfwordZeroIndexed { d, a, b },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZeroIndexed { d, a, b },
            other => return Err(unsupported(format!("load of {other:?}"))),
        };
        let mut load = PInstr::new(instruction);
        load.not_r0.push(a);
        self.emit(load);
        if extends {
            let extended = self.result(target);
            self.emit_plain(Instruction::ExtendSignByte { a: extended, s: d });
            return Ok((extended, ty));
        }
        Ok((d, ty))
    }

    /// A load from memory at `base` (+ `index`) + `offset`.
    fn memory_load(&mut self, base: &Expr, index: Option<&Expr>, offset: i32, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
            if let (Some(index), true) = (index, self.unoptimized) {
                // -O0: the index first; an absolute array's address is
                // completed with add, then accessed at 0.
                // (A loaded or variable base comes first; an absolute
                // array's index first.)
                if !matches!(base.kind, ExprKind::GlobalAddress(_) | ExprKind::Binary(..)) && !toggle("MWCC_PCODE_O0_INDEX_FIRST") {
                    let (a, _) = self.expression(base)?;
                    let (b, _) = self.expression(index)?;
                    return self.indexed_load(ty, a, b, target);
                }
                if let Some((pointer, addend)) = self.member_array(base) {
                    let (b, _) = self.expression(index)?;
                    // (GC/3.x adds the index to the pointer, the member
                    // offset staying the displacement.)
                    if self.unit.signed_promoted_truth && !toggle("MWCC_PCODE_O0_DISPLACED_INDEX") {
                        let (a, _) = self.expression(pointer)?;
                        let sum = self.temporary();
                        self.emit_plain(Instruction::Add { d: sum, a, b });
                        return self.load(ty, sum, addend, None, target);
                    }
                    let displaced = self.temporary();
                    self.emit_based(Instruction::AddImmediate { d: displaced, a: b, immediate: addend }, b);
                    let (a, _) = self.expression(pointer)?;
                    return self.indexed_load(ty, a, displaced, target);
                }
                let (b, _) = self.expression(index)?;
                let (a, _) = self.expression(base)?;
                // (GC/3.x indexes it: `lhax`.)
                if self.absolute_base(base) && (!self.unit.signed_promoted_truth || toggle("MWCC_PCODE_O0_ABSOLUTE_ADD")) {
                    let address = self.temporary();
                    self.emit_plain(Instruction::Add { d: address, a, b });
                    return self.load(ty, address, 0, None, target);
                }
                return self.indexed_load(ty, a, b, target);
            }
            // (-O1/-O2: an absolute array's index first.)
            if let Some(index) = index.filter(|_| {
                !self.unit.strength_reduction && self.absolute_base(base) && !toggle("MWCC_PCODE_O2_ADDRESS_FIRST")
            }) {
                let (b, _) = self.expression(index)?;
                let (a, _) = self.base_expression(base)?;
                return self.indexed_load(ty, a, b, target);
            }
            let (a, _) = self.base_expression(base)?;
            match index {
                Some(index) => {
                    let (b, _) = self.expression(index)?;
                    self.indexed_load(ty, a, b, target)
                }
                None => {
                    let (a, offset) = self.displacement(a, offset)?;
                    self.load(ty, a, offset, None, target)
                }
            }
            }

    /// Name the object the last access (loading `d`) touches.
    fn tag_access(&mut self, d: u32, object: String) {
        let block = self.current_block();
        if let Some(last) = self.pcode.blocks[block].instructions.last_mut() {
            let name = format!("{:?}", last.instruction);
            if name.starts_with("Load") && last.relocation.is_none() && last.displacement_symbol.is_none() && last.object.is_none()
                && last.defs(mwcc_pcode::Class::General).contains(&d) | last.defs(mwcc_pcode::Class::Float).contains(&d)
            {
                last.object = Some(object);
            }
        }
    }

    /// The register variable `target` is (when loads into it are reusable).
    fn target_variable(&self, target: Option<u32>) -> Option<VarId> {
        let target = target?;
        if self.unoptimized || toggle("MWCC_PCODE_NO_VARIABLE_LOAD_CSE") {
            return None;
        }
        (0..self.function.variables.len()).find(|&id| self.registers[id] == Some(target) && self.homes[id].is_none())
    }

    fn load(
        &mut self,
        ty: Type,
        base: u32,
        offset: i16,
        relocation: Option<AttachedRelocation>,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        let d = self.result_for(ty, target);
        let load = match ty {
            Type::Float => Instruction::LoadFloatSingle { d, a: base, offset },
            Type::Double => Instruction::LoadFloatDouble { d, a: base, offset },
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                Instruction::LoadWord { d, a: base, offset }
            }
            Type::Short => Instruction::LoadHalfwordAlgebraic { d, a: base, offset },
            Type::UnsignedShort => Instruction::LoadHalfwordZero { d, a: base, offset },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZero { d, a: base, offset },
            other => return Err(unsupported(format!("load of {other:?}"))),
        };
        let mut instruction = PInstr::new(load);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        Ok((d, ty))
    }

    fn load_global(&mut self, name: &str, global: GlobalInfo, target: Option<u32>) -> Compilation<(u32, Type)> {
        let external = || RelocationTarget::External(name.to_owned());
        if global.small_data {
            let relocation = AttachedRelocation { kind: RelocationKind::EmbSda21, target: external() };
            return self.load(global.ty, 0, 0, Some(relocation), target);
        }
        if let Some(anchor) = self.anchored.get(name).copied() {
            return self.anchored_load(global.ty, anchor, name, target);
        }
        let address = self.global_address(name);
        self.load(global.ty, address, 0, None, target)
    }

    /// The high half of a constant address, as a memory base.
    fn address_high(&mut self, high: i16) -> Compilation<u32> {
        // (An address within the low 32 KiB needs no base: `lwz r,d(0)`.)
        if high == 0 && !self.unoptimized && !toggle("MWCC_PCODE_ZERO_ADDRESS_BASE") {
            return Ok(0);
        }
        let value = i64::from(high) << 16;
        if !self.address_bases.contains(&value) {
            self.address_bases.push(value);
        }
        Ok(self.expression(&Expr::int(value))?.0)
    }

    /// A global's absolute address (`lis; addi`), reused within a block
    /// by its loads and stores.
    fn global_address(&mut self, name: &str) -> u32 {
        let key = format!("&{name}");
        let shared = !self.unoptimized && !self.unit.absolute_low_folds && !toggle("MWCC_PCODE_NO_GLOBAL_ADDRESS_REUSE");
        if shared {
            if let Some(&(register, _, _)) = self.common.get(&key) {
                return register;
            }
        }
        let d = self.absolute_address(name);
        if shared {
            self.common.insert(key, (d, Type::Pointer(mwcc_iro::Pointee::Int), Vec::new()));
        }
        d
    }

    /// `lis; addi` forming an absolute symbol's address.
    fn absolute_address(&mut self, name: &str) -> u32 {
        self.absolute_address_into(name, None, false)
    }

    fn absolute_address_into(&mut self, name: &str, target: Option<u32>, tied: bool) -> u32 {
        let external = || RelocationTarget::External(name.to_owned());
        let high = if tied { self.result(target) } else { self.temporary() };
        let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
        lis.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Ha, target: external() });
        self.emit(lis);
        let d = if tied { high } else { self.result(target) };
        let mut addi = PInstr::new(Instruction::AddImmediate { d, a: high, immediate: 0 });
        addi.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: external() });
        addi.flags.in_place = tied;
        addi.not_r0.push(high);
        self.emit(addi);
        d
    }

    // ------------------------------------------------------------ floating point

    /// A floating constant: loaded from the pool (`lfs fD,@N@sda21(r0)`).
    fn float_constant(&mut self, value: f64, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let key = if ty == Type::Float { (u64::from((value as f32).to_bits()), 4u8) } else { (value.to_bits(), 8u8) };
        if target.is_none() && !self.unoptimized {
            if let Some(&register) = self.float_constants.get(&key) {
                return Ok((register, ty));
            }
        }
        let index = match self.pcode.pool.iter().position(|&entry| entry == key) {
            Some(index) => index,
            None => {
                self.pcode.pool.push(key);
                self.pcode.pool.len() - 1
            }
        };
        let d = self.result_for(ty, target);
        if !self.unit.pool_small_data && self.constants_anchored {
            // `lfs f,<offset>(anchor)` off the `...rodata.0` base.
            let base = self.anchor_register("...rodata.0");
            let mut load = PInstr::new(if key.1 == 4 {
                Instruction::LoadFloatSingle { d, a: base, offset: 0 }
            } else {
                Instruction::LoadFloatDouble { d, a: base, offset: 0 }
            });
            load.not_r0.push(base);
            load.flags.read_only = true;
            load.displacement_symbol = Some(format!("@@const{index}"));
            self.emit(load);
            if target.is_none() {
                self.float_constants.insert(key, d);
            }
            return Ok((d, ty));
        }
        if !self.unit.pool_small_data {
            // `lis r,@N@ha; lfs f,@N@l(r)`.
            let pool = || RelocationTarget::Constant(index);
            let high = self.temporary();
            let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
            lis.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Ha, target: pool() });
            self.emit(lis);
            if self.unoptimized {
                // Unfolded at -O0: `lis; addi; lfs 0`.
                let mut addi = PInstr::new(Instruction::AddImmediate { d: high, a: high, immediate: 0 });
                addi.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: pool() });
                addi.not_r0.push(high);
                self.emit(addi);
                self.emit_based(
                    if key.1 == 4 {
                        Instruction::LoadFloatSingle { d, a: high, offset: 0 }
                    } else {
                        Instruction::LoadFloatDouble { d, a: high, offset: 0 }
                    },
                    high,
                );
                return Ok((d, ty));
            }
            let mut load = PInstr::new(if key.1 == 4 {
                Instruction::LoadFloatSingle { d, a: high, offset: 0 }
            } else {
                Instruction::LoadFloatDouble { d, a: high, offset: 0 }
            });
            load.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: pool() });
            load.not_r0.push(high);
            self.emit(load);
            if target.is_none() {
                self.float_constants.insert(key, d);
            }
            return Ok((d, ty));
        }
        let mut load = PInstr::new(if key.1 == 4 {
            Instruction::LoadFloatSingle { d, a: 0, offset: 0 }
        } else {
            Instruction::LoadFloatDouble { d, a: 0, offset: 0 }
        });
        load.relocation = Some(AttachedRelocation { kind: RelocationKind::EmbSda21, target: RelocationTarget::Constant(index) });
        self.emit(load);
        if target.is_none() {
            self.float_constants.insert(key, d);
        }
        Ok((d, ty))
    }

    fn float_binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let single = ty == Type::Float;
        fn product(e: &Expr, ty: Type) -> Option<(&Expr, &Expr)> {
            match &e.kind {
                ExprKind::Binary(BinaryOp::Multiply, x, y) if e.ty == ty => Some((x.as_ref(), y.as_ref())),
                _ => None,
            }
        }
        if self.contract {
            // Multiply-add contraction: x*y + z, z + x*y, x*y - z, z - x*y.
            let fused = match op {
                BinaryOp::Add => product(left, ty).map(|p| (p, right, 0)).or_else(|| product(right, ty).map(|p| (p, left, 0))),
                BinaryOp::Subtract => {
                    product(left, ty).map(|p| (p, right, 1)).or_else(|| product(right, ty).map(|p| (p, left, 2)))
                }
                _ => None,
            };
            if let Some(((x, y), z, form)) = fused {
                let (a, _) = self.expression(x)?;
                let (c, _) = self.expression(y)?;
                let (b, _) = self.expression(z)?;
                let d = self.result_for(ty, target);
                self.emit_plain(match (form, single) {
                    (0, true) => Instruction::FloatMultiplyAddSingle { d, a, c, b },
                    (0, false) => Instruction::FloatMultiplyAddDouble { d, a, c, b },
                    (1, true) => Instruction::FloatMultiplySubtractSingle { d, a, c, b },
                    (1, false) => Instruction::FloatMultiplySubtractDouble { d, a, c, b },
                    (_, true) => Instruction::FloatNegativeMultiplySubtractSingle { d, a, c, b },
                    (_, false) => Instruction::FloatNegativeMultiplySubtractDouble { d, a, c, b },
                });
                return Ok((d, ty));
            }
        }
        // (A constant right operand of a commutative operation, which goes
        // first, is loaded first too: it enters the pool before the other
        // operand's constants.)
        // (-O0 too, unless the other operand is loaded: it then goes first.)
        // (-O0 keeps a compound update's loaded value first: `s->f += k`.)
        let compound_update = self.unoptimized && self.compound_store && matches!(left.kind, ExprKind::Load { .. });
        let o0_constant_first = self.unoptimized
            && !compound_update
            && !(toggle("MWCC_PCODE_O0_LOADED_CONSTANT_LATE")
                && (matches!(left.kind, ExprKind::Load { .. }) || left.as_var().is_some_and(|id| self.homes[id].is_some())))
            && !toggle("MWCC_PCODE_O0_CONSTANT_LATE");
        let constant_first = matches!(op, BinaryOp::Add | BinaryOp::Multiply)
            && (!self.unoptimized || o0_constant_first)
            && !contains_call(left)
            && matches!(right.kind, ExprKind::Float(_))
            && !matches!(left.kind, ExprKind::Float(_))
            && !toggle("MWCC_PCODE_CONSTANT_OPERAND_IN_ORDER");
        // (-O0: of two results, the one needing fewer registers first.)
        let need_first = self.unoptimized
            && matches!(op, BinaryOp::Add | BinaryOp::Multiply)
            && matches!(left.kind, ExprKind::Binary(..))
            && matches!(right.kind, ExprKind::Binary(..))
            && register_need(left) > register_need(right)
            && !toggle("MWCC_PCODE_NO_FLOAT_NEED_ORDER");
        // (An operand that calls is evaluated first.)
        let (a, b) = if constant_first
            || need_first
            || contains_call(right)
                && (!contains_call(left) || !toggle("MWCC_PCODE_BOTH_CALLS_IN_ORDER"))
                && !toggle("MWCC_PCODE_CALL_OPERAND_IN_ORDER")
        {
            let (b, _) = self.expression(right)?;
            let (a, _) = self.expression(left)?;
            (a, b)
        } else {
            let (a, _) = self.expression(left)?;
            let (b, _) = self.expression(right)?;
            (a, b)
        };
        // A computed right operand of a commutative operation goes first
        // (a loaded one keeps source order).
        let commutative = matches!(op, BinaryOp::Add | BinaryOp::Multiply);
        let loaded = matches!(&right.kind, ExprKind::Load { base, index: None, .. }
                if matches!(base.kind, ExprKind::Var(_) | ExprKind::GlobalAddress(_) | ExprKind::LocalAddress(_)))
            && std::env::var_os("MWCC_PCODE_FLOAT_LOAD_FIRST").is_none();
        // A constant goes first; a computed right operand goes first (a
        // loaded one keeps source order); a variable goes before a
        // computed binary left operand.
        let refined = !toggle("MWCC_PCODE_NO_FLOAT_VARIABLE_FIRST");
        let constant = |e: &Expr| match &e.kind {
            ExprKind::Float(_) => true,
            ExprKind::Global(name) => self.unit.globals.get(name).is_some_and(|global| global.is_const),
            ExprKind::Convert(inner) => matches!(inner.kind, ExprKind::Float(_)),
            _ => false,
        };
        let one_calls = contains_call(left) != contains_call(right) && !toggle("MWCC_PCODE_CALL_OPERAND_IN_ORDER");
        let swap = commutative
            && if one_calls {
                // (A call's result goes second.)
                contains_call(left)
            } else if refined && constant(left) {
                false
            // (-O0: a constant follows an operand loaded from memory, and
            // the variable of a self-update `x = x + k`.)
            } else if refined
                && constant(right)
                && !(self.unoptimized
                    && (compound_update
                        || (toggle("MWCC_PCODE_O0_LOADED_CONSTANT_LATE")
                            && (matches!(left.kind, ExprKind::Load { .. })
                                || left.as_var().is_some_and(|id| self.homes[id].is_some() || target.is_some() && self.registers[id] == target)))))
            {
                true
            } else if self.unoptimized && !toggle("MWCC_PCODE_O0_FLOAT_REORDER") {
                // (-O0 otherwise keeps source order, except that a declared
                // variable goes before a computed operand.)
                // (Not after one computed from a single variable and constants.)
                let single = |e: &Expr| match &e.kind {
                    ExprKind::Binary(_, x, y) => {
                        let leafish = |v: &Expr| v.as_var().is_some() || matches!(v.kind, ExprKind::Float(_) | ExprKind::Int(_));
                        leafish(x) && leafish(y) && (x.as_var().is_none() || y.as_var().is_none())
                    }
                    _ => false,
                };
                need_first
                    || matches!(left.kind, ExprKind::Binary(..))
                    && !single(left)
                    && right.as_var().is_some_and(|id| {
                        self.function.variables[id].kind != VariableKind::Temporary
                            && !self.function.variables[id].name.starts_with('@')
                            && self.homes[id].is_none()
                    })
                    && !toggle("MWCC_PCODE_O0_FLOAT_SOURCE_ORDER")
            } else if !toggle("MWCC_PCODE_FLOAT_OLD_ORDER") {
                // Source order, except that a variable or loaded value goes
                // before an arithmetic result, and of two results the one
                // needing fewer registers goes first.
                let simple = |e: &Expr| e.as_var().is_some() || matches!(e.kind, ExprKind::Load { .. } | ExprKind::Global(_));
                (matches!(left.kind, ExprKind::Binary(..)) && simple(right))
                    || (matches!(left.kind, ExprKind::Binary(..))
                        && matches!(right.kind, ExprKind::Binary(..))
                        && register_need(left) > register_need(right)
                        && !toggle("MWCC_PCODE_NO_FLOAT_NEED_ORDER"))
            } else if refined && right.as_var().is_some() && matches!(left.kind, ExprKind::Binary(..)) {
                true
            } else if refined && left.as_var().is_some() && !toggle("MWCC_PCODE_FLOAT_COMPUTED_FIRST") {
                false
            } else {
                right.as_var().is_none() && !loaded
            };
        let (a, b) = if swap { (b, a) } else { (a, b) };
        let d = self.result_for(ty, target);
        self.emit_plain(match (op, single) {
            (BinaryOp::Add, true) => Instruction::FloatAddSingle { d, a, b },
            (BinaryOp::Add, false) => Instruction::FloatAddDouble { d, a, b },
            (BinaryOp::Subtract, true) => Instruction::FloatSubtractSingle { d, a, b },
            (BinaryOp::Subtract, false) => Instruction::FloatSubtractDouble { d, a, b },
            (BinaryOp::Multiply, true) => Instruction::FloatMultiplySingle { d, a, c: b },
            (BinaryOp::Multiply, false) => Instruction::FloatMultiplyDouble { d, a, c: b },
            (BinaryOp::Divide, true) => Instruction::FloatDivideSingle { d, a, b },
            (BinaryOp::Divide, false) => Instruction::FloatDivideDouble { d, a, b },
            (other, _) => return Err(unsupported(format!("floating operator {other:?}"))),
        });
        Ok((d, ty))
    }

    fn float_convert(&mut self, operand: &Expr, to: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        match (operand.ty, to) {
            (from, to) if from == to => self.expression_with_target(operand, target),
            // A double literal (the parser's `(double)` of a bare literal)
            // keeps its full precision, as the -O4 fold gives it.
            (Type::Float, Type::Double) if matches!(operand.kind, ExprKind::Float(_)) && !toggle("MWCC_PCODE_ROUNDED_DOUBLE_LITERALS") => {
                let ExprKind::Float(value) = operand.kind else { unreachable!() };
                self.float_constant(value, Type::Double, target)
            }
            (Type::Float, Type::Double) => {
                let (source, _) = self.expression_with_target(operand, target)?;
                Ok((source, to))
            }
            (Type::Double, Type::Float) => {
                // (A call's result rounds from its register into the target.)
                if matches!(operand.kind, ExprKind::Call { .. }) && target.is_some() && !toggle("MWCC_PCODE_CALL_RESULT_ROUNDS_IN_PLACE") {
                    let (source, _) = self.expression(operand)?;
                    let d = self.result_for(to, target);
                    self.emit_plain(Instruction::RoundToSingle { d, b: source });
                    return Ok((d, to));
                }
                // A computed double rounds in place in its own register.
                if operand.as_var().is_none() && std::env::var_os("MWCC_PCODE_NO_INPLACE_FRSP").is_none() {
                    let (source, _) = self.expression_with_target(operand, target)?;
                    // Only a fresh value may be overwritten: not one a cache
                    // or a variable still holds.
                    let shared = self.common.values().any(|&(register, _, _)| register == source)
                        || self.float_constants.values().any(|&register| register == source)
                        || self.loaded_globals.values().any(|&(register, _)| register == source)
                        || self.registers.contains(&Some(source));
                    if shared {
                        let d = self.result_for(to, target);
                        self.emit_plain(Instruction::RoundToSingle { d, b: source });
                        return Ok((d, to));
                    }
                    let mut round = PInstr::new(Instruction::RoundToSingle { d: source, b: source });
                    round.flags.in_place = true;
                    self.emit(round);
                    return Ok((source, to));
                }
                let (source, _) = self.expression(operand)?;
                let d = self.result_for(to, target);
                self.emit_plain(Instruction::RoundToSingle { d, b: source });
                Ok((d, to))
            }
            (from, to) if is_float(to) && matches!(from, Type::Int | Type::UnsignedInt | Type::Char | Type::Short | Type::UnsignedChar | Type::UnsignedShort) => {
                // (double)(x ^ 0x80000000 as the low word of 0x4330...) - 2^52 (+ 2^31).
                let signed = matches!(from, Type::Int | Type::Char | Type::Short);
                let narrow_unsigned = matches!(from, Type::UnsignedChar | Type::UnsignedShort);
                let (source, _) = if from == Type::Int || from == Type::UnsignedInt {
                    self.expression(operand)?
                } else {
                    // A narrow integer widens first.
                    let wide = if signed { Type::Int } else { Type::UnsignedInt };
                    self.expression(&Expr { kind: ExprKind::Convert(Box::new(operand.clone())), ty: wide })?
                };
                let slot = self.conversion_slot(false);
                if self.unoptimized {
                    if narrow_unsigned {
                        return Err(unsupported("narrow unsigned conversion at -O0"));
                    }
                    // Unscheduled: the bias, then each word as it is formed.
                    let magic = if signed { 4503601774854144.0 } else { 4503599627370496.0 };
                    let (bias, _) = self.float_constant(magic, Type::Double, None)?;
                    let low = if signed {
                        let flipped = self.temporary();
                        self.emit_plain(Instruction::XorImmediateShifted { a: flipped, s: source, immediate: 0x8000 });
                        flipped
                    } else {
                        source
                    };
                    self.emit_plain(Instruction::StoreWord { s: low, a: 1, offset: slot + 4 });
                    let high = self.temporary();
                    self.load_constant(high, 0x4330_0000)?;
                    self.emit_plain(Instruction::StoreWord { s: high, a: 1, offset: slot });
                    let loaded = self.fresh(Type::Double);
                    self.emit_plain(Instruction::LoadFloatDouble { d: loaded, a: 1, offset: slot });
                    let d = self.result_for(to, target);
                    self.emit_plain(if to == Type::Float {
                        Instruction::FloatSubtractSingle { d, a: loaded, b: bias }
                    } else {
                        Instruction::FloatSubtractDouble { d, a: loaded, b: bias }
                    });
                    return Ok((d, to));
                }
                let low = if signed {
                    let flipped = self.temporary();
                    self.emit_plain(Instruction::XorImmediateShifted { a: flipped, s: source, immediate: 0x8000 });
                    flipped
                } else {
                    source
                };
                let (high, _) = self.expression(&Expr::int(0x4330_0000))?;
                let magic = if signed { 4503601774854144.0 } else { 4503599627370496.0 };
                let (bias, _) = self.float_constant(magic, Type::Double, None)?;
                if !narrow_unsigned {
                    self.emit_plain(Instruction::StoreWord { s: low, a: 1, offset: slot + 4 });
                    self.emit_plain(Instruction::StoreWord { s: high, a: 1, offset: slot });
                } else {
                    self.emit_plain(Instruction::StoreWord { s: high, a: 1, offset: slot });
                    self.emit_plain(Instruction::StoreWord { s: low, a: 1, offset: slot + 4 });
                }
                let loaded = self.fresh(Type::Double);
                self.emit_plain(Instruction::LoadFloatDouble { d: loaded, a: 1, offset: slot });
                let d = self.result_for(to, target);
                self.emit_plain(if to == Type::Float {
                    Instruction::FloatSubtractSingle { d, a: loaded, b: bias }
                } else {
                    Instruction::FloatSubtractDouble { d, a: loaded, b: bias }
                });
                Ok((d, to))
            }
            // To unsigned: MWCC's runtime helper.
            (from, Type::UnsignedInt) if is_float(from) && !toggle("MWCC_PCODE_NO_FP2UNSIGNED") => {
                let argument = Expr { kind: ExprKind::Convert(Box::new(operand.clone())), ty: Type::Double };
                let argument = if from == Type::Double { operand.clone() } else { argument };
                let d = self.call("__cvt_fp2unsigned", &[argument], Type::UnsignedInt, target)?;
                Ok((d, Type::UnsignedInt))
            }
            // To a narrow type: as to int; the word is the raw narrow value.
            (from, to) if is_float(from) && is_narrow(to) && !toggle("MWCC_PCODE_NO_FLOAT_TO_NARROW") => {
                let (source, _) = self.expression(operand)?;
                let slot = self.conversion_slot(true);
                let rounded = self.fresh(Type::Double);
                self.emit_plain(Instruction::ConvertToIntegerWordZero { d: rounded, b: source });
                self.emit_plain(Instruction::StoreFloatDouble { s: rounded, a: 1, offset: slot });
                let d = self.result(target);
                self.emit_plain(Instruction::LoadWord { d, a: 1, offset: slot + 4 });
                Ok((d, to))
            }
            (from, to) if is_float(from) && matches!(to, Type::Int) => {
                // fctiwz; stfd; lwz the low word.
                let (source, _) = self.expression(operand)?;
                let slot = self.conversion_slot(true);
                let rounded = self.fresh(Type::Double);
                self.emit_plain(Instruction::ConvertToIntegerWordZero { d: rounded, b: source });
                self.emit_plain(Instruction::StoreFloatDouble { s: rounded, a: 1, offset: slot });
                let d = self.result(target);
                self.emit_plain(Instruction::LoadWord { d, a: 1, offset: slot + 4 });
                Ok((d, to))
            }
            _ => Err(unsupported("integer/floating conversion")),
        }
    }

    /// A floating comparison's 0/1 value: compare, `mfcr`, extract the bit.
    fn float_comparison_value(&mut self, op: BinaryOp, left: &Expr, right: &Expr, target: Option<u32>) -> Compilation<(u32, Type)> {
        let (bit, true_when_set) = self.compare(op, left, right)?;
        if !true_when_set {
            return Err(unsupported("floating != as a value"));
        }
        let condition = self.temporary();
        self.emit_plain(Instruction::MoveFromConditionRegister { d: condition });
        let d = self.result(target);
        self.emit_plain(Instruction::RotateAndMask { a: d, s: condition, shift: bit + 1, begin: 31, end: 31 });
        Ok((d, Type::Int))
    }

    // ------------------------------------------------------------ idioms

    fn idiom(&mut self, idiom: &Idiom, target: Option<u32>) -> Compilation<(u32, Type)> {
        match idiom {
            // `x op= k`: the operands load and combine in source order.
            Idiom::Update(op, left, right) => {
                let ty = left.ty;
                let single = ty == Type::Float;
                let (a, _) = self.expression(left)?;
                let (b, _) = self.expression(right)?;
                let d = self.result_for(ty, target);
                self.emit_plain(match (op, single) {
                    (BinaryOp::Add, true) => Instruction::FloatAddSingle { d, a, b },
                    (BinaryOp::Add, false) => Instruction::FloatAddDouble { d, a, b },
                    (BinaryOp::Multiply, true) => Instruction::FloatMultiplySingle { d, a, c: b },
                    (BinaryOp::Multiply, false) => Instruction::FloatMultiplyDouble { d, a, c: b },
                    (other, _) => return Err(unsupported(format!("floating update {other:?}"))),
                });
                Ok((d, ty))
            }
            Idiom::Insert { base, value, shift, begin, end } => {
                // The inserted value is computed before the old unit loads.
                // -O0 inserts a promoted narrow value as it is (no
                // extension), and converts a wider value to the unit's
                // type first.
                let unit = unpromoted(base).ty;
                let narrow_source = match &value.kind {
                    ExprKind::Convert(inner) if self.unoptimized && is_narrow(inner.ty) && !toggle("MWCC_PCODE_O0_UNCONVERTED_INSERT") => {
                        Some(inner.as_ref())
                    }
                    _ => None,
                };
                // (A value `(y >> n) & m` whose inserted bits lie inside m and
                // below the shifted-out ones is y rotated by `shift - n`.)
                let mut shift = *shift;
                let rotated = if self.unoptimized || self.unit.early_frame || toggle("MWCC_PCODE_NO_ROTATED_INSERT") {
                    None
                } else {
                    let (shifted, mask) = match &value.kind {
                        ExprKind::Binary(BinaryOp::BitAnd, inner, m) if m.as_int().is_some() => (inner.as_ref(), m.as_int().map(|m| m as u32)),
                        _ => (value.as_ref(), None),
                    };
                    match &shifted.kind {
                        ExprKind::Binary(BinaryOp::ShiftRight, y, n)
                            if matches!(y.ty, Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. })
                                && n.as_int().is_some_and(|n| (1..32).contains(&n)) =>
                        {
                            let n = n.as_int().unwrap_or(0) as u32;
                            let value_bits = field_bits(*begin, *end).rotate_right(u32::from(shift));
                            let fits = value_bits & !(u32::MAX >> n) == 0 && mask.is_none_or(|m| value_bits & !m == 0);
                            fits.then_some((y.as_ref(), n))
                        }
                        _ => None,
                    }
                };
                let (mut x, _) = match (self.inserted.take(), rotated) {
                    (Some((at, x)), _) if at == value.as_ref() as *const Expr as usize => (x, value.ty),
                    (_, Some((y, n))) => {
                        shift = ((u32::from(shift) + 32 - n) % 32) as u8;
                        self.expression(y)?
                    }
                    _ => self.expression(narrow_source.unwrap_or(value))?,
                };
                if self.unoptimized
                    && narrow_source.is_none()
                    && is_narrow(unit)
                    && !is_narrow(value.ty)
                    && value.as_int().is_none()
                    && !toggle("MWCC_PCODE_O0_UNCONVERTED_INSERT")
                {
                    let extended = self.temporary();
                    self.emit_plain(extension(unit, extended, x));
                    x = extended;
                }
                let (b, _) = self.expression(base)?;
                // rlwimi overwrites its destination before reading the
                // source: the destination must not hold the source.
                let target = target.filter(|&d| d != x);
                let d = self.result(target);
                if b != d {
                    self.emit_plain(Instruction::Or { a: d, s: b, b });
                }
                self.emit_plain(Instruction::RotateAndMaskInsert { a: d, s: x, shift, begin: *begin, end: *end });
                Ok((d, Type::Int))
            }
            Idiom::Unary(IntrinsicOp::CountLeadingZeros, value) => {
                let (a, _) = self.expression(value)?;
                let d = self.result(target);
                self.emit_plain(Instruction::CountLeadingZeros { a: d, s: a });
                Ok((d, Type::Int))
            }
            Idiom::Unary(IntrinsicOp::FloatAbsolute, value) => {
                let (b, ty) = self.expression(value)?;
                let d = self.result_for(ty, target);
                self.emit_plain(Instruction::FloatAbsolute { d, b });
                Ok((d, ty))
            }
            Idiom::Unary(IntrinsicOp::VaInfo, _) => Err(unsupported("a va_info value")),
            Idiom::Unary(op @ (IntrinsicOp::VaIncoming | IntrinsicOp::VaSaveArea), _) => {
                let d = self.result(target);
                let mut addi = PInstr::new(Instruction::AddImmediate { d, a: 1, immediate: 8 });
                if *op == IntrinsicOp::VaIncoming {
                    addi.displacement_symbol = Some("@@incoming".to_owned());
                }
                self.emit(addi);
                Ok((d, Type::Pointer(mwcc_iro::Pointee::Char)))
            }
            Idiom::Unary(IntrinsicOp::Synchronize | IntrinsicOp::InstructionSynchronize | IntrinsicOp::EnforceInOrderIo, _) => {
                Err(unsupported("a barrier's value"))
            }
            Idiom::Absolute(value) => {
                let (a, _) = self.expression(value)?;
                let sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: sign, s: a, shift: 31 });
                let flipped = self.temporary();
                self.emit_plain(Instruction::Xor { a: flipped, s: sign, b: a });
                let d = self.result(target);
                self.emit_plain(Instruction::SubtractFrom { d, a: sign, b: flipped });
                Ok((d, Type::Int))
            }
            // (A mask of all ones is the relation's mask itself.)
            Idiom::Masked { relation, tested, value, keep_when_true: true } if value.as_int() == Some(-1) => {
                let (a, _) = self.expression(tested)?;
                let d = self.result(target);
                self.relation_mask(*relation, a, d);
                Ok((d, Type::Int))
            }
            Idiom::Masked { relation, tested, value, keep_when_true } => {
                let (a, _) = self.expression(tested)?;
                let mask = self.temporary();
                self.relation_mask(*relation, a, mask);
                // (The tested value itself is not recomputed.)
                let (b, ty) = if format!("{:?}", value.kind) == format!("{:?}", tested.kind) && value.as_var().is_none() {
                    (a, value.ty)
                } else {
                    self.expression(value)?
                };
                let d = self.result(target);
                self.emit_plain(if *keep_when_true {
                    Instruction::And { a: d, s: b, b: mask }
                } else {
                    Instruction::AndComplement { a: d, s: b, b: mask }
                });
                Ok((d, promote(ty)))
            }
        }
    }

    // ------------------------------------------------------------ comparisons

    /// A comparison's 0/1 value, branch-free as MWCC generates it.
    fn comparison_value(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        use BinaryOp::*;
        let (op, left, right) = if left.as_int().is_some() && right.as_int().is_none() {
            (op.mirror(), right, left)
        } else if !self.unoptimized && equality_variable_first(op, left, right) {
            (op, right, left)
        } else {
            (op, left, right)
        };
        let constant = right.as_int();
        let early = if self.unit.branch_preserving { None } else { self.call_operand_first(right)? };
        let (p, left_type) = self.expression(left)?;
        let unsigned = is_unsigned(promote(left_type)) || is_unsigned(promote(right.ty));
        let small = |value: i64| i16::try_from(value).is_ok() && i16::try_from(-value).is_ok();
        if self.unit.branch_preserving {
            return self.early_comparison_value(op, p, right, unsigned, target);
        }
        // Forms against constants that need no second register.
        match (op, constant, unsigned) {
            (Equal, Some(0), _) => {
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: p });
                return self.shift_out(n, 5, target);
            }
            (Equal, Some(value), _) if small(value) => {
                let d = self.temporary();
                if self.unit.equality_subtracts_constant {
                    self.emit_based(Instruction::AddImmediate { d, a: p, immediate: -value as i16 }, p);
                } else {
                    self.emit_plain(Instruction::SubtractFromImmediate { d, a: p, immediate: value as i16 });
                }
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: d });
                return self.shift_out(n, 5, target);
            }
            (NotEqual, Some(0), _) => {
                let n = self.temporary();
                self.emit_plain(Instruction::Negate { d: n, a: p });
                let o = self.temporary();
                self.emit_plain(Instruction::Or { a: o, s: n, b: p });
                return self.shift_out(o, 31, target);
            }
            (NotEqual, Some(value), _) if small(value) => {
                let a = self.temporary();
                self.emit_plain(Instruction::SubtractFromImmediate { d: a, a: p, immediate: value as i16 });
                let b = self.temporary();
                self.emit_based(Instruction::AddImmediate { d: b, a: p, immediate: -value as i16 }, p);
                let o = self.temporary();
                self.emit_plain(Instruction::Or { a: o, s: a, b });
                return self.shift_out(o, 31, target);
            }
            (Less, Some(0), false) => return self.shift_out(p, 31, target),
            (GreaterEqual, Some(0), false) => {
                let s = self.temporary();
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: s, s: p, shift: 31 });
                let d = self.result(target);
                self.emit_plain(Instruction::XorImmediate { a: d, s, immediate: 1 });
                return Ok((d, Type::Int));
            }
            (Greater, Some(0), false) => {
                let n = self.temporary();
                self.emit_plain(Instruction::Negate { d: n, a: p });
                let a = self.temporary();
                self.emit_plain(Instruction::AndComplement { a, s: n, b: p });
                return self.shift_out(a, 31, target);
            }
            (LessEqual, Some(0), false) => {
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: p });
                // `1 rotl clz(p)` keeps bit 31 only when p <= 0, in place.
                let d = self.result(target);
                self.load_constant(d, 1)?;
                self.emit_plain(Instruction::RotateAndMaskVariable { a: d, s: d, b: n, begin: 31, end: 31 });
                return Ok((d, Type::Int));
            }
            // Unsigned relations against zero fold in IRO (unmodeled).
            (_, Some(0), true) => return Err(unsupported("unsigned comparison with zero")),
            _ => {}
        }
        let q = match early {
            Some(q) => q,
            None => self.expression(right)?.0,
        };
        match (op, unsigned) {
            (Equal, _) => {
                let d = self.temporary();
                if constant.is_some() && self.unit.equality_subtracts_constant {
                    self.emit_plain(Instruction::SubtractFrom { d, a: q, b: p });
                } else {
                    self.emit_plain(Instruction::SubtractFrom { d, a: p, b: q });
                }
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: d });
                self.shift_out(n, 5, target)
            }
            (NotEqual, _) => {
                let a = self.temporary();
                self.emit_plain(Instruction::SubtractFrom { d: a, a: p, b: q });
                let b = self.temporary();
                self.emit_plain(Instruction::SubtractFrom { d: b, a: q, b: p });
                let o = self.temporary();
                self.emit_plain(Instruction::Or { a: o, s: a, b });
                self.shift_out(o, 31, target)
            }
            (Less, false) => self.signed_less(p, q, target),
            (Greater, false) => self.signed_less(q, p, target),
            (LessEqual, false) => self.signed_less_equal(p, q, target),
            (GreaterEqual, false) => self.signed_less_equal(q, p, target),
            (Less, true) => self.unsigned_less(p, q, target),
            (Greater, true) => self.unsigned_less(q, p, target),
            (LessEqual, true) => self.unsigned_less_equal(p, q, target),
            (GreaterEqual, true) => self.unsigned_less_equal(q, p, target),
            _ => unreachable!("comparison operators only"),
        }
    }

    /// GC/1.0-1.2.5n comparison values: `x == y` from `y - x` (`neg` for
    /// zero) and `cntlzw`, `x != y` by the carry of `y - x - 1`, signed `<`
    /// by `eqv` and the carry, unsigned relations by the carry alone.
    fn early_comparison_value(
        &mut self,
        op: BinaryOp,
        p: u32,
        right: &Expr,
        unsigned: bool,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        use BinaryOp::*;
        let small = |value: i64| i16::try_from(value).is_ok() && i16::try_from(-value).is_ok();
        if matches!(op, Equal | NotEqual) {
            let difference = self.temporary();
            match right.as_int() {
                Some(0) => self.emit_plain(Instruction::Negate { d: difference, a: p }),
                Some(value) if small(value) => {
                    self.emit_plain(Instruction::SubtractFromImmediate { d: difference, a: p, immediate: value as i16 })
                }
                _ => {
                    let (q, _) = self.expression(right)?;
                    self.emit_plain(Instruction::SubtractFrom { d: difference, a: p, b: q });
                }
            }
            if op == Equal {
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: difference });
                return self.shift_out(n, 5, target);
            }
            let less = self.temporary();
            self.emit_plain(Instruction::AddImmediateCarrying { d: less, a: difference, immediate: -1 });
            let d = self.result(target);
            self.emit_plain(Instruction::SubtractFromExtended { d, a: less, b: difference });
            return Ok((d, Type::Int));
        }
        let (q, _) = self.expression(right)?;
        let (low, high) = if matches!(op, Greater | GreaterEqual) { (q, p) } else { (p, q) };
        match (op, unsigned) {
            (Less | Greater, false) => {
                // `low < high`: (signs equal) + carry(low - high), low bit.
                // (The carry-only difference, the sum and the result share
                // one register.)
                let same = self.temporary();
                self.emit_plain(Instruction::Eqv { a: same, s: high, b: low });
                let d = self.result(target);
                self.emit_plain(Instruction::SubtractFromCarrying { d, a: high, b: low });
                let sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: same, shift: 31 });
                let mut sum = PInstr::new(Instruction::AddToZeroExtended { d, a: sign });
                sum.flags.continues_web = true;
                self.emit(sum);
                let mut low_bit = PInstr::new(Instruction::ClearLeftImmediate { a: d, s: d, clear: 31 });
                low_bit.flags.in_place = true;
                self.emit(low_bit);
                Ok((d, Type::Int))
            }
            (LessEqual | GreaterEqual, false) => {
                // (The sign words number low first.)
                let low_sign = self.temporary();
                let high_sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: high_sign, s: high, shift: 31 });
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: low_sign, s: low, shift: 31 });
                let c = self.temporary();
                self.emit_plain(Instruction::SubtractFromCarrying { d: c, a: low, b: high });
                let d = self.result(target);
                self.emit_plain(Instruction::AddExtended { d, a: high_sign, b: low_sign });
                Ok((d, Type::Int))
            }
            (Less | Greater, true) => {
                // `low < high`: borrow of low - high, as 0/-1, negated.
                let d = self.result(target);
                self.emit_plain(Instruction::SubtractFromCarrying { d, a: high, b: low });
                // (`subfe m,m,m` reads its own undefined register: -1 + CA.)
                let mask = self.temporary();
                self.emit_plain(Instruction::SubtractFromExtended { d: mask, a: mask, b: mask });
                let mut negated = PInstr::new(Instruction::Negate { d, a: mask });
                negated.flags.continues_web = true;
                self.emit(negated);
                Ok((d, Type::Int))
            }
            (LessEqual | GreaterEqual, true) => {
                // `low <= high`: the carry of high - low.
                // (The carry-only difference shares the -1's register.)
                let ones = self.temporary();
                self.emit_plain(Instruction::SubtractFromCarrying { d: ones, a: low, b: high });
                let mut minus_one = PInstr::new(Instruction::AddImmediate { d: ones, a: 0, immediate: -1 });
                minus_one.flags.continues_web = true;
                self.emit(minus_one);
                let d = self.result(target);
                self.emit_plain(Instruction::SubtractFromZeroExtended { d, a: ones });
                Ok((d, Type::Int))
            }
            _ => unreachable!("comparison operators only"),
        }
    }

    /// `value >> shift` (logical) into the result register.
    fn shift_out(&mut self, value: u32, shift: u8, target: Option<u32>) -> Compilation<(u32, Type)> {
        let d = self.result(target);
        self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: d, s: value, shift });
        Ok((d, Type::Int))
    }

    fn signed_less(&mut self, p: u32, q: u32, target: Option<u32>) -> Compilation<(u32, Type)> {
        let x = self.temporary();
        self.emit_plain(Instruction::Xor { a: x, s: q, b: p });
        let s = self.temporary();
        self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: s, s: x, shift: 1 });
        let a = self.temporary();
        self.emit_plain(Instruction::And { a, s: x, b: q });
        let d = self.temporary();
        self.emit_plain(Instruction::SubtractFrom { d, a, b: s });
        self.shift_out(d, 31, target)
    }

    fn signed_less_equal(&mut self, p: u32, q: u32, target: Option<u32>) -> Compilation<(u32, Type)> {
        let high = self.temporary();
        self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: high, s: q, shift: 31 });
        let low = self.temporary();
        self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: low, s: p, shift: 31 });
        let c = self.temporary();
        self.emit_plain(Instruction::SubtractFromCarrying { d: c, a: p, b: q });
        let d = self.result(target);
        self.emit_plain(Instruction::AddExtended { d, a: high, b: low });
        Ok((d, Type::Int))
    }

    fn unsigned_less(&mut self, p: u32, q: u32, target: Option<u32>) -> Compilation<(u32, Type)> {
        let x = self.temporary();
        self.emit_plain(Instruction::Xor { a: x, s: q, b: p });
        let n = self.temporary();
        self.emit_plain(Instruction::CountLeadingZeros { a: n, s: x });
        let w = self.temporary();
        self.emit_plain(Instruction::ShiftLeftWord { a: w, s: q, b: n });
        self.shift_out(w, 31, target)
    }

    fn unsigned_less_equal(&mut self, p: u32, q: u32, target: Option<u32>) -> Compilation<(u32, Type)> {
        let d = self.temporary();
        self.emit_plain(Instruction::SubtractFrom { d, a: p, b: q });
        let o = self.temporary();
        self.emit_plain(Instruction::OrComplement { a: o, s: q, b: p });
        let s = self.temporary();
        self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: s, s: d, shift: 1 });
        let r = self.temporary();
        self.emit_plain(Instruction::SubtractFrom { d: r, a: s, b: o });
        self.shift_out(r, 31, target)
    }

    // ------------------------------------------------------------ arithmetic

    fn binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let returning = std::mem::take(&mut self.returning);
        if matches!(op, BinaryOp::Divide | BinaryOp::Modulo) {
            return self.division(op, left, right, ty, target);
        }
        // A `&&`/`||` value: 0, then 1 unless the condition is false
        // (GC/1.0-1.2.5n: an `||` 1, then 0 unless it is true).
        if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) && !toggle("MWCC_PCODE_NO_LOGICAL_VALUES") {
            let whole = Expr::binary(op, left.clone(), right.clone(), Type::Int);
            let first = op == BinaryOp::LogicalOr && self.unit.early_frame;
            let d = self.result(target);
            self.emit_plain(Instruction::AddImmediate { d, a: 0, immediate: i16::from(first) });
            let skip = self.new_label();
            self.branch_on(&whole, first, skip)?;
            self.emit_plain(Instruction::AddImmediate { d, a: 0, immediate: i16::from(!first) });
            self.place_label(skip);
            return Ok((d, Type::Int));
        }
        // `k - x` is `subfic`.
        if let (BinaryOp::Subtract, Some(value), None) = (op, left.as_int(), right.as_int()) {
            if let Ok(value) = i16::try_from(value) {
                let (a, _) = self.expression(right)?;
                let d = self.result(target);
                self.emit_plain(Instruction::SubtractFromImmediate { d, a, immediate: value });
                return Ok((d, ty));
            }
        }
        let immediate = right.as_int().and_then(|value| i16::try_from(value).ok());
        // `(x & mask) << n` (or `* 2^n`) is one rotate-and-mask.
        let left_shift = match (op, right.as_int()) {
            (BinaryOp::ShiftLeft, Some(n)) if (1..32).contains(&n) => Some(n as u8),
            (BinaryOp::Multiply, Some(n)) if n > 1 && n < (1 << 31) && (n as u32).is_power_of_two() => {
                Some((n as u32).trailing_zeros() as u8)
            }
            _ => None,
        };
        // A narrow unsigned variable shifted left rotates and masks its
        // bits as they are (`rlwinm d,x,n,24-n,31-n` for a byte).
        if let (Some(n), Some(inner), false) = (left_shift, narrow_variable(left), self.unoptimized) {
            if matches!(inner.ty, Type::UnsignedChar | Type::UnsignedShort)
                && u32::from(n) + 8 * width(inner.ty) <= 32
                && !toggle("MWCC_PCODE_NO_NARROW_SHIFT_MASK")
            {
                let bits = 8 * width(inner.ty) as u8;
                let (x, _) = self.expression(inner)?;
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift: n, begin: 32 - bits - n, end: 31 - n });
                return Ok((d, ty));
            }
        }
        if let (Some(n), ExprKind::Binary(BinaryOp::BitAnd, inner, mask), false) = (left_shift, &left.kind, self.unoptimized) {
            if let Some((begin, end)) = mask_bounds(mask) {
                if begin >= n {
                    // `((y >> s) & m) << n` (y unsigned, m within the shifted
                    // value's bits) rotates y by n - s.
                    let shifted = match &unpromoted(inner).kind {
                        ExprKind::Binary(BinaryOp::ShiftRight, y, amount) => amount.as_int().map(|s| (y, s)),
                        // (An unsigned division by 2^s is the same shift.)
                        ExprKind::Binary(BinaryOp::Divide, y, divisor) => divisor
                            .as_int()
                            .filter(|&k| k > 1 && (k as u64).is_power_of_two())
                            .map(|k| (y, i64::from((k as u64).trailing_zeros()))),
                        _ => None,
                    };
                    if let Some((y, s)) = shifted {
                        if (1..32).contains(&s) && i64::from(begin) >= s {
                            if is_unsigned(promote(y.ty)) && !toggle("MWCC_PCODE_NO_SHIFT_MASK_SHIFT") {
                                let (x, _) = self.expression(y)?;
                                let d = self.result(target);
                                let shift = ((32 + i64::from(n) - s) % 32) as u8;
                                self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift, begin: begin - n, end: end - n });
                                return Ok((d, ty));
                            }
                        }
                    }
                    let (x, _) = self.expression(inner)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift: n, begin: begin - n, end: end - n });
                    return Ok((d, ty));
                }
            }
        }
        // `field | base` where the base is known zero under the field's mask
        // inserts the field: `rlwimi base,x,shift,mb,me` (the left operand
        // when it is a field, else the right one).
        if op == BinaryOp::BitOr && !self.unoptimized {
            let refined = !toggle("MWCC_PCODE_OLD_INSERTS");
            // (A rotation `(x << n) | (x >> 32 - n)` is a rotate on GC/3.x
            // and Wii, an `or` on GC/1.0-1.2.5n when it is the target's value.)
            let rotated = rotation(left, right).filter(|_| {
                refined && (self.unit.rotates || (self.unit.early_frame && target.is_some())) && !toggle("MWCC_PCODE_ROTATION_INSERTS")
            });
            if let Some((x, n)) = rotated {
                if self.unit.rotates {
                    let (x, _) = self.expression(x)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift: n, begin: 0, end: 31 });
                    return Ok((d, ty));
                }
            }
            // (A shifted right operand is inserted into the left one.)
            let shifted_right = refined
                && matches!(right.kind, ExprKind::Binary(BinaryOp::ShiftLeft, ..))
                && left.as_int().is_none()
                && !matches!(left.kind, ExprKind::Binary(BinaryOp::ShiftRight | BinaryOp::BitAnd, ..))
                && insert_field(right, true).is_some_and(|field| field_bits(field.2, field.3) & !known_zero(left, true) == 0)
                && !toggle("MWCC_PCODE_LEFT_FIELD_FIRST");
            // (GC/1.0-1.2.5n insert into a shifted parameter on the left.)
            let parameter_count = self.function.parameter_count;
            let shifted_left = refined
                && self.unit.early_frame
                && rotation(left, right).is_none()
                && matches!(&left.kind, ExprKind::Binary(BinaryOp::ShiftLeft, inner, n)
                    if n.as_int().is_some() && matches!(unpromoted(inner).kind, ExprKind::Var(id) if id < parameter_count))
                && right.as_int().is_none()
                && insert_field(right, true).is_some_and(|field| field_bits(field.2, field.3) & !known_zero(left, true) == 0)
                && !toggle("MWCC_PCODE_NO_SHIFTED_LEFT_BASE");
            let shifted_right = shifted_right || shifted_left;
            let insertion = if rotated.is_some() { None } else { match insert_field(left, refined).filter(|_| !shifted_right) {
                // (A constant base is `ori`/`oris`d instead.)
                Some(field)
                    if field_bits(field.2, field.3) & !known_zero(right, refined) == 0
                        && !(refined && right.as_int().is_some()) =>
                {
                    Some((field, right))
                }
                // (Only a narrow value, into a computed base.)
                _ if refined
                    && left.as_int().is_none()
                    && (shifted_right || match &right.kind {
                        ExprKind::Convert(_) => true,
                        ExprKind::Binary(BinaryOp::ShiftLeft, inner, _) => narrow_variable(inner).is_some() || matches!(inner.kind, ExprKind::Convert(_)),
                        _ => false,
                    }) =>
                {
                    insert_field(right, true)
                        .filter(|field| field_bits(field.2, field.3) & !known_zero(left, true) == 0)
                        .map(|field| (field, left))
                }
                _ => None,
            } };
            if let Some(((source, shift, begin, end), right)) = insertion {
                // (A clean value inserted unshifted is an `or`.)
                if refined
                    && shift == 0
                    && narrow_variable(source).is_none()
                    && matches!(&source.kind, ExprKind::Convert(inner) if matches!(inner.kind, ExprKind::Load { .. }))
                    && possible_bits(source) & !field_bits(begin, end) == 0
                {
                    let (base, _) = self.expression(right)?;
                    let (x, _) = self.expression(source)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::Or { a: d, s: x, b: base });
                    return Ok((d, ty));
                }
                {
                    // A narrow variable inserted within its width, or the
                    // unsigned narrow base, is used as it is (unextended).
                    let unextended = !toggle("MWCC_PCODE_NO_UNEXTENDED_INSERT");
                    let source = match narrow_variable(source) {
                        Some(inner)
                            if unextended
                                && field_bits(begin, end).rotate_right(u32::from(shift)) & !narrow_mask(inner.ty) == 0 =>
                        {
                            inner
                        }
                        _ => source,
                    };
                    // (An unextended base only when the field covers its upper bits.)
                    let right = match narrow_variable(right) {
                        Some(inner)
                            if unextended
                                && matches!(inner.ty, Type::UnsignedChar | Type::UnsignedShort)
                                && (!refined || (field_bits(begin, end) | narrow_mask(inner.ty)) == u32::MAX) =>
                        {
                            inner
                        }
                        _ => right,
                    };
                    let (x, _) = self.expression(source)?;
                    // A returned insert is made in r3.
                    let target = match target {
                        None if returning && !toggle("MWCC_PCODE_NO_RETURNED_INSERT") => Some(3),
                        target => target,
                    };
                    let target = target.filter(|&d| d != x);
                    let mark = self.pcode.register_count(Class::General);
                    // (A base that is itself an insert is built in its own
                    // register.)
                    let (base, _) = if toggle("MWCC_PCODE_NO_INSERT_BASE_TARGET")
                        || refined && matches!(right.kind, ExprKind::Binary(BinaryOp::BitOr, ..))
                    {
                        self.expression(right)?
                    } else {
                        self.expression_with_target(right, target)?
                    };
                    // (A loaded base this insert just made takes the field
                    // in place.)
                    if target.is_none()
                        && refined
                        && base >= mark
                        && !self.registers.contains(&Some(base))
                        && matches!(unpromoted(right).kind, ExprKind::Load { .. })
                        && !toggle("MWCC_PCODE_NO_INPLACE_INSERT")
                    {
                        self.common.retain(|_, entry| entry.0 != base);
                        self.loaded_globals.retain(|_, entry| entry.0 != base);
                        let mut insert = PInstr::new(Instruction::RotateAndMaskInsert { a: base, s: x, shift, begin, end });
                        insert.flags.continues_web = true;
                        self.emit(insert);
                        return Ok((base, ty));
                    }
                    let d = self.result(target);
                    if base != d {
                        self.emit_plain(Instruction::Or { a: d, s: base, b: base });
                    }
                    self.emit_plain(Instruction::RotateAndMaskInsert { a: d, s: x, shift, begin, end });
                    return Ok((d, ty));
                }
            }
        }
        // `a & ~b` / `a | ~b` are single instructions.
        if let (BinaryOp::BitAnd | BinaryOp::BitOr, ExprKind::Unary(UnaryOp::BitNot, operand)) = (op, &right.kind) {
            let (a, _) = self.expression(left)?;
            let (b, _) = self.expression(operand)?;
            let d = self.result(target);
            self.emit_plain(if op == BinaryOp::BitAnd {
                Instruction::AndComplement { a: d, s: a, b }
            } else {
                Instruction::OrComplement { a: d, s: a, b }
            });
            return Ok((d, ty));
        }
        if op == BinaryOp::BitAnd {
            // A mask within a narrow load's width does not observe its sign
            // extension: load zero-extended and mask.
            let loaded = unpromoted(left);
            if let (Some((begin, end)), ExprKind::Load { base, index: None, offset }) = (mask_bounds(right), &loaded.kind) {
                let bits = match loaded.ty {
                    Type::Char => Some(8),
                    Type::Short => Some(16),
                    _ => None,
                };
                if let Some(bits) = bits.filter(|&bits| begin >= 32 - bits) {
                    let unsigned = if bits == 8 { Type::UnsignedChar } else { Type::UnsignedShort };
                    let widened = Expr {
                        kind: ExprKind::Load { base: base.clone(), index: None, offset: *offset },
                        ty: unsigned,
                    };
                    let (a, _) = self.expression(&widened)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                    return Ok((d, Type::Int));
                }
            }
            // A masked rotation is one rotate-and-mask (GC/3.x, Wii).
            if let (Some((begin, end)), ExprKind::Binary(BinaryOp::BitOr, a, b), true) = (mask_bounds(right), &left.kind, self.unit.rotates && !self.unoptimized) {
                if let Some((x, n)) = rotation(a, b).filter(|_| !toggle("MWCC_PCODE_ROTATION_INSERTS")) {
                    let (x, _) = self.expression(x)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift: n, begin, end });
                    return Ok((d, ty));
                }
            }
            if let Some((operand, shift, begin, end)) = shift_mask(left, right) {
                // (GC/1.0-1.2.5n extract a loaded bit-field value in place;
                // not a tested one.)
                if self.unit.early_frame
                    && self.testing == 0
                    && matches!(unpromoted(operand).kind, ExprKind::Load { .. })
                    && matches!(&left.kind, ExprKind::Binary(BinaryOp::ShiftRight, _, count) if count.ty == mwcc_iro::BIT_FIELD_SHIFT)
                    && !toggle("MWCC_PCODE_NO_INPLACE_FIELD")
                {
                    // (Only a register this load just made; the caches that
                    // would reuse the loaded value forget it.)
                    let mark = self.pcode.register_count(Class::General);
                    let (a, _) = self.expression(operand)?;
                    if a >= mark && !self.registers.contains(&Some(a)) {
                        self.common.retain(|_, entry| entry.0 != a);
                        self.loaded_globals.retain(|_, entry| entry.0 != a);
                        let mut extract = PInstr::new(Instruction::RotateAndMask { a, s: a, shift, begin, end });
                        extract.flags.continues_web = true;
                        self.emit(extract);
                        return Ok((a, ty));
                    }
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift, begin, end });
                    return Ok((d, ty));
                }
                let (a, _) = self.expression(operand)?;
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift, begin, end });
                return Ok((d, ty));
            }
        }
        // `(x & mask) >> n`: one rotate-and-mask when the masked value is
        // non-negative.
        if let (BinaryOp::ShiftRight, Some(n), ExprKind::Binary(BinaryOp::BitAnd, inner, mask), false) =
            (op, right.as_int(), &left.kind, self.unoptimized)
        {
            if let (Some((begin, end)), true) = (mask_bounds(mask), (1..32).contains(&n)) {
                let n = n as u8;
                if begin > 0 && begin + n <= 31 {
                    let (x, _) = self.expression(inner)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask {
                        a: d,
                        s: x,
                        shift: 32 - n,
                        begin: begin + n,
                        end: 31.min(end + n),
                    });
                    return Ok((d, ty));
                }
            }
        }
        // A raw unsigned narrow parameter shifted right folds its extension.
        let shifted = unpromoted(left);
        let raw_unsigned = match (&shifted.kind, op, immediate) {
            // (Not -O0: it extends, then shifts the int.)
            (ExprKind::Var(id), BinaryOp::ShiftRight, Some(shift))
                if (self.raw_narrow[*id]
                    // (A trusted extended unsigned parameter folds the same.)
                    || (self.unit.narrow_parameters_extended
                        && *id < self.function.parameter_count
                        && matches!(self.function.variables[*id].ty, Type::UnsignedChar | Type::UnsignedShort)
                        && !assigns(&self.function.body, *id)))
                    && (!self.unoptimized || toggle("MWCC_PCODE_O0_FOLDED_NARROW_SHIFT")) =>
            {
                let bits = match shifted.ty {
                    Type::UnsignedChar => Some(8u8),
                    Type::UnsignedShort => Some(16u8),
                    _ => None,
                };
                bits.filter(|&bits| (0..i16::from(bits)).contains(&shift)).map(|bits| (self.register(*id), bits))
            }
            _ => None,
        };
        if let (Some((raw, bits)), Some(shift)) = (raw_unsigned, immediate) {
            let d = self.result(target);
            let shift = shift as u8;
            self.emit_plain(Instruction::RotateAndMask { a: d, s: raw, shift: (32 - shift) % 32, begin: 32 - bits + shift, end: 31 });
            return Ok((d, ty));
        }
        // `(x << n) | (x >> (32 - n))` (x unsigned) is `rotlw x, n`; the
        // mirror rotates left by its computed `32 - n`.
        if op == BinaryOp::BitOr && !self.unoptimized && !toggle("MWCC_PCODE_NO_VARIABLE_ROTATE") {
            if let Some((x, amount)) = variable_rotate(left, right).or_else(|| variable_rotate(right, left)) {
                let (b, _) = self.expression(amount)?;
                let (s, _) = self.expression(x)?;
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMaskVariable { a: d, s, b, begin: 0, end: 31 });
                return Ok((d, ty));
            }
        }
        // An absolute array's index is computed before its address (at -O4
        // unless the sum is a memory access base).
        let addressing = std::mem::take(&mut self.addressing);
        let index_first = (self.unoptimized || !addressing)
            && op == BinaryOp::Add
            && immediate.is_none()
            && self.absolute_base(left)
            && right.as_int().is_none()
            && !toggle("MWCC_PCODE_ADDRESS_FIRST");
        // An operand that calls is evaluated first.
        // (When both call, the right one still goes first.)
        let calls_first = immediate.is_none()
            && right.as_int().is_none()
            && contains_call(right)
            // (Integer operands that both call: right first, except for a
            // subtraction.)
            && (!contains_call(left) || (op != BinaryOp::Subtract && !toggle("MWCC_PCODE_BOTH_CALLS_IN_ORDER")))
            && !toggle("MWCC_PCODE_CALL_OPERAND_IN_ORDER");
        // (-O0 computes a subtraction's right operand first.)
        let subtrahend_first = self.unoptimized
            && op == BinaryOp::Subtract
            && immediate.is_none()
            && right.as_int().is_none()
            && !toggle("MWCC_PCODE_O0_MINUEND_FIRST");
        // (Of two results of a commutative operation, the one needing fewer
        // registers goes first; -O0 also computes it first.)
        let need_order = op.is_commutative()
            && !op.is_comparison()
            && matches!(unpromoted(left).kind, ExprKind::Binary(..))
            && matches!(unpromoted(right).kind, ExprKind::Binary(..))
            && register_need(left) > register_need(right)
            && !toggle("MWCC_PCODE_NO_INTEGER_NEED_ORDER");
        let need_first = need_order && self.unoptimized;
        let early = if index_first || calls_first || subtrahend_first || need_first { Some(self.expression(right)?.0) } else { None };
        let (a, _) = self.expression(left)?;
        // GC/3.x: `x * (2^n ± 1)` = `(x << n) ± x`; `x * (1 - 2^n)` = `x - (x << n)`.
        let shift_add = (op == BinaryOp::Multiply && self.unit.shift_add_multiply)
            .then(|| right.as_int())
            .flatten()
            .and_then(|value| {
                (2..31).find_map(|n: u32| {
                    let power = 1i64 << n;
                    [(power + 1, 0u8), (power - 1, 1), (1 - power, 2)]
                        .into_iter()
                        .find(|&(k, _)| i64::from(value as i32) == k)
                        .map(|(_, form)| (n as u8, form))
                })
            });
        if let Some((shift, form)) = shift_add {
            let shifted = self.temporary();
            self.emit_plain(Instruction::ShiftLeftImmediate { a: shifted, s: a, shift });
            let d = self.result(target);
            self.emit_plain(match form {
                0 => Instruction::Add { d, a: shifted, b: a },
                1 => Instruction::SubtractFrom { d, a, b: shifted },
                _ => Instruction::SubtractFrom { d, a: shifted, b: a },
            });
            return Ok((d, ty));
        }
        match (op, immediate) {
            // `x * -2^k` is a shift and a negation (`neg` alone for -1).
            (BinaryOp::Multiply, Some(value)) if value < 0 && value != i16::MIN && (-value as u16).is_power_of_two() => {
                let shift = (-value as u16).trailing_zeros() as u8;
                let shifted = if shift == 0 {
                    a
                } else {
                    let shifted = self.temporary();
                    self.emit_plain(Instruction::ShiftLeftImmediate { a: shifted, s: a, shift });
                    shifted
                };
                let d = self.result(target);
                self.emit_plain(Instruction::Negate { d, a: shifted });
                return Ok((d, ty));
            }
            (BinaryOp::Add, Some(value)) => {
                let d = self.result(target);
                self.emit_based(Instruction::AddImmediate { d, a, immediate: value }, a);
                return Ok((d, ty));
            }
            (BinaryOp::Add, None) if wide_constant(right).is_some() => {
                let (high, low) = wide_constant(right).expect("checked");
                let d = self.result(target);
                let middle = if low == 0 { d } else { self.temporary() };
                self.emit_based(Instruction::AddImmediateShifted { d: middle, a, immediate: high }, a);
                if low != 0 {
                    self.emit_based(Instruction::AddImmediate { d, a: middle, immediate: low }, middle);
                }
                return Ok((d, ty));
            }
            (BinaryOp::Subtract, Some(value)) if value != i16::MIN => {
                let d = self.result(target);
                self.emit_based(Instruction::AddImmediate { d, a, immediate: -value }, a);
                return Ok((d, ty));
            }
            (BinaryOp::Multiply, Some(value)) if value > 0 && (value as u16).is_power_of_two() => {
                let d = self.result(target);
                let shift = (value as u16).trailing_zeros() as u8;
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift });
                return Ok((d, ty));
            }
            (BinaryOp::Multiply, Some(value)) => {
                let d = self.result(target);
                self.emit_plain(Instruction::MultiplyImmediate { d, a, immediate: value });
                return Ok((d, ty));
            }
            (BinaryOp::BitOr | BinaryOp::BitXor, _) if unsigned_immediate(right).is_some() => {
                let (value, shifted) = unsigned_immediate(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(match (op, shifted) {
                    (BinaryOp::BitOr, false) => Instruction::OrImmediate { a: d, s: a, immediate: value },
                    (BinaryOp::BitOr, true) => Instruction::OrImmediateShifted { a: d, s: a, immediate: value },
                    (_, false) => Instruction::XorImmediate { a: d, s: a, immediate: value },
                    (_, true) => Instruction::XorImmediateShifted { a: d, s: a, immediate: value },
                });
                return Ok((d, ty));
            }
            (BinaryOp::BitAnd, _) if mask_bounds(right).is_none() && unsigned_immediate(right).is_some() => {
                let (value, shifted) = unsigned_immediate(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(if shifted {
                    Instruction::AndImmediateShiftedRecord { a: d, s: a, immediate: value }
                } else {
                    Instruction::AndImmediateRecord { a: d, s: a, immediate: value }
                });
                return Ok((d, ty));
            }
            (BinaryOp::BitAnd, _) if mask_bounds(right).is_some() => {
                let (begin, end) = mask_bounds(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                return Ok((d, ty));
            }
            // A mask with one contiguous hole wraps around: `rlwinm d,a,0,mb,me`
            // with mb > me.
            (BinaryOp::BitAnd, _) if wrapped_mask_bounds(right).is_some() && !toggle("MWCC_PCODE_NO_WRAPPED_MASKS") => {
                let (begin, end) = wrapped_mask_bounds(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                return Ok((d, ty));
            }
            (BinaryOp::ShiftLeft, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.result(target);
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift: shift as u8 });
                return Ok((d, ty));
            }
            (BinaryOp::ShiftRight, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.result(target);
                let shift = shift as u8;
                self.emit_plain(if is_unsigned(ty) {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: a, shift }
                } else {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: a, shift }
                });
                return Ok((d, ty));
            }
            _ => {}
        }
        // A returned sum of a variable and a computed sum builds the inner
        // sum in r3 (the variable, if it is there, moves aside).
        let returned_sum = returning
            && op == BinaryOp::Add
            && !self.unoptimized
            && matches!(ty, Type::Int | Type::UnsignedInt)
            && unpromoted(left).as_var().is_some()
            && matches!(right.kind, ExprKind::Binary(BinaryOp::Add, ..))
            && !toggle("MWCC_PCODE_NO_RETURNED_SUM");
        let b = match early {
            Some(b) => b,
            None if returned_sum => self.expression_with_target(right, Some(3))?.0,
            None => self.expression(right)?.0,
        };
        // MWCC places a leaf operand first in a commutative operation whose
        // other operand is computed (`a*b + c` -> `add r3,c,t`).
        let leaf = |e: &Expr| unpromoted(e).as_var().is_some();
        // A left operand computed from one register and constants (`a-1`,
        // `2-a`, `-a`) keeps source order.
        // (A same-variable leaf, `a*2 + a`, still goes first.)
        let right_var = unpromoted(right).as_var();
        let one_register = |e: &Expr| {
            let e = unpromoted(e);
            // (Or a value loaded through one: `(p->b & ~0x10) | x`.)
            let other = |v: &Expr| {
                unpromoted(v).as_var().is_some_and(|id| Some(id) != right_var)
                    || (matches!(&unpromoted(v).kind, ExprKind::Load { base, index: None, .. }
                        if base.as_var().is_some_and(|id| Some(id) != right_var) || matches!(base.kind, ExprKind::Global(_) | ExprKind::Int(_)))
                        && !toggle("MWCC_PCODE_NO_LOADED_ONE_REGISTER"))
            };
            match &e.kind {
                ExprKind::Binary(_, x, y) => (other(x) && y.as_int().is_some()) || (x.as_int().is_some() && other(y)),
                ExprKind::Unary(UnaryOp::Negate, x) => other(x),
                _ => false,
            }
        };
        // A loaded left operand keeps source order (`*p + b`).
        let loaded = match &unpromoted(left).kind {
            ExprKind::Global(_) => true,
            // (Any unindexed load at -O0.)
            ExprKind::Load { index: None, .. } if self.unoptimized && !toggle("MWCC_PCODE_O0_LOAD_SWAP") => true,
            // (An indexed one too in a compound update: `a[i] |= x`.)
            ExprKind::Load { .. } if self.unoptimized && (self.compound_store || toggle("MWCC_PCODE_O0_INDEXED_LOAD_KEEP")) => true,
            ExprKind::Load { base, index: None, .. } => {
                matches!(base.kind, ExprKind::Var(_) | ExprKind::GlobalAddress(_) | ExprKind::LocalAddress(_))
                    // (Through a global pointer: `gp->c | x`; or a fixed address.)
                    || (matches!(base.kind, ExprKind::Global(_) | ExprKind::Int(_)) && !toggle("MWCC_PCODE_GLOBAL_BASE_LOAD_SWAP"))
            }
            _ => false,
        }
            && std::env::var_os("MWCC_PCODE_LOAD_SWAP").is_none();
        // (GC/1.0-1.2.5n add an index to an absolute address in order.)
        let swap = op.is_commutative()
            && !leaf(left)
            && !loaded
            && !(self.unit.branch_preserving && self.absolute_base(left))
            && leaf(right)
            // (-O0 puts the leaf first after a product.)
            && (!one_register(left)
                || (self.unoptimized
                    && matches!(unpromoted(left).kind, ExprKind::Binary(BinaryOp::Multiply, ..))
                    // (A declared variable, not a compiler temporary.)
                    && unpromoted(right).as_var().is_some_and(|id| {
                        self.function.variables[id].kind != VariableKind::Temporary && !self.function.variables[id].name.starts_with('@')
                    })
                    && !toggle("MWCC_PCODE_O0_ONE_REGISTER_ORDER"))
                || std::env::var_os("MWCC_PCODE_LEAF_FIRST_ALWAYS").is_some());
        let swap = swap || need_order;
        // (A call's result goes second in a commutative operation.)
        let call_order = !toggle("MWCC_PCODE_CALL_OPERAND_IN_ORDER");
        let swap = if call_order && op.is_commutative() && contains_call(left) != contains_call(right) {
            contains_call(left)
        } else {
            swap
        };
        let (a, b) = if swap { (b, a) } else { (a, b) };
        let d = self.result(target);
        let instruction = match op {
            BinaryOp::Add => Instruction::Add { d, a, b },
            BinaryOp::Subtract => Instruction::SubtractFrom { d, a: b, b: a },
            BinaryOp::Multiply => Instruction::MultiplyLow { d, a, b },
            BinaryOp::BitAnd => Instruction::And { a: d, s: a, b },
            BinaryOp::BitOr => Instruction::Or { a: d, s: a, b },
            BinaryOp::BitXor => Instruction::Xor { a: d, s: a, b },
            BinaryOp::ShiftLeft => Instruction::ShiftLeftWord { a: d, s: a, b },
            BinaryOp::ShiftRight if is_unsigned(ty) => Instruction::ShiftRightWord { a: d, s: a, b },
            BinaryOp::ShiftRight => Instruction::ShiftRightAlgebraicWord { a: d, s: a, b },
            other => return Err(unsupported(format!("binary operator {other:?}"))),
        };
        self.emit_plain(instruction);
        Ok((d, ty))
    }

    /// Integer `/` and `%`: `divw`/`divwu`, shifts for powers of two, and
    /// multiply-high by a magic reciprocal for other constants.
    fn division(&mut self, op: BinaryOp, left: &Expr, right: &Expr, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let unsigned = is_unsigned(ty);
        let divisor = right.as_int().map(|value| value as i32).filter(|&value| value != 0);
        let Some(divisor) = divisor else {
            return self.divide_registers(op, left, right, ty, target);
        };
        let magnitude = divisor.unsigned_abs();
        let power_of_two = if unsigned { (divisor as u32).is_power_of_two() } else { magnitude.is_power_of_two() && magnitude < 0x8000_0000 };
        if !power_of_two && !self.unit.magic_division {
            return self.divide_registers(op, left, right, ty, target);
        }
        self.constant_division(op, left, divisor, ty, target)
    }

    /// `divw`/`divwu` (and `mullw; subf` for a remainder).
    fn divide_registers(&mut self, op: BinaryOp, left: &Expr, right: &Expr, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let unsigned = is_unsigned(ty);
        {
            // (A divisor that calls is evaluated first.)
            let (x, y) = if contains_call(right) && !toggle("MWCC_PCODE_CALL_OPERAND_IN_ORDER") {
                let (y, _) = self.expression(right)?;
                let (x, _) = self.expression(left)?;
                (x, y)
            } else {
                let (x, _) = self.expression(left)?;
                let (y, _) = self.expression(right)?;
                (x, y)
            };
            let quotient = if op == BinaryOp::Divide { self.result(target) } else { self.temporary() };
            self.emit_plain(if unsigned {
                Instruction::DivideWordUnsigned { d: quotient, a: x, b: y }
            } else {
                Instruction::DivideWord { d: quotient, a: x, b: y }
            });
            if op == BinaryOp::Divide {
                return Ok((quotient, ty));
            }
            let product = self.temporary();
            self.emit_plain(Instruction::MultiplyLow { d: product, a: quotient, b: y });
            let d = self.result(target);
            self.emit_plain(Instruction::SubtractFrom { d, a: product, b: x });
            Ok((d, ty))
        }
    }

    /// Division by a nonzero constant: shifts for a power of two, else the
    /// magic-number multiply.
    fn constant_division(&mut self, op: BinaryOp, left: &Expr, divisor: i32, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let unsigned = is_unsigned(ty);
        let magnitude = divisor.unsigned_abs();
        if unsigned && (divisor as u32).is_power_of_two() {
            let k = (divisor as u32).trailing_zeros() as u8;
            let (x, _) = self.expression(left)?;
            if op == BinaryOp::Divide && k == 0 {
                return Ok((x, ty));
            }
            let d = self.result(target);
            self.emit_plain(if op == BinaryOp::Divide {
                Instruction::ShiftRightLogicalImmediate { a: d, s: x, shift: k }
            } else {
                Instruction::RotateAndMask { a: d, s: x, shift: 0, begin: 32 - k, end: 31 }
            });
            return Ok((d, ty));
        }
        if !unsigned && magnitude.is_power_of_two() && magnitude < 0x8000_0000 {
            let k = magnitude.trailing_zeros() as u8;
            if op == BinaryOp::Modulo {
                if divisor < 0 || k == 0 {
                    return Err(unsupported("remainder by this power of two"));
                }
                // (GC/1.0-1.2.5n: x - (x / 2^k << k), `srawi; addze; slwi;
                // subfc`.)
                // (The quotient in place in one register.)
                if self.unit.early_frame && !toggle("MWCC_PCODE_EARLY_ROTATED_REMAINDER") {
                    let (x, _) = self.expression(left)?;
                    let quotient = self.temporary();
                    self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: quotient, s: x, shift: k });
                    for instruction in [
                        Instruction::AddToZeroExtended { d: quotient, a: quotient },
                        Instruction::ShiftLeftImmediate { a: quotient, s: quotient, shift: k },
                    ] {
                        let mut step = PInstr::new(instruction);
                        step.flags.in_place = true;
                        self.emit(step);
                    }
                    let d = self.result(target);
                    self.emit_plain(Instruction::SubtractFromCarrying { d, a: quotient, b: x });
                    return Ok((d, ty));
                }
                // (`x % 2`: the low bit, sign-adjusted: `srwi s,x,31; clrlwi
                // t,x,31; xor t,t,s; subf d,s,t`.)
                if k == 1 && !toggle("MWCC_PCODE_ROTATED_REMAINDER_TWO") {
                    let (x, _) = self.expression(left)?;
                    let sign = self.temporary();
                    self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: x, shift: 31 });
                    let bit = self.temporary();
                    self.emit_plain(Instruction::RotateAndMask { a: bit, s: x, shift: 0, begin: 31, end: 31 });
                    let flipped = self.temporary();
                    self.emit_plain(Instruction::Xor { a: flipped, s: bit, b: sign });
                    let d = self.result(target);
                    self.emit_plain(Instruction::SubtractFrom { d, a: sign, b: flipped });
                    return Ok((d, ty));
                }
                // slwi t,x,32-k; srwi s,x,31; subf t,s,t; rotlwi t,t,k; add d,t,s
                let (x, _) = self.expression(left)?;
                let low = self.temporary();
                self.emit_plain(Instruction::ShiftLeftImmediate { a: low, s: x, shift: 32 - k });
                let sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: x, shift: 31 });
                let adjusted = self.temporary();
                self.emit_plain(Instruction::SubtractFrom { d: adjusted, a: sign, b: low });
                let rotated = self.temporary();
                self.emit_plain(Instruction::RotateAndMask { a: rotated, s: adjusted, shift: k, begin: 0, end: 31 });
                let d = self.result(target);
                self.emit_plain(Instruction::Add { d, a: rotated, b: sign });
                return Ok((d, ty));
            }
            // (After the early builds, a zero-extended narrow dividend is
            // never negative: a logical shift.)
            if divisor > 1
                && !self.unit.branch_preserving
                && matches!(&left.kind, ExprKind::Convert(inner) if matches!(inner.ty, Type::UnsignedChar | Type::UnsignedShort))
                && !toggle("MWCC_PCODE_NARROW_DIVIDE_SIGNED")
            {
                // (A variable through its narrow-shift fold; anything else as
                // an unsigned word.)
                let operand = if matches!(unpromoted(left).kind, ExprKind::Var(_)) {
                    left.clone()
                } else {
                    Expr { kind: left.kind.clone(), ty: Type::UnsignedInt }
                };
                let shifted = Expr::binary(BinaryOp::ShiftRight, operand.clone(), Expr::typed_int(i64::from(k), Type::Int), operand.ty);
                let (d, _) = self.expression_with_target(&shifted, target)?;
                return Ok((d, ty));
            }
            let (x, _) = self.expression(left)?;
            if k == 0 {
                if divisor > 0 {
                    return Ok((x, ty));
                }
                let d = self.result(target);
                self.emit_plain(Instruction::Negate { d, a: x });
                return Ok((d, ty));
            }
            if divisor < 0 && k == 1 {
                return Err(unsupported("division by -2"));
            }
            let d = self.result(target);
            let quotient = if divisor < 0 { self.temporary() } else { d };
            if k == 1 {
                // srwi t,x,31; add t,t,x; srawi q,t,1
                let sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: x, shift: 31 });
                let sum = self.temporary();
                self.emit_plain(Instruction::Add { d: sum, a: sign, b: x });
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: quotient, s: sum, shift: 1 });
            } else if self.unit.early_frame && divisor > 0 && !toggle("MWCC_PCODE_EARLY_SEPARATE_QUOTIENT") {
                // (GC/1.0-1.2.5n: in the quotient's register, `srawi q,x,k;
                // addze q,q`.)
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: quotient, s: x, shift: k });
                let mut carry = PInstr::new(Instruction::AddToZeroExtended { d: quotient, a: quotient });
                carry.flags.in_place = true;
                self.emit(carry);
            } else {
                // srawi t,x,k; addze q,t
                let shifted = self.temporary();
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: shifted, s: x, shift: k });
                self.emit_plain(Instruction::AddToZeroExtended { d: quotient, a: shifted });
            }
            if divisor < 0 {
                self.emit_plain(Instruction::Negate { d, a: quotient });
            }
            return Ok((d, ty));
        }
        if !unsigned && divisor < 0 {
            return Err(unsupported("division by a negative constant"));
        }
        let (x, _) = self.expression(left)?;
        let quotient = if op == BinaryOp::Divide { self.result(target) } else { self.temporary() };
        if unsigned {
            let (magic, add, shift) = unsigned_magic(divisor as u32);
            let (m, _) = self.expression(&Expr::int(i64::from(magic as i32)))?;
            let high = self.temporary();
            self.emit_plain(Instruction::MultiplyHighWordUnsigned { d: high, a: m, b: x });
            if add {
                // q = (((x - t) >> 1) + t) >> (s - 1)
                let difference = self.temporary();
                self.emit_plain(Instruction::SubtractFrom { d: difference, a: high, b: x });
                let halved = self.temporary();
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: halved, s: difference, shift: 1 });
                let sum = self.temporary();
                self.emit_plain(Instruction::Add { d: sum, a: halved, b: high });
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: quotient, s: sum, shift: shift as u8 - 1 });
            } else if shift == 0 {
                self.emit_plain(Instruction::Or { a: quotient, s: high, b: high });
            } else {
                self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: quotient, s: high, shift: shift as u8 });
            }
        } else {
            let (magic, shift) = signed_magic(divisor);
            let (m, _) = self.expression(&Expr::int(i64::from(magic)))?;
            let mut value = self.temporary();
            self.emit_plain(Instruction::MultiplyHighWord { d: value, a: m, b: x });
            // The product is adjusted in place.
            let in_place = std::env::var_os("MWCC_PCODE_DIV_FRESH").is_none();
            if magic < 0 {
                let sum = if in_place { value } else { self.temporary() };
                self.emit_plain(Instruction::Add { d: sum, a: value, b: x });
                value = sum;
            }
            if shift > 0 {
                let shifted = if in_place { value } else { self.temporary() };
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: shifted, s: value, shift: shift as u8 });
                value = shifted;
            }
            let sign = self.temporary();
            self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: value, shift: 31 });
            self.emit_plain(Instruction::Add { d: quotient, a: value, b: sign });
        }
        if op == BinaryOp::Divide {
            return Ok((quotient, ty));
        }
        let product = self.temporary();
        match i16::try_from(divisor) {
            Ok(immediate) => self.emit_plain(Instruction::MultiplyImmediate { d: product, a: quotient, immediate }),
            Err(_) => return Err(unsupported("remainder by a wide constant")),
        }
        let d = self.result(target);
        self.emit_plain(Instruction::SubtractFrom { d, a: product, b: x });
        Ok((d, ty))
    }

    // ------------------------------------------------------------ stores

    /// The register holding a stored value: a raw narrow parameter at least
    /// as wide as the store needs no extension.
    /// An absolute (`lis`/`addi`) global address.
    /// -O0 `(b + x) + offset` with a variable index `x` and a displacement
    /// (or an absolute array): the parts `(b, x)`.
    fn unoptimized_indexed<'e>(&self, base: &'e Expr) -> Option<(&'e Expr, &'e Expr)> {
        if toggle("MWCC_PCODE_O0_NO_INDEXED_MEMBERS") {
            return None;
        }
        match &base.kind {
            ExprKind::Binary(BinaryOp::Add, b, x)
                if x.as_int().is_none()
                    && matches!(b.ty, Type::Pointer(_) | Type::StructPointer { .. })
                    && !matches!(x.ty, Type::Pointer(_) | Type::StructPointer { .. }) =>
            {
                Some((b, x))
            }
            _ => None,
        }
    }

    /// -O0 address of `(b + x) + offset`: an absolute array computes the
    /// index first, adds its address and keeps the displacement (`None`
    /// index); otherwise the base, then the index with the displacement
    /// added (`lwzx`).
    fn unoptimized_address(&mut self, base: &Expr, offset: i32) -> Compilation<(u32, Option<u32>, i32)> {
        let (mut b, x) = self.unoptimized_indexed(base).expect("checked");
        // An embedded array's constant member offset joins the displacement
        // (`p->array[i]`: `(p + 12) + i*4` as `p + (i*4 + 12)`).
        let mut offset = offset;
        if let ExprKind::Binary(BinaryOp::Add, pointer, addend) = &b.kind {
            if let (Some(addend), false) = (addend.as_int(), toggle("MWCC_PCODE_O0_NO_MEMBER_ARRAY_OFFSET")) {
                if !self.absolute_base(pointer) {
                    b = pointer;
                    offset += i32::try_from(addend).map_err(|_| unsupported("large member offset"))?;
                }
            }
        }
        if self.absolute_base(b) {
            let (index, _) = self.expression(x)?;
            let (address, _) = self.expression(b)?;
            // (GC/3.x indexes it.)
            if offset == 0 && self.unit.signed_promoted_truth && !toggle("MWCC_PCODE_O0_ABSOLUTE_ADD") {
                return Ok((address, Some(index), 0));
            }
            let sum = self.temporary();
            self.emit_plain(Instruction::Add { d: sum, a: address, b: index });
            return Ok((sum, None, offset));
        }
        let (address, _) = self.expression(b)?;
        let (index, _) = self.expression(x)?;
        if offset == 0 {
            return Ok((address, Some(index), 0));
        }
        // (GC/3.x adds the index to the base and keeps the displacement.)
        if self.unit.signed_promoted_truth && !toggle("MWCC_PCODE_O0_DISPLACED_INDEX") {
            let sum = self.temporary();
            self.emit_plain(Instruction::Add { d: sum, a: address, b: index });
            return Ok((sum, None, offset));
        }
        let immediate = i16::try_from(offset).map_err(|_| unsupported("large member offset"))?;
        let displaced = self.temporary();
        self.emit_based(Instruction::AddImmediate { d: displaced, a: index, immediate }, index);
        Ok((address, Some(displaced), 0))
    }

    /// The small-data object a base addresses (`&g`).
    /// An object addressed through its section anchor: (name, anchor).
    fn anchored_object(&self, base: &Expr) -> Option<(String, &'static str)> {
        match &base.kind {
            ExprKind::GlobalAddress(name) => self.anchored.get(name).map(|&anchor| (name.clone(), anchor)),
            _ => None,
        }
    }

    /// The section anchor's address (`lis; addi`), shared like a global's.
    fn anchor_register(&mut self, anchor: &'static str) -> u32 {
        let key = format!("&@{anchor}");
        let shared = !self.unoptimized;
        if shared {
            if let Some(&(register, _, _)) = self.common.get(&key) {
                return register;
            }
        }
        let d = self.absolute_address(anchor);
        if shared {
            self.common.insert(key, (d, Type::Pointer(mwcc_iro::Pointee::Int), Vec::new()));
        }
        d
    }

    /// A load of an anchored object: `op d,<offset>(anchor)`.
    fn anchored_load(&mut self, ty: Type, anchor: &'static str, name: &str, target: Option<u32>) -> Compilation<(u32, Type)> {
        let base = self.anchor_register(anchor);
        let loaded = self.load(ty, base, 0, None, target)?;
        let block = self.current_block();
        if let Some(last) = self.pcode.blocks[block].instructions.last_mut() {
            last.displacement_symbol = Some(name.to_owned());
        }
        Ok(loaded)
    }

    fn small_data_object<'e>(&self, base: &'e Expr) -> Option<&'e str> {
        match &base.kind {
            ExprKind::GlobalAddress(name)
                if self.unit.globals.get(name).is_some_and(|global| global.small_data && !global.is_function)
                    && !toggle("MWCC_PCODE_NO_SMALL_DATA_MEMBER_FOLD") =>
            {
                Some(name)
            }
            _ => None,
        }
    }

    /// -O0 `p + k` (an embedded array member of a non-absolute pointer):
    /// the member offset rides the index (`i*4 + k`, `lwzx p`).
    fn member_array<'e>(&self, base: &'e Expr) -> Option<(&'e Expr, i16)> {
        match &base.kind {
            ExprKind::Binary(BinaryOp::Add, pointer, addend)
                if !self.absolute_base(pointer)
                    && matches!(pointer.ty, Type::Pointer(_) | Type::StructPointer { .. })
                    && !toggle("MWCC_PCODE_O0_NO_MEMBER_ARRAY_OFFSET") =>
            {
                Some((pointer, i16::try_from(addend.as_int()?).ok()?))
            }
            _ => None,
        }
    }

    /// A register pointer variable whose pointee has no volatile storage.
    fn shareable_pointer(&self, base: &Expr) -> Option<VarId> {
        if self.unoptimized || toggle("MWCC_PCODE_NO_POINTER_LOAD_CSE") {
            return None;
        }
        match base.kind {
            ExprKind::Var(id)
                if self.homes[id].is_none()
                    && self.unit.nonvolatile_pointers.contains(&self.function.variables[id].name) =>
            {
                Some(id)
            }
            _ => None,
        }
    }

    fn absolute_base(&self, base: &Expr) -> bool {
        matches!(&base.kind, ExprKind::GlobalAddress(name) if !self.unit.globals[name].small_data)
    }

    fn store_source(&mut self, value: &Expr, stored: Type) -> Compilation<u32> {
        let (source, ty) = self.expression(value)?;
        // -O0 narrows a full-width value before a narrow store.
        // (Not a narrow value updated by a constant, `x--`, done in its own
        // type; not an assignment's value or a bit-field insert.)
        let narrow_update = matches!(&value.kind, ExprKind::Binary(op, left, right)
            if right.as_int().is_some() && is_narrow(unpromoted(left).ty)
                && (matches!(op, BinaryOp::Add | BinaryOp::Subtract) || toggle("MWCC_PCODE_O0_BITWISE_UPDATE_RAW")))
            // (A bitwise combination of narrow operands stays narrow.)
            || matches!(&value.kind, ExprKind::Binary(BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::BitAnd, left, right)
                if is_narrow(unpromoted(left).ty) && is_narrow(unpromoted(right).ty)
                    && !toggle("MWCC_PCODE_O0_NARROW_BITWISE_EXTENDS"));
        let already = match &value.kind {
            ExprKind::Var(id) => self.function.variables[*id].kind == VariableKind::Temporary,
            ExprKind::Idiom(Idiom::Insert { .. }) => true,
            _ => false,
        };
        if self.unoptimized
            && is_narrow(stored)
            && !is_narrow(ty)
            && value.as_int().is_none()
            && !is_float(ty)
            && !narrow_update
            && !already
        {
            let extended = self.temporary();
            self.emit_plain(extension(stored, extended, source));
            return Ok(extended);
        }
        // A raw narrow value stored as a wider narrow type is extended as
        // itself first (`lbz; extsb; sth`).
        if is_narrow(stored)
            && is_narrow(ty)
            && mwcc_iro::width(ty) < mwcc_iro::width(stored)
            && self.is_raw(value)
            && !(self.unoptimized && matches!(value.kind, ExprKind::Var(_)))
        {
            let extended = self.temporary();
            self.emit_plain(extension(ty, extended, source));
            return Ok(extended);
        }
        // -O0: a raw narrow variable stored as another narrow type is read
        // extended (as itself when narrower, else as the stored type).
        if self.unoptimized
            && is_narrow(stored)
            && is_narrow(ty)
            && ty != stored
            && matches!(value.kind, ExprKind::Var(_))
            && self.is_raw(value)
            && !toggle("MWCC_PCODE_O0_NO_RAW_STORE_EXTEND")
        {
            let as_type = if mwcc_iro::width(ty) < mwcc_iro::width(stored) { ty } else { stored };
            let extended = self.temporary();
            self.emit_plain(extension(as_type, extended, source));
            return Ok(extended);
        }
        Ok(source)
    }

    /// A store's address: base register, displacement, index register and
    /// relocation.
    fn store_address(&mut self, place: &Place) -> Compilation<(u32, i16, Option<u32>, Option<AttachedRelocation>)> {
        Ok(match place {
            Place::Memory { base, index: None, offset } if matches!(base.kind, ExprKind::LocalAddress(_)) => {
                let ExprKind::LocalAddress(id) = base.kind else { unreachable!() };
                let slot = self.frame_offsets[id].ok_or_else(|| unsupported("frame slot"))?;
                let offset = i16::try_from(i32::from(slot) + *offset).map_err(|_| unsupported("a large frame"))?;
                (1, offset, None, None)
            }
            Place::Memory { base, index: None, offset } if base.as_int().is_some() => {
                let (high, low) = split_address(base.as_int().expect("checked") + i64::from(*offset))?;
                let a = self.address_high(high)?;
                (a, low, None, None)
            }
            Place::Memory { base, index: None, offset: 0 } if self.anchored_object(base).is_some() => {
                let (name, anchor) = self.anchored_object(base).expect("checked");
                self.store_displacement = Some(name);
                (self.anchor_register(anchor), 0, None, None)
            }
            Place::Global(name) if self.anchored.contains_key(name) => {
                let anchor = self.anchored[name];
                self.store_displacement = Some(name.clone());
                (self.anchor_register(anchor), 0, None, None)
            }
            Place::Memory { base, index: None, offset: 0 } if self.small_data_object(base).is_some() => {
                let name = self.small_data_object(base).expect("checked").to_owned();
                (0, 0, None, Some(AttachedRelocation { kind: RelocationKind::EmbSda21, target: RelocationTarget::External(name) }))
            }
            Place::Memory { base, index: None, offset } if self.unoptimized && self.unoptimized_indexed(base).is_some() => {
                let (a, b, displacement) = self.unoptimized_address(base, *offset)?;
                match b {
                    Some(b) => (a, 0, Some(b), None),
                    None => (a, i16::try_from(displacement).map_err(|_| unsupported("large member offset"))?, None, None),
                }
            }
            Place::Memory { base: base_expression, index: Some(index), .. }
                if self.unoptimized && self.member_array(base_expression).is_some() =>
            {
                let (pointer, addend) = self.member_array(base_expression).expect("checked");
                let (b, _) = self.expression(index)?;
                if self.unit.signed_promoted_truth && !toggle("MWCC_PCODE_O0_DISPLACED_INDEX") {
                    let (a, _) = self.expression(pointer)?;
                    let sum = self.temporary();
                    self.emit_plain(Instruction::Add { d: sum, a, b });
                    (sum, addend, None, None)
                } else {
                    let displaced = self.temporary();
                    self.emit_based(Instruction::AddImmediate { d: displaced, a: b, immediate: addend }, b);
                    let (a, _) = self.expression(pointer)?;
                    (a, 0, Some(displaced), None)
                }
            }
            Place::Memory { base: base_expression, index: Some(index), .. } if self.unoptimized => {
                let (b, _) = self.expression(index)?;
                let (a, _) = self.expression(base_expression)?;
                if self.absolute_base(base_expression) && (!self.unit.signed_promoted_truth || toggle("MWCC_PCODE_O0_ABSOLUTE_ADD")) {
                    let address = self.temporary();
                    self.emit_plain(Instruction::Add { d: address, a, b });
                    (address, 0, None, None)
                } else {
                    (a, 0, Some(b), None)
                }
            }
            // (-O1/-O2: an absolute array's index first.)
            Place::Memory { base, index: Some(index), offset }
                if !self.unit.strength_reduction && self.absolute_base(base) && !toggle("MWCC_PCODE_O2_ADDRESS_FIRST") =>
            {
                let (b, _) = self.expression(index)?;
                let (a, _) = self.base_expression(base)?;
                (a, i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?, Some(b), None)
            }
            Place::Memory { base, index, offset } => {
                let (base, _) = self.base_expression(base)?;
                let index = match index {
                    Some(index) => Some(self.expression(index)?.0),
                    None => None,
                };
                let (base, offset) = match index {
                    None => self.displacement(base, *offset)?,
                    Some(_) => (base, i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?),
                };
                (base, offset, index, None)
            }
            Place::Global(name) => {
                let global = self.unit.globals[name];
                if global.small_data {
                    let relocation = AttachedRelocation {
                        kind: RelocationKind::EmbSda21,
                        target: RelocationTarget::External(name.clone()),
                    };
                    (0, 0, None, Some(relocation))
                } else {
                    (self.global_address(name), 0, None, None)
                }
            }
        })
    }

    /// A struct assignment: MWCC's block copy. -O4 moves words (whatever
    /// the alignment) in 8-byte load/load/store/store pairs, then a word,
    /// half and byte tail; above 64 bytes the pairs run in a `ctr` loop
    /// through pre-decremented pointers with update forms. -O0 moves units
    /// of the struct's alignment the same way, looping above 16 bytes.
    fn block_copy(&mut self, place: &Place, size: u32, align: u8, value: &Expr) -> Compilation<()> {
        let block = self.current_block();
        let start = self.pcode.blocks[block].instructions.len();
        self.block_copy_units(place, size, align, value)?;
        // (MWCC expands the copy after the first scheduling pass: other code
        // goes ahead of it.)
        // (Only a local's initializer image.)
        let image = matches!(&value.kind, ExprKind::Load { base, .. } if matches!(base.kind, ExprKind::Image(_)));
        if image && self.current_block() == block && !toggle("MWCC_PCODE_NO_LATE_COPIES") {
            for instruction in &mut self.pcode.blocks[block].instructions[start..] {
                let name = format!("{:?}", instruction.instruction);
                instruction.flags.block_copy = name.starts_with("Load") || name.starts_with("Store");
            }
        }
        Ok(())
    }

    fn block_copy_units(&mut self, place: &Place, size: u32, align: u8, value: &Expr) -> Compilation<()> {
        self.forget_frame_loads();
        let ExprKind::Load { base: source, index: None, offset: source_offset } = &value.kind else {
            return Err(unsupported("struct copy from this value"));
        };
        let destination = match place {
            Place::Memory { base, index: None, offset } => (base.as_ref().clone(), *offset),
            Place::Global(name) => (
                Expr { kind: ExprKind::GlobalAddress(name.clone()), ty: Type::StructPointer { element_size: size } },
                0,
            ),
            _ => return Err(unsupported("struct copy to this place")),
        };
        let unit: u32 = if self.unoptimized { u32::from(align.clamp(1, 4)) } else { 4 };
        // (-O0 still unrolls an initializer image like -O4.)
        let image_source = matches!(source.kind, ExprKind::Image(_)) && !toggle("MWCC_PCODE_O0_IMAGE_LOOPS");
        let limit = if self.unoptimized && !image_source { 16 } else { 64 };
        // A small constant image is read unit by unit through the pool
        // (`lwz r,@N+k@sda21`).
        if let ExprKind::Image(index) = source.kind {
            let image = self.function.images[index].clone();
            if self.small_image(image.len()) && *source_offset == 0 && size as usize <= image.len() {
                return self.copy_small_image(place, &destination, &image, size, unit);
            }
        }
        // A constant image (MWCC still orders its loads like any memory).
        let read_only = matches!(source.kind, ExprKind::Image(_));
        let unordered = read_only && toggle("MWCC_PCODE_IMAGE_LOADS_UNORDERED");
        // A frame object is stored through r1 directly.
        let frame = match destination.0.kind {
            ExprKind::LocalAddress(id) => {
                let slot = self.frame_offsets[id].ok_or_else(|| unsupported("frame slot"))?;
                Some(i16::try_from(i32::from(slot) + destination.1).map_err(|_| unsupported("a large frame"))?)
            }
            _ => None,
        };
        let looping = size > limit;
        // (-O0 sets a frame destination's loop pointer first.)
        let early_destination = match frame {
            Some(slot) if looping && self.unoptimized => {
                let ds = self.temporary();
                self.emit_based(Instruction::AddImmediate { d: ds, a: 1, immediate: slot - unit as i16 }, 1);
                Some(ds)
            }
            _ => None,
        };
        let (s, _) = self.expression(source)?;
        let (s, s_offset) = self.displacement(s, *source_offset)?;
        let (d, d_offset) = match frame {
            Some(slot) => (1, slot),
            None => {
                let (d, _) = self.expression(&destination.0)?;
                self.displacement(d, destination.1)?
            }
        };
        let copy_load_at = |this: &mut Self, width: u32, value: u32, base: u32, offset: i16, update: bool| {
            let mut load = PInstr::new(copy_load(width, value, base, offset, update));
            load.not_r0.push(base);
            load.flags.read_only = unordered;
            this.emit(load);
        };
        // -O4 from a constant image or a global: every unit loaded, then
        // every unit stored.
        let absolute_source = matches!(source.kind, ExprKind::GlobalAddress(_)) && !toggle("MWCC_PCODE_FRAME_LOAD_ALL");
        let load_all = read_only || absolute_source || (frame.is_some() && toggle("MWCC_PCODE_FRAME_LOAD_ALL"));
        if size <= limit && load_all && !self.unoptimized && !toggle("MWCC_PCODE_IMAGE_PAIRS") {
            let mut units = Vec::new();
            let mut at = 0u32;
            while at < size {
                let width = if size - at >= 4 { 4 } else { tail_width(size - at) };
                let value = self.temporary();
                copy_load_at(self, width, value, s, at_offset(s_offset, at)?, false);
                units.push((width, value, at));
                at += width;
            }
            for (width, value, at) in units {
                self.emit_based(copy_store(width, value, d, at_offset(d_offset, at)?, false), d);
            }
            return Ok(());
        }
        if size <= limit {
            let mut at = 0u32;
            while size - at >= 8 {
                // An 8-byte chunk as unit pairs.
                let chunk_end = at + 8;
                while at < chunk_end {
                    let (first, second) = (self.temporary(), self.temporary());
                    copy_load_at(self, unit, first, s, at_offset(s_offset, at)?, false);
                    copy_load_at(self, unit, second, s, at_offset(s_offset, at + unit)?, false);
                    self.emit_based(copy_store(unit, first, d, at_offset(d_offset, at)?, false), d);
                    self.emit_based(copy_store(unit, second, d, at_offset(d_offset, at + unit)?, false), d);
                    at += 2 * unit;
                }
            }
            while at < size {
                let width = if self.unoptimized { unit } else { tail_width(size - at) };
                self.copy_single(width, s, at_offset(s_offset, at)?, d, at_offset(d_offset, at)?);
                at += width;
            }
            return Ok(());
        }
        // The loop: pointers one unit back, `size / (2 * unit)` pairs.
        let pairs = size / unit / 2;
        // (Optimized, the count loads first; -O0 after the pointers.)
        let count = self.temporary();
        if !self.unoptimized {
            let value = i16::try_from(pairs).map_err(|_| unsupported("a large struct copy"))?;
            let mut li = PInstr::new(Instruction::AddImmediate { d: count, a: 0, immediate: value });
            li.flags.serialize = !toggle("MWCC_PCODE_FREE_COPY_MTCTR");
            self.emit(li);
        }
        let ds = match early_destination {
            Some(ds) => ds,
            None => {
                let ds = self.temporary();
                self.emit_based(Instruction::AddImmediate { d: ds, a: d, immediate: at_offset(d_offset, 0)? - unit as i16 }, d);
                ds
            }
        };
        let ss = self.temporary();
        self.emit_based(Instruction::AddImmediate { d: ss, a: s, immediate: at_offset(s_offset, 0)? - unit as i16 }, s);
        if self.unoptimized {
            self.load_constant(count, i64::from(pairs))?;
        }
        // (`mtctr` stays after the pointer setup.)
        let mut mtctr = PInstr::new(Instruction::MoveToCountRegister { s: count });
        mtctr.flags.serialize = !toggle("MWCC_PCODE_FREE_COPY_MTCTR");
        self.emit(mtctr);
        let top = self.new_join_label();
        self.place_label(top);
        let (first, second) = (self.temporary(), self.temporary());
        copy_load_at(self, unit, first, ss, unit as i16, false);
        copy_load_at(self, unit, second, ss, 2 * unit as i16, true);
        self.emit_based(copy_store(unit, first, ds, unit as i16, false), ds);
        let mut stored = PInstr::new(copy_store(unit, second, ds, 2 * unit as i16, true));
        stored.not_r0.push(ds);
        stored.implicit_defs.push(Register::general(ds));
        self.emit(stored);
        self.branch(Instruction::BranchConditionalForward { options: 16, condition_bit: 0, target: 0 }, top);
        // The tail, from the updated pointers.
        let done = pairs * 2 * unit;
        let mut at = done;
        while at < size {
            let width = if self.unoptimized { unit } else { tail_width(size - at) };
            let relative = (at - done + unit) as i16;
            self.copy_single(width, ss, relative, ds, relative);
            at += width;
        }
        Ok(())
    }

    /// Images of at most 8 bytes live in the small-data pool.
    fn small_image(&self, bytes: usize) -> bool {
        self.unit.pool_small_data && bytes <= 8 && !toggle("MWCC_PCODE_NO_SMALL_IMAGES")
    }

    /// The `.rodata` blob holding image `index` (one per image).
    fn rodata_blob(&mut self, index: usize) -> usize {
        let bytes = self.function.images[index].clone();
        if let Some(blob) = self.pcode.rodata_images.iter().position(|(existing, _)| *existing == bytes) {
            return blob;
        }
        self.pcode.rodata_images.push((bytes, 4));
        self.pcode.rodata_images.len() - 1
    }

    /// A block copy from a small pooled image: unit pairs per 8-byte chunk,
    /// then single units, each load relocated into the pool entry.
    fn copy_small_image(&mut self, place: &Place, destination: &(Expr, i32), image: &[u8], size: u32, unit: u32) -> Compilation<()> {
        let _ = place;
        let mut padded = image.to_vec();
        let width: u8 = if padded.len() <= 4 { 4 } else { 8 };
        padded.resize(usize::from(width), 0);
        let bits = padded.iter().fold(0u64, |bits, &byte| (bits << 8) | u64::from(byte));
        let index = match self.pcode.pool.iter().position(|&entry| entry == (bits, width)) {
            Some(index) => index,
            None => {
                self.pcode.pool.push((bits, width));
                self.pcode.pool.len() - 1
            }
        };
        let (d, d_offset) = match destination.0.kind {
            ExprKind::LocalAddress(id) => {
                let slot = self.frame_offsets[id].ok_or_else(|| unsupported("frame slot"))?;
                (1, i16::try_from(i32::from(slot) + destination.1).map_err(|_| unsupported("a large frame"))?)
            }
            _ => {
                let (d, _) = self.expression(&destination.0)?;
                self.displacement(d, destination.1)?
            }
        };
        let pooled = |this: &mut Self, width: u32, value: u32, at: u32| {
            let mut load = PInstr::new(copy_load(width, value, 0, 0, false));
            load.flags.read_only = true;
            load.relocation = Some(AttachedRelocation {
                kind: RelocationKind::EmbSda21,
                target: if at == 0 {
                    RelocationTarget::Constant(index)
                } else {
                    RelocationTarget::ConstantWithAddend(index, at as i32)
                },
            });
            this.emit(load);
        };
        let mut at = 0u32;
        while size - at >= 8 {
            let chunk_end = at + 8;
            while at < chunk_end {
                let (first, second) = (self.temporary(), self.temporary());
                pooled(self, unit, first, at);
                pooled(self, unit, second, at + unit);
                self.emit_based(copy_store(unit, first, d, at_offset(d_offset, at)?, false), d);
                self.emit_based(copy_store(unit, second, d, at_offset(d_offset, at + unit)?, false), d);
                at += 2 * unit;
            }
        }
        while at < size {
            let width = if self.unoptimized { unit } else { tail_width(size - at) };
            let value = self.temporary();
            pooled(self, width, value, at);
            self.emit_based(copy_store(width, value, d, at_offset(d_offset, at)?, false), d);
            at += width;
        }
        Ok(())
    }

    /// One unit of a block copy (its tail).
    fn copy_single(&mut self, width: u32, s: u32, source_offset: i16, d: u32, destination_offset: i16) {
        let value = self.temporary();
        self.emit_based(copy_load(width, value, s, source_offset, false), s);
        self.emit_based(copy_store(width, value, d, destination_offset, false), d);
    }

    fn store(&mut self, place: &Place, ty: Type, value: &Expr) -> Compilation<()> {
        if let Type::Struct { size, align } = ty {
            if toggle("MWCC_PCODE_NO_STRUCT_COPY") {
                return Err(unsupported("struct copy"));
            }
            return self.block_copy(place, size, align, value);
        }
        if is_float(ty) != is_float(value.ty) {
            return Err(unsupported("store of a value in the other register class"));
        }
        // `a[i] op= v` (-O0): the address once, first; the value's load of
        // the same place reuses it.
        let compound = if self.unoptimized && !toggle("MWCC_PCODE_O0_NO_COMPOUND_ADDRESS") {
            compound_place_key(place, value)
        } else {
            None
        };
        let (source, (base, offset, index, relocation)) = match compound {
            Some(key) => {
                if let ExprKind::Idiom(Idiom::Insert { value: inserted, .. }) = &value.kind {
                    if !toggle("MWCC_PCODE_O0_INSERT_ADDRESS_FIRST") {
                        // (A promoted narrow value is inserted as it is.)
                        let source = match &inserted.kind {
                            ExprKind::Convert(inner) if is_narrow(inner.ty) && !toggle("MWCC_PCODE_O0_UNCONVERTED_INSERT") => inner.as_ref(),
                            _ => inserted.as_ref(),
                        };
                        let (x, _) = self.expression(source)?;
                        self.inserted = Some((inserted.as_ref() as *const Expr as usize, x));
                    }
                }
                let address = self.store_address(place)?;
                self.address_reuse = Some((key, address.clone()));
                let source = self.store_source(value, ty);
                self.address_reuse = None;
                (source?, address)
            }
            None => {
                let source = self.store_source(value, ty)?;
                (source, self.store_address(place)?)
            }
        };
        let store = match (index, ty) {
            (Some(b), Type::Float) => Instruction::StoreFloatSingleIndexed { s: source, a: base, b },
            (Some(b), Type::Double) => Instruction::StoreFloatDoubleIndexed { s: source, a: base, b },
            (None, Type::Float) => Instruction::StoreFloatSingle { s: source, a: base, offset },
            (None, Type::Double) => Instruction::StoreFloatDouble { s: source, a: base, offset },
            (Some(b), Type::Short | Type::UnsignedShort) => Instruction::StoreHalfwordIndexed { s: source, a: base, b },
            (Some(b), Type::Char | Type::UnsignedChar) => Instruction::StoreByteIndexed { s: source, a: base, b },
            (Some(b), _) => Instruction::StoreWordIndexed { s: source, a: base, b },
            (None, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }) => {
                Instruction::StoreWord { s: source, a: base, offset }
            }
            (None, Type::Short | Type::UnsignedShort) => Instruction::StoreHalfword { s: source, a: base, offset },
            (None, Type::Char | Type::UnsignedChar) => Instruction::StoreByte { s: source, a: base, offset },
            (None, other) => return Err(unsupported(format!("store of {other:?}"))),
        };
        let mut instruction = PInstr::new(store);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.flags.compound = self.compound_store;
        instruction.flags.nonvolatile_base = matches!(place, Place::Memory { base, .. } if self.shareable_pointer(base).is_some());
        instruction.relocation = relocation;
        instruction.displacement_symbol = self.store_displacement.take();
        if instruction.relocation.is_none() && instruction.displacement_symbol.is_none() && !self.unit.early_frame && !toggle("MWCC_PCODE_NO_GLOBAL_OBJECTS") {
            if let Place::Memory { base, .. } = place {
                instruction.object = root_global(base);
            }
        }
        self.emit(instruction);
        // (A store to a frame object no pointer reaches keeps the loads.)
        let private = matches!(place, Place::Memory { base, .. }
            if matches!(base.kind, ExprKind::LocalAddress(id) if !self.escaping[id]))
            && !toggle("MWCC_PCODE_PRIVATE_STORES_FORGET");
        if !private {
            self.forget_loaded_globals(false);
            let escaping = self.escaping.clone();
            // (GC/3.x: loads through the same non-volatile pointer at other
            // bytes stay valid.)
            let disjoint = match place {
                Place::Memory { base, index: None, offset } if self.unit.forwards_stores && !toggle("MWCC_PCODE_NO_DISJOINT_LOAD_CACHE") => {
                    self.shareable_pointer(base).map(|id| (id, *offset, mwcc_iro::width(ty) as i32))
                }
                _ => None,
            };
            self.common.retain(|key, _| {
                if let Some((id, offset, bytes)) = disjoint {
                    let mut parts = key.trim_start_matches('*').split(':');
                    if let (Some(pointer), Some(at), Some(kind)) = (parts.next(), parts.next(), parts.next()) {
                        if pointer.parse::<usize>().ok() == Some(id) {
                            if let Ok(at) = at.parse::<i32>() {
                                let size = match kind {
                                    "word" | "Float" | "Int" | "UnsignedInt" => 4,
                                    "Double" => 8,
                                    "Short" | "UnsignedShort" => 2,
                                    "Char" | "UnsignedChar" => 1,
                                    _ => i32::MAX / 2,
                                };
                                if key.starts_with('*') && (at + size <= offset || offset + bytes <= at) {
                                    return true;
                                }
                            }
                        }
                    }
                }
                !key.starts_with('*') || private_frame_key(key, &escaping)
            });
        } else if let Place::Memory { base, .. } = place {
            // (A private frame object's own known loads change.)
            if let ExprKind::LocalAddress(id) = base.kind {
                let prefix = format!("*F{id}:");
                self.common.retain(|key, _| !key.starts_with(&prefix));
            }
        }
        // (GC/3.x: a word stored through a pointer to non-volatile storage
        // is that place's value until the next store or call.)
        if let (Place::Memory { base, index: None, offset }, true) = (place, self.unit.forwards_stores) {
            if let Some(id) = self.shareable_pointer(base).filter(|_| !is_narrow(ty) && !toggle("MWCC_PCODE_NO_STORE_FORWARDING")) {
                if source >= 32 {
                    // (Also stale once a variable holding the value changes.)
                    let mut read = vec![id];
                    read.extend((0..self.function.variables.len()).filter(|&owner| self.registers[owner] == Some(source)));
                    self.common.insert(load_key(id, *offset, ty), (source, ty, read));
                }
            }
        }
        // A stored word-sized global's value is still in `source`.
        if let Place::Global(name) = place {
            let global = self.unit.globals[name];
            if !global.is_volatile && !is_narrow(ty) && !self.unoptimized {
                self.loaded_globals.insert(name.clone(), (source, ty));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ calls

    /// A string literal addressed from small data (`li d,@sda21`).
    fn small_string(&self, expression: &Expr) -> bool {
        matches!(expression.kind, ExprKind::StringAddress(index)
            if self.unit.strings_small_data && self.function.strings[index].len() + 1 <= 8)
    }

    fn call(&mut self, name: &str, arguments: &[Expr], ty: Type, target: Option<u32>) -> Compilation<u32> {
        // An indirect call's first argument is the target address.
        let (callee, arguments) = if name == mwcc_iro::INDIRECT_CALL {
            (Some(&arguments[0]), &arguments[1..])
        } else {
            (None, arguments)
        };
        // Arguments that call are evaluated first, then the others in order.
        let calls: Vec<bool> = arguments
            .iter()
            .map(|argument| !toggle("MWCC_PCODE_ARGUMENTS_IN_ORDER") && format!("{:?}", argument.kind).contains("Call {"))
            .collect();
        let mut values = vec![None; arguments.len()];
        // (A wide argument's register pair.)
        let mut wide_values: Vec<Option<(u32, u32)>> = vec![None; arguments.len()];
        // -O0 evaluates an argument without calls straight into its
        // register (in order, after those that call).
        let mut registers = Vec::with_capacity(arguments.len());
        let (mut general, mut float) = (FIRST_GENERAL_ARGUMENT, 1);
        for argument in arguments {
            if is_float(argument.ty) {
                registers.push(float);
                float += 1;
            } else if is_wide(argument.ty) {
                // (An odd-aligned pair.)
                general += 1 - general % 2;
                registers.push(general);
                general += 2;
            } else {
                registers.push(general);
                general += 1;
            }
        }
        for pass in [true, false] {
            for (index, argument) in arguments.iter().enumerate() {
                if calls[index] != pass {
                    continue;
                }
                // (Optimized, a string's address too: in order, or with the
                // constants when it is in small data.)
                let string = matches!(argument.kind, ExprKind::StringAddress(_))
                    && !toggle("MWCC_PCODE_STRING_ARGUMENT_TEMPORARY");
                let direct = (self.unoptimized || string)
                    && !pass
                    && !is_float(argument.ty)
                    && !toggle("MWCC_PCODE_O0_ARGUMENT_TEMPORARIES");
                if is_wide(argument.ty) {
                    // (A constant loads with the other constants.)
                    if long_constant(argument).is_none() {
                        wide_values[index] = Some(self.wide(argument)?);
                    }
                    continue;
                }
                // (An argument past r10 is computed, then stored.)
                if !is_float(argument.ty) && registers[index] > LAST_GENERAL_ARGUMENT {
                    values[index] = Some(self.expression(argument)?.0);
                    continue;
                }
                // A constant argument is loaded straight into its register.
                values[index] = match argument.as_int() {
                    Some(_) if !self.unoptimized => None,
                    None if string && !self.unoptimized && self.small_string(argument) => None,
                    _ if direct => {
                        let (value, _) = self.expression_with_target(argument, Some(registers[index]))?;
                        if value != registers[index] {
                            self.emit_plain(Instruction::Or { a: registers[index], s: value, b: value });
                        }
                        Some(registers[index])
                    }
                    // (A signed byte extends in place, in its own register.)
                    _ if matches!(&argument.kind, ExprKind::Convert(operand) if operand.ty == Type::Char)
                        && !toggle("MWCC_PCODE_NO_ARGUMENT_INPLACE_EXTSB") =>
                    {
                        let own = self.temporary();
                        Some(self.expression_with_target(argument, Some(own))?.0)
                    }
                    // (Optimized, after the early builds, a computed word
                    // goes straight to its register too.)
                    _ if !pass
                        && !self.unit.early_frame
                        && !is_float(argument.ty)
                        && !plain_variable(argument)
                        // (A global's address too once another argument called.)
                        && (!matches!(argument.kind, ExprKind::GlobalAddress(..))
                            || (arguments.iter().any(contains_call) && !toggle("MWCC_PCODE_NO_GLOBAL_ARGUMENT_AFTER_CALL")))
                        // (A frame address with a global's waits for the copies.)
                        && !(matches!(argument.kind, ExprKind::LocalAddress(_))
                            && arguments.iter().any(|other| matches!(other.kind, ExprKind::GlobalAddress(..))))
                        && !toggle("MWCC_PCODE_COMPUTED_ARGUMENT_TEMPORARIES") =>
                    {
                        // (A value found elsewhere is copied with the others.)
                        Some(self.expression_with_target(argument, Some(registers[index]))?.0)
                    }
                    _ => Some(self.expression(argument)?.0),
                };
            }
        }
        let mut argument_registers = Vec::new();
        let (mut general, mut float) = (FIRST_GENERAL_ARGUMENT, 1);
        for (index, value) in values.into_iter().enumerate() {
            if let Some(constant) = long_constant(&arguments[index]).filter(|_| is_wide(arguments[index].ty) && wide_values[index].is_none()) {
                general += 1 - general % 2;
                self.load_constant(general, constant >> 32)?;
                self.load_constant(general + 1, i64::from(constant as i32))?;
                argument_registers.push(Register::general(general));
                argument_registers.push(Register::general(general + 1));
                general += 2;
                continue;
            }
            if let Some((high, low)) = wide_values[index] {
                general += 1 - general % 2;
                for (register, value) in [(general, high), (general + 1, low)] {
                    if value != register {
                        self.emit_plain(Instruction::Or { a: register, s: value, b: value });
                    }
                    argument_registers.push(Register::general(register));
                }
                general += 2;
                continue;
            }
            if is_float(arguments[index].ty) {
                let value = value.expect("floating arguments are evaluated");
                if value != float {
                    self.emit_plain(Instruction::FloatMove { d: float, b: value });
                }
                argument_registers.push(Register::float(float));
                float += 1;
                continue;
            }
            let register = general;
            general += 1;
            // Past r10: the outgoing argument area at r1+8.
            if register > LAST_GENERAL_ARGUMENT {
                let value = value.expect("stack arguments are evaluated");
                let offset = 8 + 4 * (register - LAST_GENERAL_ARGUMENT - 1) as i16;
                let mut store = PInstr::new(Instruction::StoreWord { s: value, a: 1, offset });
                store.not_r0.push(1);
                self.emit(store);
                continue;
            }
            match value {
                Some(value) if value == register => {}
                Some(value) => self.emit_plain(Instruction::Or { a: register, s: value, b: value }),
                None => match arguments[index].as_int() {
                    Some(constant) => self.load_constant(register, constant)?,
                    None => {
                        let (value, _) = self.expression_with_target(&arguments[index], Some(register))?;
                        if value != register {
                            self.emit_plain(Instruction::Or { a: register, s: value, b: value });
                        }
                    }
                },
            }
            argument_registers.push(Register::general(register));
        }
        let mut call = match callee {
            Some(callee) => {
                // The target goes through r12 into CTR.
                let (address, _) = self.expression_with_target(callee, Some(12))?;
                if address != 12 {
                    self.emit_plain(Instruction::Or { a: 12, s: address, b: address });
                }
                self.emit_plain(Instruction::MoveToCountRegister { s: 12 });
                argument_registers.push(Register::general(12));
                PInstr::new(Instruction::BranchToCountRegisterAndLink)
            }
            None => {
                if self.unit.variadic_callees.contains(name) {
                    // CR1[eq] tells a variadic callee whether floating
                    // arguments are in registers.
                    let floating = arguments.iter().any(|argument| is_float(argument.ty));
                    self.emit_plain(if floating {
                        Instruction::ConditionRegisterSet { d: 6 }
                    } else {
                        Instruction::ConditionRegisterClear { d: 6 }
                    });
                }
                let mut call = PInstr::new(Instruction::BranchAndLink { target: name.to_owned() });
                call.relocation = Some(AttachedRelocation {
                    kind: RelocationKind::Rel24,
                    target: RelocationTarget::External(name.to_owned()),
                });
                call
            }
        };
        call.implicit_uses = argument_registers;
        call.implicit_defs = VOLATILE_GENERAL.iter().map(|&r| Register::general(r)).collect();
        call.implicit_defs.extend((0..14).map(Register::float));
        self.emit(call);
        self.makes_calls = true;
        self.forget_loaded_globals(true);
        // Constants are rematerialized rather than kept across a call
        // (but for a constant address's base on some builds).
        if self.unit.address_bases_across_calls && !toggle("MWCC_PCODE_NO_BASES_ACROSS_CALLS") {
            let bases = &self.address_bases;
            self.constants.retain(|value, _| bases.contains(value));
        } else {
            self.constants.clear();
        }
        self.float_constants.clear();
        // (A section anchor stays live across the call.)
        let keep_anchors = !toggle("MWCC_PCODE_ANCHOR_AFTER_CALL");
        let escaping = self.escaping.clone();
        self.common.retain(|key, _| {
            (keep_anchors && key.starts_with("&@..."))
                || (!key.starts_with('&') && !key.contains('@') && !key.starts_with('*'))
                // (A frame object no pointer reaches survives the call.)
                || private_frame_key(key, &escaping)
        });
        let result = self.result_for(ty, target);
        // (A result wanted in the result register stays there.)
        if result != result_register(ty) {
            self.copy(ty, result, result_register(ty));
        }
        Ok(result)
    }
}

use mwcc_syntax_trees_to_iro::passes::fits_unconverted as mwcc_syntax_trees_to_iro_fits;

/// A block copy's load of one unit (`width` bytes), optionally updating
/// the base.
fn copy_load(width: u32, d: u32, a: u32, offset: i16, update: bool) -> Instruction {
    match (width, update) {
        (4, false) => Instruction::LoadWord { d, a, offset },
        (4, true) => Instruction::LoadWordWithUpdate { d, a, offset },
        (2, false) => Instruction::LoadHalfwordZero { d, a, offset },
        (2, true) => Instruction::LoadHalfZeroWithUpdate { d, a, offset },
        (_, false) => Instruction::LoadByteZero { d, a, offset },
        (_, true) => Instruction::LoadByteZeroWithUpdate { d, a, offset },
    }
}

/// A block copy's store of one unit.
fn copy_store(width: u32, s: u32, a: u32, offset: i16, update: bool) -> Instruction {
    match (width, update) {
        (4, false) => Instruction::StoreWord { s, a, offset },
        (4, true) => Instruction::StoreWordWithUpdate { s, a, offset },
        (2, false) => Instruction::StoreHalfword { s, a, offset },
        (2, true) => Instruction::StoreHalfwordWithUpdate { s, a, offset },
        (_, false) => Instruction::StoreByte { s, a, offset },
        (_, true) => Instruction::StoreByteWithUpdate { s, a, offset },
    }
}

/// `base + delta` as a displacement.
fn at_offset(base: i16, delta: u32) -> Compilation<i16> {
    i16::try_from(i32::from(base) + delta as i32).map_err(|_| unsupported("a large struct copy offset"))
}

/// The widest unit (word, half, byte) a remaining tail allows.
fn tail_width(remaining: u32) -> u32 {
    if remaining >= 4 {
        4
    } else if remaining >= 2 {
        2
    } else {
        1
    }
}

/// A memory place's identity (base, index, offset).
fn place_key(base: &Expr, index: Option<&Expr>, offset: i32) -> String {
    format!("{base:?}|{index:?}|{offset}")
}

/// The key of a store place its value loads from (`a[i] = a[i] op v`):
/// the left operand chain, through conversions and bit-field inserts.
fn compound_place_key(place: &Place, value: &Expr) -> Option<String> {
    let Place::Memory { base, index, offset } = place else { return None };
    let key = place_key(base, index.as_deref(), *offset);
    let mut current = value;
    loop {
        match &current.kind {
            ExprKind::Load { base, index, offset } => {
                return (place_key(base, index.as_deref(), *offset) == key).then_some(key);
            }
            ExprKind::Convert(inner) => current = inner,
            ExprKind::Binary(_, left, _) => current = left,
            ExprKind::Idiom(Idiom::Insert { base, .. }) => current = base,
            _ => return None,
        }
    }
}

/// Values known in registers at a program point.
#[derive(Clone)]
struct Known {
    loaded_globals: HashMap<String, (u32, Type)>,
    extended: HashMap<VarId, u32>,
    constants: HashMap<i64, u32>,
    common: HashMap<String, (u32, Type, Vec<VarId>)>,
    float_constants: HashMap<(u64, u8), u32>,
}

/// Whether any statement assigns `variable`.
/// Whether statements hold a source label (a `goto` target).
fn holds_label(body: &[Stmt]) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Label(_) => true,
        Stmt::If { then_body, else_body, .. } => holds_label(then_body) || holds_label(else_body),
        Stmt::Loop { body, step, effects, .. } => holds_label(body) || holds_label(step) || holds_label(effects),
        Stmt::Counted { body, .. } => holds_label(body),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| holds_label(arm)),
        _ => false,
    })
}

/// How many statements assign `variable`.
fn assignments(body: &[Stmt], variable: VarId) -> usize {
    body.iter()
        .map(|statement| match statement {
            Stmt::Assign { variable: assigned, .. } => usize::from(*assigned == variable),
            Stmt::If { then_body, else_body, .. } => assignments(then_body, variable) + assignments(else_body, variable),
            Stmt::Loop { body, step, effects, .. } => assignments(body, variable) + assignments(step, variable) + assignments(effects, variable),
            Stmt::Counted { body, .. } => assignments(body, variable),
            Stmt::Switch { arms, .. } => arms.iter().map(|arm| assignments(arm, variable)).sum(),
            _ => 0,
        })
        .sum()
}

fn assigns(body: &[Stmt], variable: VarId) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Assign { variable: assigned, .. } => *assigned == variable,
        Stmt::If { then_body, else_body, .. } => assigns(then_body, variable) || assigns(else_body, variable),
        Stmt::Loop { body, step, effects, .. } => assigns(body, variable) || assigns(step, variable) || assigns(effects, variable),
        Stmt::Counted { body, .. } => assigns(body, variable),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| assigns(arm, variable)),
        _ => false,
    })
}

/// Sign or zero extension of a narrow value (a copy for a word).
fn extension(ty: Type, d: u32, s: u32) -> Instruction {
    match ty {
        Type::Char => Instruction::ExtendSignByte { a: d, s },
        Type::Short => Instruction::ExtendSignHalfword { a: d, s },
        Type::UnsignedChar => Instruction::ClearLeftImmediate { a: d, s, clear: 24 },
        Type::UnsignedShort => Instruction::ClearLeftImmediate { a: d, s, clear: 16 },
        _ => Instruction::Or { a: d, s, b: s },
    }
}

/// For a comparison: the cr0 bit it tests and whether the relation holds
/// when the bit is set (`<`: LT set; `>=`: LT clear).
fn comparison(op: BinaryOp) -> (u8, bool) {
    match op {
        BinaryOp::Less => (0, true),
        BinaryOp::GreaterEqual => (0, false),
        BinaryOp::Greater => (1, true),
        BinaryOp::LessEqual => (1, false),
        BinaryOp::Equal => (2, true),
        BinaryOp::NotEqual => (2, false),
        _ => unreachable!("comparison operators only"),
    }
}

/// `x | k` / `x ^ k` immediates: a low halfword (`ori`) or a high halfword
/// with a zero low half (`oris`).
fn unsigned_immediate(expression: &Expr) -> Option<(u16, bool)> {
    let value = expression.as_int()?;
    let value = u32::try_from(value).ok().or_else(|| i32::try_from(value).ok().map(|v| v as u32))?;
    if value <= 0xFFFF {
        Some((value as u16, false))
    } else if value & 0xFFFF == 0 {
        Some(((value >> 16) as u16, true))
    } else {
        None
    }
}

/// A contiguous mask `x & k` as `rlwinm` bounds (MB..ME, bit 0 = MSB).
/// `(mb, me)` with mb > me for a mask whose clear bits are one contiguous
/// run strictly inside the word.
fn wrapped_mask_bounds(expression: &Expr) -> Option<(u8, u8)> {
    let value = expression.as_int()?;
    let mask = u32::try_from(value).ok().or_else(|| i32::try_from(value).ok().map(|v| v as u32))?;
    if mask & 1 == 0 || mask & 0x8000_0000 == 0 {
        return None;
    }
    let (hole_begin, hole_end) = mask_bounds(&Expr::int(i64::from(!mask)))?;
    Some((hole_end + 1, hole_begin - 1))
}

fn mask_bounds(expression: &Expr) -> Option<(u8, u8)> {
    let value = expression.as_int()?;
    let mask = u32::try_from(value).ok().or_else(|| i32::try_from(value).ok().map(|v| v as u32))?;
    if mask == 0 || mask == u32::MAX {
        return None;
    }
    let leading = mask.leading_zeros();
    let trailing = mask.trailing_zeros();
    ((mask >> trailing).count_ones() == 32 - leading - trailing).then_some((leading as u8, (31 - trailing) as u8))
}

/// `(x >> s) & m` / `(x << s) & m` as one `rlwinm x, rotate, mb, me` when the
/// mask keeps only bits the shift defines.
fn shift_mask<'a>(left: &'a Expr, right: &Expr) -> Option<(&'a Expr, u8, u8, u8)> {
    let (begin, end) = mask_bounds(right)?;
    let mask = right.as_int()? as u32;
    let ExprKind::Binary(op, operand, amount) = &left.kind else { return None };
    let amount = u32::try_from(amount.as_int()?).ok().filter(|amount| (1..32).contains(amount))?;
    match op {
        BinaryOp::ShiftRight if mask <= (u32::MAX >> amount) => Some((operand, (32 - amount) as u8, begin, end)),
        BinaryOp::ShiftLeft if mask & ((1 << amount) - 1) == 0 => Some((operand, amount as u8, begin, end)),
        _ => None,
    }
}

/// `x` rotated and masked, as `rlwimi`'s inserted field: `x << n`, unsigned
/// `x >> n`, `x & mask`, `(x >> n) & mask`.
fn insert_field(expression: &Expr, refined: bool) -> Option<(&Expr, u8, u8, u8)> {
    // (A narrow unsigned value, unshifted.)
    if refined {
        if let ExprKind::Convert(operand) = &expression.kind {
            if matches!(operand.ty, Type::UnsignedChar | Type::UnsignedShort) {
                let width = 8 * width(operand.ty) as u8;
                return Some((expression, 0, 32 - width, 31));
            }
        }
    }
    let ExprKind::Binary(op, left, right) = &expression.kind else { return None };
    match (op, right.as_int()) {
        // (Only the bits the shifted value can hold.)
        (BinaryOp::ShiftLeft, Some(n)) if refined && (1..32).contains(&n) && possible_bits(left) != u32::MAX => {
            let bits = possible_bits(left).checked_shl(n as u32).unwrap_or(0);
            if bits == 0 || !(bits >> bits.trailing_zeros()).wrapping_add(1).is_power_of_two() {
                return Some((left, n as u8, 0, 31 - n as u8));
            }
            Some((left, n as u8, bits.leading_zeros() as u8, (31 - bits.trailing_zeros()) as u8))
        }
        (BinaryOp::ShiftLeft, Some(n)) if (1..32).contains(&n) => Some((left, n as u8, 0, 31 - n as u8)),
        (BinaryOp::ShiftRight, Some(n)) if (1..32).contains(&n) && is_unsigned(promote(left.ty)) => {
            Some((left, (32 - n) as u8, n as u8, 31))
        }
        (BinaryOp::BitAnd, _) => {
            if let Some(field) = shift_mask(left, right) {
                return Some(field);
            }
            let (begin, end) = mask_bounds(right)?;
            Some((left, 0, begin, end))
        }
        _ => None,
    }
}

/// The bits an integer value can have set.
fn possible_bits(expression: &Expr) -> u32 {
    match &expression.kind {
        ExprKind::Convert(operand) => match operand.ty {
            Type::UnsignedChar => 0xff,
            Type::UnsignedShort => 0xffff,
            _ => u32::MAX,
        },
        ExprKind::Int(value) => *value as u32,
        ExprKind::Binary(op, left, right) => match (op, right.as_int()) {
            (BinaryOp::ShiftLeft, Some(n)) if (0..32).contains(&n) => possible_bits(left) << n,
            (BinaryOp::ShiftRight, Some(n)) if (0..32).contains(&n) && is_unsigned(promote(left.ty)) => possible_bits(left) >> n,
            (BinaryOp::BitAnd, _) => possible_bits(left) & possible_bits(right),
            (BinaryOp::BitOr, _) => possible_bits(left) | possible_bits(right),
            _ => u32::MAX,
        },
        _ => u32::MAX,
    }
}

/// Bits of `expression`'s value known to be zero.
fn known_zero(expression: &Expr, refined: bool) -> u32 {
    if refined {
        return !possible_bits(expression) | known_zero(expression, false);
    }
    if let ExprKind::Convert(operand) = &expression.kind {
        return match operand.ty {
            Type::UnsignedChar if !toggle("MWCC_PCODE_NO_NARROW_KNOWN_ZERO") => 0xffff_ff00,
            Type::UnsignedShort if !toggle("MWCC_PCODE_NO_NARROW_KNOWN_ZERO") => 0xffff_0000,
            _ => 0,
        };
    }
    let ExprKind::Binary(op, left, right) = &expression.kind else { return 0 };
    match (op, right.as_int()) {
        (BinaryOp::ShiftLeft, Some(n)) if (1..32).contains(&n) => (1u32 << n) - 1,
        (BinaryOp::ShiftRight, Some(n)) if (1..32).contains(&n) && is_unsigned(promote(left.ty)) => !(u32::MAX >> n),
        (BinaryOp::BitAnd, Some(mask)) => !(mask as u32),
        _ => 0,
    }
}

/// The bits `begin..=end` (IBM numbering, non-wrapping).
fn field_bits(begin: u8, end: u8) -> u32 {
    (u32::MAX >> begin) & (u32::MAX << (31 - end))
}

/// A constant that does not fit `addi`: `(high, low)` for `addis`+`addi`.
fn wide_constant(expression: &Expr) -> Option<(i16, i16)> {
    let value = expression.as_int()?;
    let value = i32::try_from(value).ok().or_else(|| u32::try_from(value).ok().map(|v| v as i32))?;
    if i16::try_from(value).is_ok() {
        return None;
    }
    let low = value as i16;
    let high = ((value - i32::from(low)) >> 16) as i16;
    Some((high, low))
}

/// Whether the body calls anything.
/// Whether every call in `body` is a tail call: a call statement that ends
/// the function (`f(); return;`, `return f(x);`) with call-free arguments,
/// and no call anywhere else.
fn only_tail_calls(body: &[Stmt], at_end: bool, return_type: Type) -> bool {
    let terminal_call = |call: &Expr| match &call.kind {
        ExprKind::Call { arguments, .. } => {
            !arguments.iter().any(|argument| contains_call(argument) || is_float(argument.ty))
                && !is_float(call.ty)
                && !is_narrow(return_type)
        }
        _ => false,
    };
    body.iter().enumerate().all(|(index, statement)| {
        let ends = index + 1 == body.len() && at_end || matches!(body.get(index + 1), Some(Stmt::Return(None)));
        match statement {
            // (A function that falls off its end returns nothing either.)
            Stmt::Eval(call) if matches!(call.kind, ExprKind::Call { .. }) => {
                ends && (return_type == Type::Void || !toggle("MWCC_PCODE_NO_FALLOFF_TAIL_CALLS")) && terminal_call(call)
            }
            Stmt::SetReturn(call) if matches!(call.kind, ExprKind::Call { .. }) => {
                ends && call.ty == return_type && terminal_call(call)
            }
            Stmt::Return(Some(call)) if matches!(call.kind, ExprKind::Call { .. }) => {
                call.ty == return_type && terminal_call(call)
            }
            Stmt::If { condition, then_body, else_body } => {
                !contains_call(condition)
                    && only_tail_calls(then_body, ends, return_type)
                    && only_tail_calls(else_body, ends, return_type)
            }
            other => !makes_calls(std::slice::from_ref(other)),
        }
    })
}

fn makes_calls(body: &[Stmt]) -> bool {
    fn expression(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Call { .. } => true,
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Var(_)
            | ExprKind::Global(_)
            | ExprKind::GlobalAddress(_)
            | ExprKind::LocalAddress(_)
            | ExprKind::StringAddress(_) | ExprKind::Image(_) => false,
            ExprKind::Load { base, index, .. } => expression(base) || index.as_deref().is_some_and(expression),
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand),
            ExprKind::Binary(_, left, right) => expression(left) || expression(right),
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition) || expression(when_true) || expression(when_false)
            }
            ExprKind::Idiom(Idiom::Absolute(value) | Idiom::Unary(_, value)) => expression(value),
            ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => expression(tested) || expression(value),
            ExprKind::Idiom(Idiom::Insert { base, value, .. } | Idiom::Update(_, base, value)) => expression(base) || expression(value),
        }
    }
    body.iter().any(|statement| match statement {
        Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => expression(value),
        Stmt::Return(value) => value.as_ref().is_some_and(expression),
        Stmt::Store { place, value, .. } => {
            expression(value)
                || matches!(place, Place::Memory { base, index, .. }
                    if expression(base) || index.as_deref().is_some_and(expression))
        }
        Stmt::If { condition, then_body, else_body } => {
            expression(condition) || makes_calls(then_body) || makes_calls(else_body)
        }
        Stmt::Loop { condition, body, step, effects, .. } => {
            condition.as_ref().is_some_and(expression) || makes_calls(body) || makes_calls(step) || makes_calls(effects)
        }
        Stmt::Counted { count, guard, body } => expression(count) || guard.as_ref().is_some_and(expression) || makes_calls(body),
        Stmt::Switch { value, arms, .. } => expression(value) || arms.iter().any(|arm| makes_calls(arm)),
        Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => false,
    })
}

/// A top-level `v = value` (no call) read only by the next statement, which
/// makes no call: MWCC propagates the value into that use.
fn propagated_into_next(body: &[Stmt], variable: VarId) -> bool {
    let Some(at) = body.iter().position(|statement| matches!(statement, Stmt::Assign { variable: assigned, .. } if *assigned == variable)) else {
        return false;
    };
    let Stmt::Assign { value, .. } = &body[at] else { return false };
    let Some(next) = body.get(at + 1) else { return false };
    !format!("{value:?}").contains("Call {")
        && !format!("{next:?}").contains("Call {")
        && references(std::slice::from_ref(next), variable) == 1
        && matches!(next, Stmt::Assign { .. } | Stmt::Store { .. } | Stmt::SetReturn(_) | Stmt::Return(_) | Stmt::Eval(_))
}

/// How many times the body reads or assigns `variable`.
fn references(body: &[Stmt], variable: VarId) -> usize {
    references_weighted(body, variable, 1)
}

/// References of `variable`, one used directly as an address counting `base`.
fn references_weighted(body: &[Stmt], variable: VarId, base: usize) -> usize {
    // (An address `p + k` is the pointer used as a base too.)
    fn based_on(e: &Expr, variable: VarId) -> bool {
        match &e.kind {
            ExprKind::Var(id) => *id == variable,
            ExprKind::Binary(BinaryOp::Add, left, right) if right.as_int().is_some() && !toggle("MWCC_PCODE_O0_BARE_BASES") => {
                based_on(left, variable)
            }
            _ => false,
        }
    }
    let as_base = |e: &Expr| if based_on(e, variable) { base } else { 0 };
    let expression = |e: &Expr, variable: VarId| count(e, variable, base);
    fn count(e: &Expr, variable: VarId, base: usize) -> usize {
        fn based_on(e: &Expr, variable: VarId) -> bool {
            match &e.kind {
                ExprKind::Var(id) => *id == variable,
                ExprKind::Binary(BinaryOp::Add, left, right) if right.as_int().is_some() && !toggle("MWCC_PCODE_O0_BARE_BASES") => {
                    based_on(left, variable)
                }
                _ => false,
            }
        }
        let as_base = |e: &Expr| if based_on(e, variable) { base } else { 0 };
        let expression = |e: &Expr, variable: VarId| count(e, variable, base);
        match &e.kind {
            ExprKind::Var(id) => usize::from(*id == variable),
            ExprKind::LocalAddress(id) => usize::from(*id == variable),
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Global(_) | ExprKind::GlobalAddress(_) | ExprKind::StringAddress(_) | ExprKind::Image(_) => 0,
            ExprKind::Load { base: address, index, .. } if as_base(address) > 0 => {
                as_base(address) + index.as_deref().map_or(0, |index| expression(index, variable))
            }
            ExprKind::Load { base, index, .. } => {
                expression(base, variable) + index.as_deref().map_or(0, |index| expression(index, variable))
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand, variable),
            ExprKind::Binary(_, left, right) => expression(left, variable) + expression(right, variable),
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition, variable) + expression(when_true, variable) + expression(when_false, variable)
            }
            ExprKind::Call { arguments, .. } => arguments.iter().map(|a| expression(a, variable)).sum(),
            ExprKind::Idiom(Idiom::Absolute(value) | Idiom::Unary(_, value)) => expression(value, variable),
            ExprKind::Idiom(Idiom::Insert { base, value, .. } | Idiom::Update(_, base, value)) => expression(base, variable) + expression(value, variable),
            ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => {
                expression(tested, variable) + expression(value, variable)
            }
        }
    }
    body.iter()
        .map(|statement| match statement {
            Stmt::Assign { variable: assigned, value } => {
                usize::from(*assigned == variable) + expression(value, variable)
            }
            Stmt::Eval(value) | Stmt::SetReturn(value) => expression(value, variable),
            Stmt::Return(value) => value.as_ref().map_or(0, |value| expression(value, variable)),
            Stmt::Store { place, value, .. } => {
                // (A compound update's address is computed once: its load
                // already counted the base.)
                let compound = compound_place_key(place, value).is_some() && !toggle("MWCC_PCODE_O0_COMPOUND_BASE_TWICE");
                expression(value, variable)
                    + match place {
                        Place::Memory { base: address, index, .. } if as_base(address) > 0 => {
                            (if compound { 0 } else { as_base(address) }) + index.as_deref().map_or(0, |index| expression(index, variable))
                        }
                        Place::Memory { base, index, .. } => {
                            expression(base, variable)
                                + index.as_deref().map_or(0, |index| expression(index, variable))
                        }
                        Place::Global(_) => 0,
                    }
            }
            Stmt::If { condition, then_body, else_body } => {
                expression(condition, variable) + references_weighted(then_body, variable, base) + references_weighted(else_body, variable, base)
            }
            Stmt::Loop { condition, body, step, effects, .. } => {
                condition.as_ref().map_or(0, |c| expression(c, variable))
                    + references_weighted(body, variable, base)
                    + references_weighted(step, variable, base) + references_weighted(effects, variable, base)
            }
            Stmt::Counted { count, guard, body } => {
                expression(count, variable)
                    + guard.as_ref().map_or(0, |g| expression(g, variable))
                    + references_weighted(body, variable, base)
            }
            Stmt::Switch { value, arms, .. } => {
                expression(value, variable) + arms.iter().map(|arm| references_weighted(arm, variable, base)).sum::<usize>()
            }
            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => 0,
        })
        .sum()
}

/// Whether `condition` holds on entry given the preceding constant assignment.
fn initially_true(condition: &Expr, known: &[(VarId, i64)]) -> bool {
    let ExprKind::Binary(op, left, right) = &condition.kind else { return false };
    let (Some(id), Some(bound)) = (unpromoted(left).as_var(), right.as_int()) else { return false };
    let Some(&(_, value)) = known.iter().find(|&&(variable, _)| variable == id) else { return false };
    match op {
        BinaryOp::Less => value < bound,
        BinaryOp::LessEqual => value <= bound,
        BinaryOp::Greater => value > bound,
        BinaryOp::GreaterEqual => value >= bound,
        BinaryOp::NotEqual => value != bound,
        BinaryOp::Equal => value == bound,
        _ => false,
    }
}

/// A counted loop MWCC's unroller rewrites: the condition compares (or
/// tests) a variable the loop steps by a constant, against a bound the loop
/// does not change.
fn counted(condition: Option<&Expr>, body: &[Stmt], step: &[Stmt]) -> bool {
    let Some(condition) = condition else { return false };
    let stepped = |variable: VarId| {
        body.iter().chain(step).any(|statement| {
            matches!(statement, Stmt::Assign { variable: v, value }
                if *v == variable && matches!(&value.kind, ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, right)
                    if unpromoted(left).as_var() == Some(variable) && right.as_int().is_some()))
        })
    };
    let invariant = |e: &Expr| {
        e.as_int().is_some()
            || unpromoted(e).as_var().is_some_and(|id| !assigns(body, id) && !assigns(step, id))
    };
    match &condition.kind {
        ExprKind::Binary(op, left, right) if op.is_comparison() => {
            unpromoted(left).as_var().is_some_and(stepped) && invariant(right)
                || unpromoted(right).as_var().is_some_and(stepped) && invariant(left)
        }
        _ => unpromoted(condition).as_var().is_some_and(stepped),
    }
}

/// A displacement store of `ty`'s width.
fn store_instruction(ty: Type, s: u32, a: u32, offset: i16) -> Instruction {
    match ty {
        Type::Float => Instruction::StoreFloatSingle { s, a, offset },
        Type::Double => Instruction::StoreFloatDouble { s, a, offset },
        Type::Char | Type::UnsignedChar => Instruction::StoreByte { s, a, offset },
        Type::Short | Type::UnsignedShort => Instruction::StoreHalfword { s, a, offset },
        _ => Instruction::StoreWord { s, a, offset },
    }
}

/// The operand of an integer promotion (or the expression itself).
/// `(x << a, x >> b)` with one amount `32 -` the other, x an unsigned
/// variable: the rotated value and the left rotation amount `a`.
fn variable_rotate<'e>(shifted_left: &'e Expr, shifted_right: &'e Expr) -> Option<(&'e Expr, &'e Expr)> {
    let ExprKind::Binary(BinaryOp::ShiftLeft, x, a) = &shifted_left.kind else { return None };
    let ExprKind::Binary(BinaryOp::ShiftRight, y, b) = &shifted_right.kind else { return None };
    let variable = |e: &Expr| match &peel_conversions(e).kind {
        ExprKind::Var(id) => Some(*id),
        _ => None,
    };
    let complement = |whole: &Expr, part: &Expr| {
        matches!(&peel_conversions(whole).kind, ExprKind::Binary(BinaryOp::Subtract, k, n)
            if k.as_int() == Some(32) && variable(n).is_some() && variable(n) == variable(part))
    };
    if x.ty != Type::UnsignedInt || y.ty != Type::UnsignedInt || variable(x).is_none() || variable(x) != variable(y) {
        return None;
    }
    (complement(b, a) || complement(a, b)).then_some((x.as_ref(), a.as_ref()))
}

/// Objects a function addresses through their section anchor: those of a
/// section from which it refers to three or more distinct objects.
fn anchored_objects(function: &Function, unit: &Unit<'_>) -> HashMap<String, &'static str> {
    let mut anchored = HashMap::new();
    if unit.unoptimized {
        return anchored;
    }
    let listing = format!("{:?}", function.body);
    let mut names: Vec<String> = Vec::new();
    for marker in ["Global(\"", "GlobalAddress(\""] {
        let mut rest = listing.as_str();
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            let Some(end) = rest.find('"') else { break };
            let name = rest[..end].to_owned();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    // String literals in `.data` count too (as their `@@strN` placeholders).
    let data_string = |index: usize| {
        unit.data_anchor
            && !unit.strings_packed
            && !(unit.strings_small_data && function.strings[index].len() + 1 <= 8)
            && !toggle("MWCC_PCODE_NO_STRING_ANCHOR")
    };
    let mut strings: Vec<String> = Vec::new();
    let mut rest = listing.as_str();
    while let Some(at) = rest.find("StringAddress(") {
        rest = &rest[at + "StringAddress(".len()..];
        let Some(index) = rest.split(')').next().and_then(|digits| digits.parse::<usize>().ok()) else { continue };
        let name = format!("@@str{index}");
        if data_string(index) && !strings.contains(&name) {
            strings.push(name);
        }
    }
    for section in ["...bss.0", "...data.0"] {
        let members: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|name| unit.globals.get(*name).is_some_and(|global| global.anchor == Some(section)))
            .chain(strings.iter().map(String::as_str).filter(|_| section == "...data.0"))
            .collect();
        if members.len() >= 3 {
            for name in members {
                anchored.insert(name.to_owned(), section);
            }
        }
    }
    anchored
}

/// A narrow variable's promotion: the variable.
fn narrow_variable(expression: &Expr) -> Option<&Expr> {
    match &expression.kind {
        ExprKind::Convert(inner) if is_narrow(inner.ty) && matches!(inner.kind, ExprKind::Var(_)) => Some(inner),
        _ => None,
    }
}

/// The value bits of a narrow type.
fn narrow_mask(ty: Type) -> u32 {
    match ty {
        Type::Char | Type::UnsignedChar => 0xff,
        Type::Short | Type::UnsignedShort => 0xffff,
        _ => u32::MAX,
    }
}

/// Whether an expression contains a call.
/// A call with a wide argument or result.
fn is_wide_call(call: &Expr) -> bool {
    is_wide(call.ty) || matches!(&call.kind, ExprKind::Call { arguments, .. } if arguments.iter().any(|argument| is_wide(argument.ty)))
}

fn contains_call(expression: &Expr) -> bool {
    format!("{:?}", expression.kind).contains("Call {")
}

fn peel_conversions(expression: &Expr) -> &Expr {
    match &expression.kind {
        ExprKind::Convert(operand) => peel_conversions(operand),
        _ => expression,
    }
}

fn unpromoted(expression: &Expr) -> &Expr {
    match &expression.kind {
        ExprKind::Convert(operand) if is_narrow(operand.ty) && !is_narrow(expression.ty) => operand,
        _ => expression,
    }
}

/// `(high, low)` so that `(high << 16) + low` is `address` (low signed).
fn split_address(address: i64) -> Compilation<(i16, i16)> {
    let address = address as i32;
    let low = address as i16;
    let high = ((address - i32::from(low)) >> 16) as i16;
    Ok((high, low))
}

/// An upper bound on an unsigned value, where masks make it evident.
fn max_value(expression: &Expr) -> Option<u64> {
    match &expression.kind {
        ExprKind::Int(value) => u64::try_from(*value).ok(),
        ExprKind::Binary(BinaryOp::BitAnd, left, right) => {
            let bound = |e: &Expr| e.as_int().and_then(|v| u32::try_from(v).ok()).map(u64::from);
            match (bound(right), bound(left)) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) | (None, Some(a)) => Some(a),
                _ => None,
            }
        }
        ExprKind::Binary(BinaryOp::ShiftRight, left, right) if !matches!(left.ty, Type::Int) || max_value(left).is_some() => {
            Some(max_value(left).or_else(|| unsigned_bound(left))? >> right.as_int()?)
        }
        ExprKind::Binary(op, ..) if op.is_comparison() => Some(1),
        // (GC/1.0-1.2.5n: a small sum of bounded values.)
        ExprKind::Binary(BinaryOp::Add, left, right) if SUM_BOUNDS.with(|flag| flag.get()) && !toggle("MWCC_PCODE_NO_SUM_BOUNDS") => {
            let sum = max_value(left)?.checked_add(max_value(right)?)?;
            (sum <= u64::from(u32::MAX)).then_some(sum)
        }
        ExprKind::Convert(_) | ExprKind::Load { .. } if SUM_BOUNDS.with(|flag| flag.get()) && !toggle("MWCC_PCODE_NO_SUM_BOUNDS") => {
            let narrow = match &expression.kind {
                ExprKind::Convert(operand) => operand.ty,
                _ => expression.ty,
            };
            match narrow {
                Type::UnsignedChar => Some(0xff),
                Type::UnsignedShort => Some(0xffff),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The bound of an unsigned word of unknown value.
fn unsigned_bound(expression: &Expr) -> Option<u64> {
    (matches!(expression.ty, Type::UnsignedInt) && !toggle("MWCC_PCODE_NO_SUM_BOUNDS")).then_some(u64::from(u32::MAX))
}

/// A key for a simple pure operation on variables and constants (IRO common
/// subexpressions), with the variables it reads.
fn common_key(expression: &Expr) -> Option<(String, Vec<VarId>)> {
    fn leaf(e: &Expr, variables: &mut Vec<VarId>) -> Option<String> {
        match &e.kind {
            ExprKind::Int(value) => Some(format!("{value}")),
            ExprKind::Var(id) => {
                variables.push(*id);
                Some(format!("v{id}"))
            }
            ExprKind::Convert(operand) => {
                let inner = leaf(operand, variables)?;
                Some(format!("({:?}){inner}", e.ty))
            }
            ExprKind::GlobalAddress(name) if !toggle("MWCC_PCODE_NO_GLOBAL_ADDRESS_CSE") => Some(format!("@{name}")),
            // (A nested arithmetic operand, itself reused.)
            ExprKind::Binary(op, left, right)
                if !matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::Divide | BinaryOp::Modulo)
                    && !op.is_comparison()
                    && !toggle("MWCC_PCODE_NO_NESTED_CSE") =>
            {
                Some(format!("({:?} {op:?} {} {})", e.ty, leaf(left, variables)?, leaf(right, variables)?))
            }
            _ => None,
        }
    }
    let ExprKind::Binary(op, left, right) = &expression.kind else { return None };
    if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) || op.is_comparison() {
        return None;
    }
    let mut variables = Vec::new();
    let key = format!("{:?} {op:?} {} {}", expression.ty, leaf(left, &mut variables)?, leaf(right, &mut variables)?);
    Some((key, variables))
}

/// The register a function result of `ty` returns in.
fn result_register(ty: Type) -> u32 {
    if is_float(ty) {
        1
    } else {
        3
    }
}

fn toggle(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// Signed division magic (Hacker's Delight 10-1): multiplier and shift for
/// a divisor >= 2 that is not a power of two.
fn signed_magic(divisor: i32) -> (i32, u32) {
    const TWO31: u32 = 0x8000_0000;
    let ad = divisor.unsigned_abs();
    let t = TWO31.wrapping_add((divisor as u32) >> 31);
    let anc = t - 1 - t % ad;
    let mut p = 31u32;
    let mut q1 = TWO31 / anc;
    let mut r1 = TWO31 - q1 * anc;
    let mut q2 = TWO31 / ad;
    let mut r2 = TWO31 - q2 * ad;
    loop {
        p += 1;
        q1 = q1.wrapping_mul(2);
        r1 = r1.wrapping_mul(2);
        if r1 >= anc {
            q1 = q1.wrapping_add(1);
            r1 = r1.wrapping_sub(anc);
        }
        q2 = q2.wrapping_mul(2);
        r2 = r2.wrapping_mul(2);
        if r2 >= ad {
            q2 = q2.wrapping_add(1);
            r2 = r2.wrapping_sub(ad);
        }
        let delta = ad - r2;
        if !(q1 < delta || (q1 == delta && r1 == 0)) {
            break;
        }
    }
    let magic = q2.wrapping_add(1) as i32;
    (if divisor < 0 { magic.wrapping_neg() } else { magic }, p - 32)
}

/// Unsigned division magic (Hacker's Delight 10-2): multiplier, whether the
/// add-back form is needed, and the shift.
fn unsigned_magic(divisor: u32) -> (u32, bool, u32) {
    let mut add = false;
    let nc = u32::MAX - divisor.wrapping_neg() % divisor;
    let mut p = 31u32;
    let mut q1 = 0x8000_0000 / nc;
    let mut r1 = 0x8000_0000 - q1 * nc;
    let mut q2 = 0x7FFF_FFFF / divisor;
    let mut r2 = 0x7FFF_FFFF - q2 * divisor;
    loop {
        p += 1;
        if r1 >= nc - r1 {
            q1 = q1.wrapping_mul(2).wrapping_add(1);
            r1 = r1.wrapping_mul(2).wrapping_sub(nc);
        } else {
            q1 = q1.wrapping_mul(2);
            r1 = r1.wrapping_mul(2);
        }
        if r2 + 1 >= divisor - r2 {
            if q2 >= 0x7FFF_FFFF {
                add = true;
            }
            q2 = q2.wrapping_mul(2).wrapping_add(1);
            r2 = r2.wrapping_mul(2).wrapping_add(1).wrapping_sub(divisor);
        } else {
            if q2 >= 0x8000_0000 {
                add = true;
            }
            q2 = q2.wrapping_mul(2);
            r2 = r2.wrapping_mul(2).wrapping_add(1);
        }
        let delta = divisor - 1 - r2;
        if !(p < 64 && (q1 < delta || (q1 == delta && r1 == 0))) {
            break;
        }
    }
    (q2.wrapping_add(1), add, p - 32)
}

#[cfg(test)]
mod magic_tests {
    #[test]
    fn magic_numbers_match_mwcc() {
        assert_eq!(super::signed_magic(10), (0x6666_6667, 2));
        assert_eq!(super::signed_magic(3), (0x5555_5556, 0));
        assert_eq!(super::signed_magic(7), (0x9249_2493u32 as i32, 2));
        assert_eq!(super::unsigned_magic(10), (0xCCCC_CCCD, false, 3));
        assert_eq!(super::unsigned_magic(7), (0x2492_4925, true, 3));
    }
}

/// MWCC's switch decision tree over case segments `(low, high, arm)`
/// (recovered by fitting mwcceppc's output; see `switch_node`).
struct SwitchTree<'a> {
    segments: &'a [(i64, i64, usize)],
}

enum SwitchPivot {
    /// `cmpwi x,v; beq arm; ...` for a one-value segment inside the range.
    Equal(i64, usize),
    /// `cmpwi x,at; bge ...` at a segment boundary.
    Split(i64),
}

impl SwitchTree<'_> {
    /// Segments clipped to `[low, high]`.
    fn within(&self, low: i64, high: i64) -> Vec<(i64, i64, usize)> {
        self.segments
            .iter()
            .filter(|&&(a, b, _)| b >= low && a <= high)
            .map(|&(a, b, arm)| (a.max(low), b.min(high), arm))
            .collect()
    }

    /// The one target of every value in `[low, high]` (`None` inside the
    /// option: the default), if there is only one.
    fn single(&self, low: i64, high: i64) -> Option<Option<usize>> {
        let inside = self.within(low, high);
        match inside.as_slice() {
            [] => Some(None),
            [(a, b, arm)] if *a == low && *b == high => Some(Some(*arm)),
            _ => None,
        }
    }

    /// Values where the target changes, within `(low, high]`.
    fn thresholds(&self, low: i64, high: i64) -> Vec<i64> {
        let mut thresholds = Vec::new();
        for (a, b, _) in self.within(low, high) {
            if a > low {
                thresholds.push(a);
            }
            if b < high {
                thresholds.push(b + 1);
            }
        }
        thresholds.sort_unstable();
        thresholds.dedup();
        thresholds
    }

    /// The pivot: a one-value segment strictly inside the range tests
    /// equality (consuming thresholds v and v+1); other segments offer a
    /// split at either boundary not already an equality's. The candidate
    /// leaving the fewest thresholds on its larger side wins, then the one
    /// with more on the left, then an equality.
    fn pivot(&self, low: i64, high: i64) -> SwitchPivot {
        let inside = self.within(low, high);
        let thresholds = self.thresholds(low, high);
        let mut candidates: Vec<(SwitchPivot, i64, i64)> = Vec::new();
        let mut equal_owned = Vec::new();
        for &(a, b, arm) in &inside {
            if a == b && a > low && b < high {
                candidates.push((SwitchPivot::Equal(a, arm), a, a + 1));
                equal_owned.extend([a, a + 1]);
            }
        }
        for &(a, b, _) in &inside {
            if !(a == b && a > low && b < high) {
                if a > low && !equal_owned.contains(&a) {
                    candidates.push((SwitchPivot::Split(a), a, a));
                }
                if b < high && !equal_owned.contains(&(b + 1)) {
                    candidates.push((SwitchPivot::Split(b + 1), b + 1, b + 1));
                }
            }
        }
        candidates
            .into_iter()
            .min_by_key(|(pivot, first, last)| {
                let left = thresholds.iter().filter(|&&t| t < *first).count();
                let right = thresholds.iter().filter(|&&t| t > *last).count();
                (left.max(right), std::cmp::Reverse(left), !matches!(pivot, SwitchPivot::Equal(..)))
            })
            .map(|(pivot, _, _)| pivot)
            .expect("a multi-target range has a candidate")
    }

    /// MWCC dispatches through a table when there are at least 8 target
    /// changes and the case span is at most 4 per change, less 6.
    fn uses_table(&self) -> bool {
        let (Some(first), Some(last)) = (self.segments.first(), self.segments.last()) else { return false };
        let changes = self.thresholds(i64::MIN, i64::MAX).len() as i64;
        let span = last.1 - first.0 + 1;
        changes >= 8 && span <= 4 * changes - 6
    }
}

/// The register an integer or floating load defines.
fn load_destination(instruction: &Instruction) -> Option<u32> {
    use Instruction::*;
    match *instruction {
        LoadWord { d, .. }
        | LoadWordIndexed { d, .. }
        | LoadHalfwordAlgebraic { d, .. }
        | LoadHalfwordAlgebraicIndexed { d, .. }
        | LoadHalfwordZero { d, .. }
        | LoadHalfwordZeroIndexed { d, .. }
        | LoadByteZero { d, .. }
        | LoadByteZeroIndexed { d, .. }
        | LoadFloatSingle { d, .. }
        | LoadFloatSingleIndexed { d, .. }
        | LoadFloatDouble { d, .. }
        | LoadFloatDoubleIndexed { d, .. } => Some(d),
        _ => None,
    }
}

/// Which variables' addresses escape: used other than as the base of a load
/// or store.
fn escaping_frame_objects(body: &[Stmt], count: usize) -> Vec<bool> {
    fn expression(e: &Expr, out: &mut [bool]) {
        match &e.kind {
            ExprKind::LocalAddress(id) => out[*id] = true,
            ExprKind::Load { base, index, .. } => {
                if !matches!(base.kind, ExprKind::LocalAddress(_)) {
                    expression(base, out);
                }
                if let Some(index) = index {
                    expression(index, out);
                }
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand, out),
            ExprKind::Binary(_, left, right) => {
                expression(left, out);
                expression(right, out);
            }
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition, out);
                expression(when_true, out);
                expression(when_false, out);
            }
            ExprKind::Call { arguments, .. } => arguments.iter().for_each(|argument| expression(argument, out)),
            ExprKind::Idiom(Idiom::Absolute(value) | Idiom::Unary(_, value)) => expression(value, out),
            ExprKind::Idiom(Idiom::Insert { base, value, .. } | Idiom::Update(_, base, value)) => {
                expression(base, out);
                expression(value, out);
            }
            ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => {
                expression(tested, out);
                expression(value, out);
            }
            _ => {}
        }
    }
    fn statements(body: &[Stmt], out: &mut [bool]) {
        for statement in body {
            match statement {
                Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => expression(value, out),
                Stmt::Return(value) => {
                    if let Some(value) = value {
                        expression(value, out);
                    }
                }
                Stmt::Store { place, value, .. } => {
                    if let Place::Memory { base, index, .. } = place {
                        if !matches!(base.kind, ExprKind::LocalAddress(_)) {
                            expression(base, out);
                        }
                        if let Some(index) = index {
                            expression(index, out);
                        }
                    }
                    expression(value, out);
                }
                Stmt::If { condition, then_body, else_body } => {
                    expression(condition, out);
                    statements(then_body, out);
                    statements(else_body, out);
                }
                Stmt::Loop { condition, body, step, effects, .. } => {
                    if let Some(condition) = condition {
                        expression(condition, out);
                    }
                    statements(body, out);
                    statements(step, out);
                    statements(effects, out);
                }
                Stmt::Counted { count, guard, body } => {
                    expression(count, out);
                    if let Some(guard) = guard {
                        expression(guard, out);
                    }
                    statements(body, out);
                }
                Stmt::Switch { value, arms, .. } => {
                    expression(value, out);
                    arms.iter().for_each(|arm| statements(arm, out));
                }
                _ => {}
            }
        }
    }
    let mut out = vec![true; count];
    out.iter_mut().for_each(|escaped| *escaped = false);
    statements(body, &mut out);
    out
}

/// The operands of an expression.
fn expression_children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Load { base, index, .. } => {
            let mut out = vec![base.as_ref()];
            if let Some(index) = index {
                out.push(index);
            }
            out
        }
        ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => vec![operand],
        ExprKind::Binary(_, left, right) => vec![left, right],
        ExprKind::Select { condition, when_true, when_false } => vec![condition, when_true, when_false],
        ExprKind::Call { arguments, .. } => arguments.iter().collect(),
        ExprKind::Idiom(Idiom::Absolute(value) | Idiom::Unary(_, value)) => vec![value],
        ExprKind::Idiom(Idiom::Insert { base, value, .. } | Idiom::Update(_, base, value)) => vec![base, value],
        ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => vec![tested, value],
        _ => Vec::new(),
    }
}

/// Every expression a statement list evaluates (not recursing into them).
fn for_each_statement_expression(body: &[Stmt], visit: &mut dyn FnMut(&Expr)) {
    for statement in body {
        match statement {
            Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => visit(value),
            Stmt::Return(Some(value)) => visit(value),
            Stmt::Store { place, value, .. } => {
                if let Place::Memory { base, index, .. } = place {
                    visit(base);
                    if let Some(index) = index {
                        visit(index);
                    }
                }
                visit(value);
            }
            Stmt::If { condition, then_body, else_body } => {
                visit(condition);
                for_each_statement_expression(then_body, visit);
                for_each_statement_expression(else_body, visit);
            }
            Stmt::Loop { condition, body, step, effects, .. } => {
                if let Some(condition) = condition {
                    visit(condition);
                }
                for_each_statement_expression(body, visit);
                for_each_statement_expression(step, visit);
                for_each_statement_expression(effects, visit);
            }
            Stmt::Counted { count, guard, body } => {
                visit(count);
                if let Some(guard) = guard {
                    visit(guard);
                }
                for_each_statement_expression(body, visit);
            }
            Stmt::Switch { value, arms, .. } => {
                visit(value);
                arms.iter().for_each(|arm| for_each_statement_expression(arm, visit));
            }
            _ => {}
        }
    }
}

/// Distinct floating literals a body uses (an integer conversion's bias
/// does not count).
fn pool_constant_count(body: &[Stmt]) -> usize {
    let mut keys: Vec<(u64, u8)> = Vec::new();
    let mut visit = |e: &Expr| {
        fn walk(e: &Expr, keys: &mut Vec<(u64, u8)>) {
            let key = match &e.kind {
                ExprKind::Float(value) if e.ty == Type::Float => Some((u64::from((*value as f32).to_bits()), 4u8)),
                ExprKind::Float(value) => Some((value.to_bits(), 8u8)),
                _ => None,
            };
            if let Some(key) = key {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            for child in expression_children(e) {
                walk(child, keys);
            }
        }
        walk(e, &mut keys);
    };
    for_each_statement_expression(body, &mut visit);
    keys.len()
}

/// `computed == v` compares `v` first: an arithmetic result or indexed
/// load goes second in an equality with a variable.
fn equality_variable_first(op: BinaryOp, left: &Expr, right: &Expr) -> bool {
    matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
        && right.as_var().is_some()
        && match &left.kind {
            ExprKind::Binary(inner, ..) => !inner.is_comparison(),
            ExprKind::Load { index: Some(_), .. } => true,
            _ => false,
        }
        && !toggle("MWCC_PCODE_EQUALITY_IN_ORDER")
}

/// The last general argument register.
const LAST_GENERAL_ARGUMENT: u32 = 10;

/// Bytes of the outgoing argument area the body's calls need.
fn outgoing_argument_bytes(body: &[Stmt]) -> u32 {
    let mut most = 0u32;
    let mut visit = |e: &Expr| {
        fn walk(e: &Expr, most: &mut u32) {
            if let ExprKind::Call { name, arguments } = &e.kind {
                let skip = usize::from(name == mwcc_iro::INDIRECT_CALL);
                let words = arguments.iter().skip(skip).filter(|a| !is_float(a.ty)).count() as u32;
                if words > 8 {
                    *most = (*most).max(4 * (words - 8));
                }
            }
            for child in expression_children(e) {
                walk(child, most);
            }
        }
        walk(e, &mut most);
    };
    for_each_statement_expression(body, &mut visit);
    most
}

/// The global an address expression points into (`&g`, `&g + i*k`).
fn root_global(base: &Expr) -> Option<String> {
    match &base.kind {
        ExprKind::GlobalAddress(name) => Some(name.clone()),
        ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, right) => {
            if matches!(left.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
                root_global(left)
            } else {
                root_global(right)
            }
        }
        _ => None,
    }
}

/// A reusable load's key: the pointer variable, offset and loaded kind
/// (any word is the same word).
/// `(x >> n) & m` of a bit-field read (its shift count carries the marker).
fn bit_field_extraction(expression: &Expr) -> bool {
    match &expression.kind {
        ExprKind::Binary(BinaryOp::BitAnd, left, _) => bit_field_extraction(left),
        ExprKind::Binary(BinaryOp::ShiftRight, _, count) => count.ty == mwcc_iro::BIT_FIELD_SHIFT && count.as_int().is_some(),
        ExprKind::Convert(operand) => bit_field_extraction(operand),
        _ => false,
    }
}

fn load_key(pointer: VarId, offset: i32, ty: Type) -> String {
    let kind = if is_general_word(ty) && !is_narrow(ty) && !toggle("MWCC_PCODE_TYPED_LOAD_KEYS") { "word".to_owned() } else { format!("{ty:?}") };
    format!("*{pointer}:{offset}:{kind}")
}

/// A cached load of a frame object whose address never escapes.
fn private_frame_key(key: &str, escaping: &[bool]) -> bool {
    !toggle("MWCC_PCODE_FRAME_LOADS_FORGET")
        && key
            .strip_prefix("*F")
            .and_then(|rest| rest.split(':').next())
            .and_then(|id| id.parse::<usize>().ok())
            .is_some_and(|id| !escaping[id])
}

/// Registers an expression tree needs (Ershov number; leaves need none).
fn register_need(e: &Expr) -> u32 {
    match &e.kind {
        ExprKind::Binary(_, left, right) => {
            let (a, b) = (register_need(left), register_need(right));
            if a == b { a + 1 } else { a.max(b) }
        }
        ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => register_need(operand).max(1),
        _ => 0,
    }
}

/// A variable, possibly retyped between words (no instruction).
fn plain_variable(expression: &Expr) -> bool {
    match &expression.kind {
        ExprKind::Var(_) => true,
        ExprKind::Convert(operand) => {
            !is_float(operand.ty) && !is_narrow(operand.ty) && !is_narrow(expression.ty) && !is_wide(operand.ty) && plain_variable(operand)
        }
        _ => false,
    }
}

thread_local! {
    /// The build bounds sums in narrowing conversions (GC/1.0-1.2.5n).
    static SUM_BOUNDS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `(x << n) | (x >> (32 - n))` (either order) on one unsigned variable: x and n.
fn rotation<'e>(left: &'e Expr, right: &'e Expr) -> Option<(&'e Expr, u8)> {
    let shifts = |e: &'e Expr| match &e.kind {
        ExprKind::Binary(op @ (BinaryOp::ShiftLeft | BinaryOp::ShiftRight), x, n) => Some((*op, x.as_ref(), n.as_int()?)),
        _ => None,
    };
    let ((op_a, x_a, n_a), (op_b, x_b, n_b)) = (shifts(left)?, shifts(right)?);
    if op_a == op_b || n_a + n_b != 32 || !matches!(x_a.kind, ExprKind::Var(_)) || format!("{:?}", x_a.kind) != format!("{:?}", x_b.kind) {
        return None;
    }
    if !is_unsigned(x_a.ty) {
        return None;
    }
    let left_count = if op_a == BinaryOp::ShiftLeft { n_a } else { n_b };
    Some((x_a, left_count as u8))
}
