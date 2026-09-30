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
    Function, GlobalInfo, Idiom, Place, Stmt, Type, UnaryOp, Unit, VarId, VariableKind,
};
use mwcc_machine_code::{Instruction, RelocationKind, RelocationTarget};
use mwcc_pcode::{AttachedRelocation, Block, Class, PCodeFunction, PInstr, Register, ReturnRegisters};

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
        Type::Float | Type::Double => ReturnRegisters::Float,
        other => return Err(unsupported(format!("return type {other:?}"))),
    };
    let mut lowerer = Lowerer {
        unit,
        function,
        pcode: PCodeFunction::new(function.name.clone(), returns),
        registers: vec![None; function.variables.len()],
        raw_narrow: vec![false; function.variables.len()],
        makes_calls: false,
        target: None,
        loaded_globals: HashMap::new(),
        pending_branches: Vec::new(),
        labels: Vec::new(),
        exit_label: Label(0),
        return_register: None,
        extended: HashMap::new(),
        constants: HashMap::new(),
        unoptimized,
        homes: vec![None; function.variables.len()],
        loops: Vec::new(),
        known_constant: None,
        tail_calls,
        common: HashMap::new(),
        contract,
        float_constants: HashMap::new(),
    };
    lowerer.lower_function(returns_through_variable)?;
    Ok(Lowered { pcode: lowerer.pcode, makes_calls: lowerer.makes_calls })
}

/// A branch target placed later in layout order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Label(usize);

struct Lowerer<'a> {
    unit: &'a Unit<'a>,
    function: &'a Function,
    pcode: PCodeFunction,
    /// Virtual register of each variable (temporaries on first assignment).
    registers: Vec<Option<u32>>,
    /// A narrow parameter held as it arrived: each block re-extends it at
    /// its first use (MWCC does not extend once on entry).
    raw_narrow: Vec<bool>,
    makes_calls: bool,
    /// Destination requested for the next expression's final operation.
    target: Option<u32>,
    /// Loaded values of non-volatile globals still valid (IRO common
    /// subexpressions); cleared by stores, calls and branches.
    loaded_globals: HashMap<String, (u32, Type)>,
    /// Branches awaiting their target block: (block, instruction, label).
    pending_branches: Vec<(usize, usize, Label)>,
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
    /// Frame home (`offset(r1)`) of a parameter kept in memory.
    homes: Vec<Option<i16>>,
    /// Enclosing loops' (break, continue) labels.
    loops: Vec<(Label, Label)>,
    /// The variable the previous statement set to a constant (IRO knows a
    /// loop's first test from it).
    known_constant: Option<(VarId, i64)>,
    /// A body that is one terminal call becomes a sibling branch.
    tail_calls: bool,
    /// Values of simple operations computed in this block (IRO common
    /// subexpressions): key -> (register, type, variables read).
    common: HashMap<String, (u32, Type, Vec<VarId>)>,
    /// `-fp_contract on`: multiply-add fuses.
    contract: bool,
    /// Floating constants loaded in this block: (bits, width) -> register.
    float_constants: HashMap<(u64, u8), u32>,
}

impl Lowerer<'_> {
    // ------------------------------------------------------------ blocks

    fn new_label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
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
    fn clear_block_caches(&mut self) {
        self.extended.clear();
        self.constants.clear();
        self.common.clear();
        self.float_constants.clear();
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
        // A label is a join: nothing computed before it is known here.
        self.clear_block_caches();
        let current = self.current_block();
        let block = if self.pcode.blocks[current].instructions.is_empty() && !self.block_is_target(current) {
            current
        } else {
            let falls_through = !self.block_ends_in_jump(current);
            self.start_block(falls_through)
        };
        self.labels[label.0] = Some(block);
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
        let block = self.current_block();
        let position = self.pcode.blocks[block].instructions.len();
        self.emit_plain(instruction);
        self.pending_branches.push((block, position, label));
        self.loaded_globals.clear();
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
        for id in 0..function.parameter_count {
            let ty = function.variables[id].ty;
            let register = self.fresh(ty);
            self.registers[id] = Some(register);
            self.raw_narrow[id] = is_narrow(ty) && !assigns(&function.body, id);
            let physical = if is_float(ty) {
                float_argument += 1;
                float_argument - 1
            } else {
                general_argument += 1;
                general_argument - 1
            };
            incoming.push((id, register, physical));
        }
        let locals: Vec<VarId> = (function.parameter_count..function.variables.len())
            .filter(|&id| function.variables[id].kind == VariableKind::Local)
            .collect();
        for &id in locals.iter().rev() {
            self.registers[id] = Some(self.fresh(function.variables[id].ty));
        }
        if function.return_type != Type::Void && returns_through_variable {
            self.return_register = Some(self.fresh(function.return_type));
        }
        self.pcode.begin_coalesce_window();

        for (id, virtual_register, physical) in incoming {
            let ty = function.variables[id].ty;
            if is_float(ty) {
                self.emit_plain(Instruction::FloatMove { d: virtual_register, b: physical });
                continue;
            }
            // A narrow parameter that is assigned is re-extended on entry.
            let ty = if self.raw_narrow[id] { Type::Int } else { ty };
            self.emit_plain(extension(ty, virtual_register, physical));
        }
        self.body_and_exit()
    }

    /// `-O0`: every variable lives in a fixed place for the whole function.
    /// A parameter of a leaf function stays in its argument register; in a
    /// function that calls, a parameter referenced at most once lives in the
    /// frame and the others are register variables, like declared locals
    /// (callee-saved, r31 down). Temporaries are colored around them.
    fn lower_unoptimized(&mut self, calls: bool) -> Compilation<()> {
        let function = self.function;
        if is_float(function.return_type) || function.variables.iter().any(|variable| is_float(variable.ty)) {
            return Err(unsupported("floating point at -O0"));
        }
        let mut home_slots: i16 = 0;
        let mut next_saved: u32 = 31;
        let mut entry_copies = Vec::new();
        for id in 0..function.parameter_count {
            let argument = FIRST_GENERAL_ARGUMENT + id as u32;
            if !calls {
                self.registers[id] = Some(argument);
                self.raw_narrow[id] = is_narrow(function.variables[id].ty);
            } else if references(&function.body, id) <= 1 {
                self.homes[id] = Some(8 + 4 * home_slots);
                home_slots += 1;
            } else {
                self.registers[id] = Some(next_saved);
                entry_copies.push((next_saved, argument));
                next_saved -= 1;
            }
        }
        for id in function.parameter_count..function.variables.len() {
            if function.variables[id].kind == VariableKind::Local {
                if next_saved < 14 {
                    return Err(unsupported("more register variables than callee-saved registers"));
                }
                self.registers[id] = Some(next_saved);
                next_saved -= 1;
            }
        }
        self.pcode.frame_local_bytes = ((4 * home_slots + 7) / 8) * 8;
        self.pcode.exit_uses = self.registers.iter().flatten().copied().collect();
        self.pcode.begin_coalesce_window();
        for id in 0..function.parameter_count {
            if let Some(offset) = self.homes[id] {
                let argument = FIRST_GENERAL_ARGUMENT + id as u32;
                self.emit_plain(store_instruction(function.variables[id].ty, argument, 1, offset));
            }
        }
        for (register, argument) in entry_copies {
            self.emit_plain(Instruction::Or { a: register, s: argument, b: argument });
        }
        self.body_and_exit()
    }

    /// A body that is just a call whose result (if any) is the function's
    /// becomes `b callee` after marshaling the arguments.
    fn sibling_call(&mut self) -> Compilation<bool> {
        let function = self.function;
        let call = match function.body.as_slice() {
            [Stmt::Eval(call)] if function.return_type == Type::Void => call,
            [Stmt::SetReturn(call)] if call.ty == function.return_type => call,
            _ => return Ok(false),
        };
        let ExprKind::Call { name, arguments } = &call.kind else { return Ok(false) };
        let mut values = Vec::new();
        for argument in arguments {
            values.push(self.expression(argument)?.0);
        }
        if arguments.iter().any(|argument| is_float(argument.ty)) || is_float(call.ty) {
            return Err(unsupported("floating sibling call"));
        }
        for (index, value) in values.into_iter().enumerate() {
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            self.emit_plain(Instruction::Or { a: register, s: value, b: value });
        }
        let mut branch = PInstr::new(Instruction::BranchExternal { target: name.clone() });
        branch.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(name.clone()),
        });
        branch.implicit_uses = (0..arguments.len()).map(|i| Register::general(FIRST_GENERAL_ARGUMENT + i as u32)).collect();
        self.emit(branch);
        self.pcode.ends_in_tail_call = true;
        self.start_block(false);
        self.resolve_branches()?;
        Ok(true)
    }

    fn body_and_exit(&mut self) -> Compilation<()> {
        let function = self.function;
        if self.tail_calls && !self.unoptimized && self.sibling_call()? {
            return Ok(());
        }
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
        self.resolve_branches()
    }

    fn statement(&mut self, statement: &Stmt) -> Compilation<()> {
        let known = self.known_constant.take();
        if let Stmt::Assign { variable, value } = statement {
            if let Some(value) = value.as_int() {
                self.known_constant = Some((*variable, value));
            }
        }
        match statement {
            Stmt::If { condition, then_body, else_body } if condition.as_int().is_some() && !self.unoptimized => {
                // IRO evaluates a constant condition: only one arm remains.
                let arm = if condition.as_int() != Some(0) { then_body } else { else_body };
                for statement in arm {
                    self.statement(statement)?;
                }
                Ok(())
            }
            Stmt::Loop { test_first, condition: Some(condition), body, step }
                if condition.as_int() == Some(0) && !self.unoptimized =>
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
            Stmt::Loop { condition, body, step, .. }
                if condition.as_ref().is_none_or(|c| c.as_int().is_some_and(|k| k != 0)) =>
            {
                // No test: the body repeats until a break.
                let top = self.new_label();
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
            Stmt::Loop { test_first, condition, body, step } => {
                if !self.unoptimized && counted(condition.as_ref(), body, step) && !makes_calls(body) {
                    return Err(unsupported("counted loop (unrolling not modeled)"));
                }
                let top = self.new_label();
                let next = self.new_label();
                let test = self.new_label();
                let exit = self.new_label();
                // IRO drops the entry jump when the first test is known true.
                let first_test_true = !self.unoptimized
                    && condition.as_ref().is_some_and(|condition| initially_true(condition, known));
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
                match condition {
                    Some(condition) => self.branch_on(condition, true, top)?,
                    None => self.jump(top),
                }
                self.place_label(exit);
                Ok(())
            }
            Stmt::Break => {
                let (exit, _) = *self.loops.last().ok_or_else(|| unsupported("break outside a loop"))?;
                self.jump(exit);
                Ok(())
            }
            Stmt::Continue => {
                let (_, next) = *self.loops.last().ok_or_else(|| unsupported("continue outside a loop"))?;
                self.jump(next);
                Ok(())
            }
            Stmt::Assign { variable, value } => self.assign(*variable, value),
            Stmt::Eval(value) => match &value.kind {
                ExprKind::Call { name, arguments } => self.call(name, arguments, Type::Void, None).map(|_| ()),
                // A discarded value without effects (`(void)x;`).
                ExprKind::Int(_) => Ok(()),
                _ => Err(unsupported("expression statement")),
            },
            Stmt::Store { place, ty, value } => self.store(place, *ty, value),
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
        let fits = mwcc_syntax_trees_to_iro_fits(value, return_type);
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
            let (register, _) = self.expression(value)?;
            self.copy(return_type, 1, register);
            return Ok(());
        }
        let direct = self.unoptimized.then_some(3);
        // The final instruction targets r3: the conversion when there is one.
        let converts = !fits && is_narrow(return_type) && value.ty != return_type;
        let raw = self.is_raw(value);
        let (register, ty) = self.expression_with_target(value, if converts { None } else { direct })?;
        let (register, _) =
            if fits { (register, ty) } else { self.convert_value(register, ty, raw, return_type, direct)? };
        if register != 3 {
            self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
        }
        Ok(())
    }

    fn assign(&mut self, variable: VarId, value: &Expr) -> Compilation<()> {
        if let Some(offset) = self.homes[variable] {
            let (source, _) = self.expression(value)?;
            self.emit_plain(store_instruction(self.function.variables[variable].ty, source, 1, offset));
            return Ok(());
        }
        self.common.retain(|_, (_, _, read)| !read.contains(&variable));
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
        let variable_type = self.function.variables[variable].ty;
        let narrow = is_narrow(variable_type) && !mwcc_syntax_trees_to_iro_fits(value, variable_type);
        let raw = self.is_raw(value);
        let (source, source_type) =
            self.expression_with_target(value, if narrow { None } else { Some(destination) })?;
        if narrow && (source_type != variable_type || raw) {
            let (converted, _) = self.convert(source, source_type, variable_type, Some(destination))?;
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
    fn branch_on(&mut self, condition: &Expr, when: bool, label: Label) -> Compilation<()> {
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
            _ => {
                // Truth of a value: compare against zero.
                let (register, _) = self.expression(condition)?;
                if !self.record_form(register) {
                    self.emit_plain(Instruction::CompareWordImmediate { a: register, immediate: 0 });
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
        if is_float(left.ty) || is_float(right.ty) {
            let (a, _) = self.expression(left)?;
            let (b, _) = self.expression(right)?;
            self.emit_plain(match op {
                BinaryOp::Less | BinaryOp::Greater => Instruction::FloatCompareOrdered { a, b },
                BinaryOp::Equal | BinaryOp::NotEqual => Instruction::FloatCompareUnordered { a, b },
                _ => return Err(unsupported("floating <= or >= (cror)")),
            });
            return Ok(comparison(op));
        }
        // A constant on the left compares mirrored so it can be an immediate.
        let (op, left, right) = if left.as_int().is_some() && right.as_int().is_none() {
            (op.mirror(), right, left)
        } else {
            (op, left, right)
        };
        let (a, left_type) = self.expression(left)?;
        let non_negative = right.as_int().map_or(is_unsigned_narrow(unpromoted(right).ty), |value| value >= 0);
        let unsigned = is_unsigned(promote(left_type))
            || is_unsigned(promote(right.ty))
            || is_unsigned_narrow(unpromoted(left).ty) && non_negative;
        // A compare of a just-computed value with 0 is its record form.
        let equality = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual);
        if right.as_int() == Some(0) && (!unsigned || equality) && self.record_form(a) {
            return Ok(comparison(op));
        }
        match (right.as_int(), unsigned) {
            (Some(value), false) if i16::try_from(value).is_ok() => {
                self.emit_plain(Instruction::CompareWordImmediate { a, immediate: value as i16 });
            }
            (Some(value), true) if u16::try_from(value).is_ok() => {
                self.emit_plain(Instruction::CompareLogicalWordImmediate { a, immediate: value as u16 });
            }
            _ => {
                let (b, _) = self.expression(right)?;
                self.emit_plain(if unsigned {
                    Instruction::CompareLogicalWord { a, b }
                } else {
                    Instruction::CompareWord { a, b }
                });
            }
        }
        Ok(comparison(op))
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
        let target = self.target.take();
        let ty = expression.ty;
        match &expression.kind {
            ExprKind::Float(value) => self.float_constant(*value, ty, target),
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
                let global = self.unit.globals[name];
                let external = || RelocationTarget::External(name.clone());
                if global.small_data {
                    let d = self.result(target);
                    let mut li = PInstr::new(Instruction::AddImmediate { d, a: 0, immediate: 0 });
                    li.relocation = Some(AttachedRelocation { kind: RelocationKind::EmbSda21, target: external() });
                    self.emit(li);
                    Ok((d, ty))
                } else {
                    Ok((self.absolute_address_into(name, target), ty))
                }
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
                let result = self.binary(*op, left, right, ty, target)?;
                if let Some((key, variables)) = key {
                    self.common.insert(key, (result.0, result.1, variables));
                }
                Ok(result)
            }
            ExprKind::Select { .. } => Err(unsupported("conditional expression")),
            ExprKind::Idiom(idiom) => self.idiom(idiom, target),
            ExprKind::Unary(UnaryOp::LogicalNot, operand) => {
                let inverted = match &operand.kind {
                    ExprKind::Binary(op, left, right) if op.is_comparison() => {
                        Expr::binary(op.invert(), (**left).clone(), (**right).clone(), Type::Int)
                    }
                    _ => Expr::binary(BinaryOp::Equal, (**operand).clone(), Expr::int(0), Type::Int),
                };
                self.expression_with_target(&inverted, target)
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
                let (a, _) = self.expression(&Expr::int(i64::from(high) << 16))?;
                self.load(ty, a, low, None, target)
            }
            ExprKind::Load { base, index, offset } => {
                if let (Some(index), true) = (index, self.unoptimized) {
                    // -O0: the index first; an absolute array's address is
                    // completed with add, then accessed at 0.
                    let (b, _) = self.expression(index)?;
                    let (a, _) = self.expression(base)?;
                    if self.absolute_base(base) {
                        let address = self.temporary();
                        self.emit_plain(Instruction::Add { d: address, a, b });
                        return self.load(ty, address, 0, None, target);
                    }
                    return self.indexed_load(ty, a, b, target);
                }
                let (a, _) = self.expression(base)?;
                match index {
                    Some(index) => {
                        let (b, _) = self.expression(index)?;
                        self.indexed_load(ty, a, b, target)
                    }
                    None => {
                        let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                        self.load(ty, a, offset, None, target)
                    }
                }
            }
            ExprKind::Convert(operand) if is_unsigned_narrow(ty) && max_value(operand).is_some_and(|max| max < 1u64 << (8 * width(ty))) => {
                let (source, _) = self.expression_with_target(operand, target)?;
                Ok((source, ty))
            }
            ExprKind::Convert(operand) => {
                // A promotion of a raw narrow parameter: extended once per block.
                if let ExprKind::Var(id) = operand.kind {
                    if self.raw_narrow[id] && !is_narrow(ty) {
                        if let Some(&extended) = self.extended.get(&id).filter(|_| !self.unoptimized) {
                            return Ok((extended, ty));
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
                    && std::env::var_os("MWCC_PCODE_INPLACE_EXTSB").is_some()
                {
                    let (source, _) = self.expression_with_target(operand, target)?;
                    self.emit_plain(Instruction::ExtendSignByte { a: source, s: source });
                    return Ok((source, ty));
                }
                let (source, source_type) = self.expression(operand)?;
                self.convert_value(source, source_type, raw, ty, target)
            }
            ExprKind::Call { name, arguments } => {
                let register = self.call(name, arguments, ty, target)?;
                Ok((register, ty))
            }
        }
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
        let extend_as = if is_narrow(to) {
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
            ExprKind::Var(id) => self.raw_narrow[*id] || self.homes[*id].is_some() && expression.ty == Type::Char,
            // A same-type conversion is a no-op: still raw.
            ExprKind::Convert(operand) if operand.ty == expression.ty => self.is_raw(operand),
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
            self.emit_based(Instruction::AddImmediate { d: register, a: register, immediate: low }, register);
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
        let address = self.absolute_address(name);
        self.load(global.ty, address, 0, None, target)
    }

    /// `lis; addi` forming an absolute symbol's address.
    fn absolute_address(&mut self, name: &str) -> u32 {
        self.absolute_address_into(name, None)
    }

    fn absolute_address_into(&mut self, name: &str, target: Option<u32>) -> u32 {
        let external = || RelocationTarget::External(name.to_owned());
        let high = self.temporary();
        let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
        lis.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Ha, target: external() });
        self.emit(lis);
        let d = self.result(target);
        let mut addi = PInstr::new(Instruction::AddImmediate { d, a: high, immediate: 0 });
        addi.relocation = Some(AttachedRelocation { kind: RelocationKind::Addr16Lo, target: external() });
        addi.not_r0.push(high);
        self.emit(addi);
        d
    }

    // ------------------------------------------------------------ floating point

    /// A floating constant: loaded from the pool (`lfs fD,@N@sda21(r0)`).
    fn float_constant(&mut self, value: f64, ty: Type, target: Option<u32>) -> Compilation<(u32, Type)> {
        let key = if ty == Type::Float { (u64::from((value as f32).to_bits()), 4u8) } else { (value.to_bits(), 8u8) };
        if target.is_none() {
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
        let (a, _) = self.expression(left)?;
        let (b, _) = self.expression(right)?;
        // A computed right operand of a commutative operation goes first.
        let commutative = matches!(op, BinaryOp::Add | BinaryOp::Multiply);
        let (a, b) = if commutative && right.as_var().is_none() { (b, a) } else { (a, b) };
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
            (Type::Float, Type::Double) => {
                let (source, _) = self.expression_with_target(operand, target)?;
                Ok((source, to))
            }
            (Type::Double, Type::Float) => {
                let (source, _) = self.expression(operand)?;
                let d = self.result_for(to, target);
                self.emit_plain(Instruction::RoundToSingle { d, b: source });
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
            Idiom::Masked { relation, tested, value, keep_when_true } => {
                let (a, _) = self.expression(tested)?;
                let mask = self.temporary();
                match relation {
                    BinaryOp::Less => {
                        self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: a, shift: 31 });
                    }
                    BinaryOp::Greater | BinaryOp::LessEqual => {
                        let negated = self.temporary();
                        self.emit_plain(Instruction::Negate { d: negated, a });
                        let combined = self.temporary();
                        self.emit_plain(if *relation == BinaryOp::Greater {
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
                    _ => unreachable!("sign idiom relations"),
                }
                let (b, ty) = self.expression(value)?;
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
        } else {
            (op, left, right)
        };
        let constant = right.as_int();
        let (p, left_type) = self.expression(left)?;
        let unsigned = is_unsigned(promote(left_type)) || is_unsigned(promote(right.ty));
        let small = |value: i64| i16::try_from(value).is_ok() && i16::try_from(-value).is_ok();
        // Forms against constants that need no second register.
        match (op, constant, unsigned) {
            (Equal, Some(0), _) => {
                let n = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: n, s: p });
                return self.shift_out(n, 5, target);
            }
            (Equal, Some(value), _) if small(value) => {
                let d = self.temporary();
                self.emit_plain(Instruction::SubtractFromImmediate { d, a: p, immediate: value as i16 });
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
        let (q, _) = self.expression(right)?;
        match (op, unsigned) {
            (Equal, _) => {
                let d = self.temporary();
                self.emit_plain(Instruction::SubtractFrom { d, a: p, b: q });
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
        if let (Some(n), ExprKind::Binary(BinaryOp::BitAnd, inner, mask), false) = (left_shift, &left.kind, self.unoptimized) {
            if let Some((begin, end)) = mask_bounds(mask) {
                if begin >= n {
                    let (x, _) = self.expression(inner)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: x, shift: n, begin: begin - n, end: end - n });
                    return Ok((d, ty));
                }
            }
        }
        // `field | base` where the base is known zero under the field's mask
        // inserts the field: `rlwimi base,x,shift,mb,me`.
        if op == BinaryOp::BitOr && !self.unoptimized {
            if let Some((source, shift, begin, end)) = insert_field(left) {
                if field_bits(begin, end) & !known_zero(right) == 0 {
                    let (x, _) = self.expression(source)?;
                    let (base, _) = self.expression(right)?;
                    // rlwimi overwrites its destination before reading the
                    // source: the destination must not hold the source.
                    let target = target.filter(|&d| d != x);
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
            if let Some((operand, shift, begin, end)) = shift_mask(left, right) {
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
                if begin > 0 && end >= n {
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
            (ExprKind::Var(id), BinaryOp::ShiftRight, Some(shift)) if self.raw_narrow[*id] => {
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
        let (a, _) = self.expression(left)?;
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
        let (b, _) = self.expression(right)?;
        // MWCC places a leaf operand first in a commutative operation whose
        // other operand is computed (`a*b + c` -> `add r3,c,t`).
        let leaf = |e: &Expr| unpromoted(e).as_var().is_some();
        let (a, b) = if op.is_commutative() && !leaf(left) && leaf(right) { (b, a) } else { (a, b) };
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

    // ------------------------------------------------------------ stores

    /// The register holding a stored value: a raw narrow parameter at least
    /// as wide as the store needs no extension.
    /// An absolute (`lis`/`addi`) global address.
    fn absolute_base(&self, base: &Expr) -> bool {
        matches!(&base.kind, ExprKind::GlobalAddress(name) if !self.unit.globals[name].small_data)
    }

    fn store_source(&mut self, value: &Expr, _stored: Type) -> Compilation<u32> {
        Ok(self.expression(value)?.0)
    }

    fn store(&mut self, place: &Place, ty: Type, value: &Expr) -> Compilation<()> {
        if is_float(ty) != is_float(value.ty) {
            return Err(unsupported("store of a value in the other register class"));
        }
        let source = self.store_source(value, ty)?;
        let (base, offset, index, relocation) = match place {
            Place::Memory { base, index: None, offset } if base.as_int().is_some() => {
                let (high, low) = split_address(base.as_int().expect("checked") + i64::from(*offset))?;
                let (a, _) = self.expression(&Expr::int(i64::from(high) << 16))?;
                (a, low, None, None)
            }
            Place::Memory { base: base_expression, index: Some(index), .. } if self.unoptimized => {
                let (b, _) = self.expression(index)?;
                let (a, _) = self.expression(base_expression)?;
                if self.absolute_base(base_expression) {
                    let address = self.temporary();
                    self.emit_plain(Instruction::Add { d: address, a, b });
                    (address, 0, None, None)
                } else {
                    (a, 0, Some(b), None)
                }
            }
            Place::Memory { base, index, offset } => {
                let (base, _) = self.expression(base)?;
                let index = match index {
                    Some(index) => Some(self.expression(index)?.0),
                    None => None,
                };
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
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
                    (self.absolute_address(name), 0, None, None)
                }
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
        instruction.relocation = relocation;
        self.emit(instruction);
        self.loaded_globals.clear();
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

    fn call(&mut self, name: &str, arguments: &[Expr], ty: Type, target: Option<u32>) -> Compilation<u32> {
        let mut values = Vec::new();
        for argument in arguments {
            // A constant argument is loaded straight into its register.
            values.push(match argument.as_int() {
                Some(_) if !self.unoptimized => None,
                _ => Some(self.expression(argument)?.0),
            });
        }
        let mut argument_registers = Vec::new();
        let (mut general, mut float) = (FIRST_GENERAL_ARGUMENT, 1);
        for (index, value) in values.into_iter().enumerate() {
            if is_float(arguments[index].ty) {
                let value = value.expect("floating arguments are evaluated");
                self.emit_plain(Instruction::FloatMove { d: float, b: value });
                argument_registers.push(Register::float(float));
                float += 1;
                continue;
            }
            let register = general;
            general += 1;
            match value {
                Some(value) => self.emit_plain(Instruction::Or { a: register, s: value, b: value }),
                None => self.load_constant(register, arguments[index].as_int().expect("constant"))?,
            }
            argument_registers.push(Register::general(register));
        }
        let mut call = PInstr::new(Instruction::BranchAndLink { target: name.to_owned() });
        call.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(name.to_owned()),
        });
        call.implicit_uses = argument_registers;
        call.implicit_defs = VOLATILE_GENERAL.iter().map(|&r| Register::general(r)).collect();
        call.implicit_defs.extend((0..14).map(Register::float));
        self.emit(call);
        self.makes_calls = true;
        self.loaded_globals.clear();
        // Constants are rematerialized rather than kept across a call.
        self.constants.clear();
        self.float_constants.clear();
        let result = self.result_for(ty, target);
        self.copy(ty, result, result_register(ty));
        Ok(result)
    }
}

use mwcc_syntax_trees_to_iro::passes::fits_unconverted as mwcc_syntax_trees_to_iro_fits;

/// Whether any statement assigns `variable`.
fn assigns(body: &[Stmt], variable: VarId) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Assign { variable: assigned, .. } => *assigned == variable,
        Stmt::If { then_body, else_body, .. } => assigns(then_body, variable) || assigns(else_body, variable),
        Stmt::Loop { body, step, .. } => assigns(body, variable) || assigns(step, variable),
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
fn insert_field(expression: &Expr) -> Option<(&Expr, u8, u8, u8)> {
    let ExprKind::Binary(op, left, right) = &expression.kind else { return None };
    match (op, right.as_int()) {
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

/// Bits of `expression`'s value known to be zero.
fn known_zero(expression: &Expr) -> u32 {
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
fn makes_calls(body: &[Stmt]) -> bool {
    fn expression(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Call { .. } => true,
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Var(_) | ExprKind::Global(_) | ExprKind::GlobalAddress(_) => false,
            ExprKind::Load { base, index, .. } => expression(base) || index.as_deref().is_some_and(expression),
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand),
            ExprKind::Binary(_, left, right) => expression(left) || expression(right),
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition) || expression(when_true) || expression(when_false)
            }
            ExprKind::Idiom(Idiom::Absolute(value)) => expression(value),
            ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => expression(tested) || expression(value),
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
        Stmt::Loop { condition, body, step, .. } => {
            condition.as_ref().is_some_and(expression) || makes_calls(body) || makes_calls(step)
        }
        Stmt::Break | Stmt::Continue => false,
    })
}

/// How many times the body reads or assigns `variable`.
fn references(body: &[Stmt], variable: VarId) -> usize {
    fn expression(e: &Expr, variable: VarId) -> usize {
        match &e.kind {
            ExprKind::Var(id) => usize::from(*id == variable),
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Global(_) | ExprKind::GlobalAddress(_) => 0,
            ExprKind::Load { base, index, .. } => {
                expression(base, variable) + index.as_deref().map_or(0, |index| expression(index, variable))
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand, variable),
            ExprKind::Binary(_, left, right) => expression(left, variable) + expression(right, variable),
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition, variable) + expression(when_true, variable) + expression(when_false, variable)
            }
            ExprKind::Call { arguments, .. } => arguments.iter().map(|a| expression(a, variable)).sum(),
            ExprKind::Idiom(Idiom::Absolute(value)) => expression(value, variable),
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
                expression(value, variable)
                    + match place {
                        Place::Memory { base, index, .. } => {
                            expression(base, variable)
                                + index.as_deref().map_or(0, |index| expression(index, variable))
                        }
                        Place::Global(_) => 0,
                    }
            }
            Stmt::If { condition, then_body, else_body } => {
                expression(condition, variable) + references(then_body, variable) + references(else_body, variable)
            }
            Stmt::Loop { condition, body, step, .. } => {
                condition.as_ref().map_or(0, |c| expression(c, variable))
                    + references(body, variable)
                    + references(step, variable)
            }
            Stmt::Break | Stmt::Continue => 0,
        })
        .sum()
}

/// Whether `condition` holds on entry given the preceding constant assignment.
fn initially_true(condition: &Expr, known: Option<(VarId, i64)>) -> bool {
    let Some((variable, value)) = known else { return false };
    let ExprKind::Binary(op, left, right) = &condition.kind else { return false };
    let (Some(id), Some(bound)) = (unpromoted(left).as_var(), right.as_int()) else { return false };
    if id != variable {
        return false;
    }
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
        Type::Char | Type::UnsignedChar => Instruction::StoreByte { s, a, offset },
        Type::Short | Type::UnsignedShort => Instruction::StoreHalfword { s, a, offset },
        _ => Instruction::StoreWord { s, a, offset },
    }
}

/// The operand of an integer promotion (or the expression itself).
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
            Some(max_value(left)? >> right.as_int()?)
        }
        ExprKind::Binary(op, ..) if op.is_comparison() => Some(1),
        _ => None,
    }
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
