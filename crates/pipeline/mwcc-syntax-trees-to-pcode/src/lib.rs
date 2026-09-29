//! INITIAL CODE: lower a function's syntax tree to PCode over virtual
//! registers, as MWCC's `CodeGen_Generator` does before any backend pass.
//!
//! Virtual-register numbering follows MWCC's object-preallocation walk
//! (`CodeGen_PreallocateObjectRegisters`): parameters, then declared locals,
//! then the coalescing window opens and lowering temporaries follow in
//! creation order. Every unsupported construct rejects the whole function
//! with a diagnostic so the caller can fall back to another lowering.

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind, RelocationTarget};
use mwcc_pcode::{
    AttachedRelocation, Block, Class, PCodeFunction, PInstr, Register, ReturnRegisters,
};
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, Pointee, Statement, Type, UnaryOperator,
};

/// A file-scope object as lowering sees it.
#[derive(Debug, Clone, Copy)]
pub struct GlobalInfo {
    pub ty: Type,
    /// Addressed through the small-data base (`@sda21`) rather than `lis/@l`.
    pub small_data: bool,
    /// An array object: its name denotes its address, not a loadable value.
    pub is_array: bool,
    /// Every read and write is an observable access (never reused).
    pub is_volatile: bool,
}

/// Unit-level facts lowering needs.
pub struct LoweringContext<'a> {
    pub globals: &'a HashMap<String, GlobalInfo>,
    pub call_return_types: &'a HashMap<String, Type>,
    /// Calls the compiler expands inline (`__cntlzw`, `__sync`, ...).
    pub is_intrinsic: &'a dyn Fn(&str, usize) -> bool,
    /// Callees declared with `...` (the caller must set CR1 for them).
    pub variadic_callees: &'a std::collections::HashSet<String>,
    /// Callees with a prototype in scope.
    pub prototyped: &'a std::collections::HashSet<String>,
    /// Declared parameter types of callees.
    pub call_parameter_types: &'a HashMap<String, Vec<Type>>,
}

/// A lowered function.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub pcode: PCodeFunction,
    /// The function calls another (it needs a frame and the saved LR).
    pub makes_calls: bool,
}

const FIRST_GENERAL_ARGUMENT: u32 = 3;
const LAST_GENERAL_ARGUMENT: u32 = 10;
const VOLATILE_GENERAL: [u32; 11] = [0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

fn unsupported(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("PCode lowering: {} (not yet supported)", what.into()))
}

/// Lower `function` to PCode.
pub fn lower(function: &Function, context: &LoweringContext<'_>) -> Compilation<Lowered> {
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return Err(unsupported("inline assembly"));
    }
    let returns = match function.return_type {
        Type::Void => ReturnRegisters::None,
        ty if is_general_word(ty) => ReturnRegisters::General,
        other => return Err(unsupported(format!("return type {other:?}"))),
    };
    let mut lowerer = Lowerer {
        context,
        pcode: PCodeFunction::new(function.name.clone(), returns),
        variables: HashMap::new(),
        makes_calls: false,
        return_type: function.return_type,
        target: None,
        loaded_globals: HashMap::new(),
        pending_branches: Vec::new(),
        labels: Vec::new(),
        exit_label: Label(0),
        return_register: None,
        hoisted: None,
        extended: HashMap::new(),
    };
    lowerer.lower_function(function)?;
    Ok(Lowered { pcode: lowerer.pcode, makes_calls: lowerer.makes_calls })
}

fn is_general_word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::UnsignedInt
            | Type::Char
            | Type::UnsignedChar
            | Type::Short
            | Type::UnsignedShort
            | Type::Pointer(_)
            | Type::StructPointer { .. }
    )
}

fn is_unsigned(ty: Type) -> bool {
    matches!(
        ty,
        Type::UnsignedInt
            | Type::UnsignedChar
            | Type::UnsignedShort
            | Type::Pointer(_)
            | Type::StructPointer { .. }
    )
}

/// The C integer promotion of an operand type.
fn promote(ty: Type) -> Type {
    match ty {
        Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort => Type::Int,
        other => other,
    }
}

fn pointee_type(pointee: Pointee) -> Option<Type> {
    Some(match pointee {
        Pointee::Int => Type::Int,
        Pointee::UnsignedInt => Type::UnsignedInt,
        Pointee::Char => Type::Char,
        Pointee::UnsignedChar => Type::UnsignedChar,
        Pointee::Short => Type::Short,
        Pointee::UnsignedShort => Type::UnsignedShort,
        Pointee::Pointer | Pointee::WordPointer => Type::Pointer(Pointee::Int),
        _ => return None,
    })
}

struct Variable {
    register: u32,
    ty: Type,
    /// A narrow parameter held as it arrived: each block re-extends it at
    /// its first use (MWCC does not extend once on entry).
    raw_narrow: bool,
}

struct Lowerer<'a> {
    context: &'a LoweringContext<'a>,
    pcode: PCodeFunction,
    variables: HashMap<String, Variable>,
    makes_calls: bool,
    return_type: Type,
    /// Destination requested for the next expression's final operation
    /// (an assignment computes straight into its variable's register).
    target: Option<u32>,
    /// Loaded values of non-volatile globals still valid in this statement
    /// (IRO common subexpressions); cleared by stores and calls.
    loaded_globals: HashMap<String, (u32, Type)>,
    /// Branches awaiting their target block: (block, instruction, label).
    pending_branches: Vec<(usize, usize, Label)>,
    /// Label targets once placed: label -> block.
    labels: Vec<Option<usize>>,
    /// The exit (return) block's label.
    exit_label: Label,
    /// The return value's variable when the function returns from several
    /// places: each `return` assigns it and jumps to the exit block, which
    /// copies it to r3 (MWCC's single return point).
    return_register: Option<u32>,
    /// An arm assignment hoisted above the next conditional branch (a select).
    hoisted: Option<Hoisted>,
    /// Extensions of raw narrow parameters made in the current block.
    extended: HashMap<String, u32>,
}

/// One arm of a two-way select, evaluated unconditionally ahead of the
/// branch that skips the other arm.
#[derive(Clone)]
enum Hoisted {
    Return(Expression),
    Assign(String, Expression),
}

/// A branch target placed later in layout order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Label(usize);

impl Lowerer<'_> {
    fn new_label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    fn current_block(&self) -> usize {
        self.pcode.blocks.len() - 1
    }

    /// Start a new block in layout order. `falls_through` links the previous
    /// block to it.
    fn start_block(&mut self, falls_through: bool) -> usize {
        let previous = self.current_block();
        self.pcode.blocks.push(Block { weight: 1, ..Block::default() });
        self.extended.clear();
        let block = self.current_block();
        if falls_through {
            self.pcode.blocks[previous].successors.push(block);
        }
        block
    }

    /// Place `label` at a fresh block (or the current one if it is empty).
    fn place_label(&mut self, label: Label) {
        let current = self.current_block();
        let block = if self.pcode.blocks[current].instructions.is_empty()
            && !self.block_is_target(current)
        {
            current
        } else {
            let falls_through = !self.block_ends_in_jump(current);
            self.start_block(falls_through)
        };
        self.labels[label.0] = Some(block);
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

    /// Emit a branch to `label`; conditional branches end their block.
    fn branch(&mut self, instruction: Instruction, label: Label) -> Compilation<()> {
        let conditional = !matches!(instruction, Instruction::Branch { .. });
        if conditional {
            if let Some(hoisted) = self.hoisted.take() {
                self.emit_hoisted(hoisted)?;
            }
        }
        let block = self.current_block();
        let position = self.pcode.blocks[block].instructions.len();
        self.emit_plain(instruction);
        self.pending_branches.push((block, position, label));
        self.loaded_globals.clear();
        if conditional {
            self.start_block(true);
        } else {
            // An unconditional jump ends the block with no fall-through.
            self.start_block(false);
        }
        Ok(())
    }

    fn emit_hoisted(&mut self, hoisted: Hoisted) -> Compilation<()> {
        match hoisted {
            Hoisted::Return(value) => self.return_value(&value),
            Hoisted::Assign(name, value) => self.assign(&name, &value),
        }
    }

    /// `if (condition) <then> else <otherwise>` where each arm is one
    /// assignment of the same destination: MWCC assigns one arm before the
    /// branch and conditionally overwrites it. The else arm goes first unless
    /// only the then arm is a constant. Returns false when not a select.
    fn select(
        &mut self,
        condition: &Expression,
        then_arm: Hoisted,
        else_arm: Hoisted,
        join: Label,
    ) -> Compilation<bool> {
        // `if (!c) A else B` is `if (c) B else A`.
        if let Expression::Unary { operator: UnaryOperator::LogicalNot, operand } = condition {
            return self.select(operand, else_arm, then_arm, join);
        }
        let value = |arm: &Hoisted| match arm {
            Hoisted::Return(value) | Hoisted::Assign(_, value) => value.clone(),
        };
        let (then_value, else_value) = (value(&then_arm), value(&else_arm));
        if self.signed_idiom(condition, &then_value, &else_value).is_some() {
            let selected = Expression::Conditional {
                condition: Box::new(condition.clone()),
                when_true: Box::new(then_value),
                when_false: Box::new(else_value),
                origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
            };
            let arm = match then_arm {
                Hoisted::Return(_) => Hoisted::Return(selected),
                Hoisted::Assign(name, _) => Hoisted::Assign(name, selected),
            };
            self.emit_hoisted(arm)?;
            return Ok(true);
        }
        if simple_condition(condition) {
            if let (Expression::IntegerLiteral(1), Expression::IntegerLiteral(0)) = (&then_value, &else_value) {
                let truth = match condition {
                    Expression::Binary { operator, .. } if comparison(*operator).is_some() => condition.clone(),
                    other => Expression::Binary {
                        operator: BinaryOperator::NotEqual,
                        left: Box::new(other.clone()),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    },
                };
                let arm = match then_arm {
                    Hoisted::Return(_) => Hoisted::Return(truth),
                    Hoisted::Assign(name, _) => Hoisted::Assign(name, truth),
                };
                self.emit_hoisted(arm)?;
                return Ok(true);
            }
            if let (Expression::IntegerLiteral(0), Expression::IntegerLiteral(1)) = (&then_value, &else_value) {
                let negated = Expression::Unary { operator: UnaryOperator::LogicalNot, operand: Box::new(condition.clone()) };
                let arm = match then_arm {
                    Hoisted::Return(_) => Hoisted::Return(negated),
                    Hoisted::Assign(name, _) => Hoisted::Assign(name, negated),
                };
                self.emit_hoisted(arm)?;
                return Ok(true);
            }
        }
        if !simple_condition(condition) || !speculable(&then_value) || !speculable(&else_value) {
            return Ok(false);
        }
        let constant = |value: &Expression| matches!(value, Expression::IntegerLiteral(_));
        let then_first = constant(&then_value) && !constant(&else_value);
        let (first, second) = if then_first { (then_arm, else_arm) } else { (else_arm, then_arm) };
        if std::env::var_os("MWCC_PCODE_HOIST_LATE").is_some() {
            self.hoisted = Some(first);
        } else {
            // IRO makes the first arm a statement ahead of the `if`.
            self.emit_hoisted(first)?;
        }
        self.branch_on(condition, then_first, join)?;
        self.hoisted = None;
        self.emit_hoisted(second)?;
        Ok(true)
    }

    /// Resolve pending branches: targets become block indices (flattening
    /// converts them to instruction positions) and edges are recorded.
    fn resolve_branches(&mut self) -> Compilation<()> {
        for (block, position, label) in std::mem::take(&mut self.pending_branches) {
            let target = self.labels[label.0].ok_or_else(|| unsupported("unplaced label"))?;
            match &mut self.pcode.blocks[block].instructions[position].instruction {
                Instruction::Branch { target: field }
                | Instruction::BranchConditionalForward { target: field, .. } => *field = target,
                _ => unreachable!("pending branches are branches"),
            }
            if !self.pcode.blocks[block].successors.contains(&target) {
                self.pcode.blocks[block].successors.push(target);
            }
        }
        Ok(())
    }

    fn expression_with_target(
        &mut self,
        expression: &Expression,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        self.target = target;
        let evaluated = self.expression(expression);
        self.target = None;
        evaluated
    }

    fn result(&mut self, target: Option<u32>) -> u32 {
        target.unwrap_or_else(|| self.temporary())
    }

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

    /// Branch to `label` when `condition` is false (fall through when true).
    fn branch_unless(&mut self, condition: &Expression, label: Label) -> Compilation<()> {
        self.branch_on(condition, false, label)
    }

    /// Branch to `label` when `condition` evaluates to `when`.
    fn branch_on(&mut self, condition: &Expression, when: bool, label: Label) -> Compilation<()> {
        match condition {
            Expression::Binary { operator: BinaryOperator::LogicalAnd, left, right } => {
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
            Expression::Binary { operator: BinaryOperator::LogicalOr, left, right } => {
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
            Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => {
                self.branch_on(operand, !when, label)
            }
            Expression::Binary { operator, left, right } if comparison(*operator).is_some() => {
                let (bit, true_when_set) = self.compare(*operator, left, right)?;
                // BO 12: branch if the bit is set; BO 4: if it is clear.
                let options = if when == true_when_set { 12 } else { 4 };
                self.branch(
                    Instruction::BranchConditionalForward { options, condition_bit: bit, target: 0 },
                    label,
                )?;
                Ok(())
            }
            other => {
                // Truth of a value: compare against zero.
                let (register, _) = self.expression(other)?;
                if !self.record_form(register) {
                    self.emit_plain(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                }
                let options = if when { 4 } else { 12 };
                self.branch(
                    Instruction::BranchConditionalForward { options, condition_bit: 2, target: 0 },
                    label,
                )?;
                Ok(())
            }
        }
    }

    /// Emit a compare for `left op right`; returns the cr0 bit to test and
    /// whether the relation holds when that bit is set.
    fn compare(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<(u8, bool)> {
        // A constant on the left compares mirrored so it can be an immediate.
        let (operator, left, right) = match (left, right) {
            (Expression::IntegerLiteral(_), other) if !matches!(other, Expression::IntegerLiteral(_)) => {
                (mirror(operator), right, left)
            }
            _ => (operator, left, right),
        };
        let (a, left_type) = self.expression(left)?;
        let unsigned = is_unsigned(promote(left_type))
            || self.static_type(right).is_some_and(|ty| is_unsigned(promote(ty)))
            || (is_unsigned_narrow(left_type) && self.non_negative(right));
        // A compare of a just-computed value with 0 is its record form.
        let equality = matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual);
        if matches!(right, Expression::IntegerLiteral(0)) && (!unsigned || equality) && self.record_form(a) {
            return Ok(comparison(operator).expect("checked by the caller"));
        }
        match (right, unsigned) {
            (Expression::IntegerLiteral(value), false) if i16::try_from(*value).is_ok() => {
                self.emit_plain(Instruction::CompareWordImmediate { a, immediate: *value as i16 });
            }
            (Expression::IntegerLiteral(value), true) if u16::try_from(*value).is_ok() => {
                self.emit_plain(Instruction::CompareLogicalWordImmediate { a, immediate: *value as u16 });
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
        Ok(comparison(operator).expect("checked by the caller"))
    }

    /// [`sign_idiom`] when the tested value is a signed word.
    fn signed_idiom<'e>(
        &self,
        condition: &'e Expression,
        when_true: &'e Expression,
        when_false: &'e Expression,
    ) -> Option<SignIdiom<'e>> {
        let idiom = sign_idiom(condition, when_true, when_false)?;
        let tested = match &idiom {
            SignIdiom::Absolute(tested) | SignIdiom::Masked { tested, .. } => *tested,
        };
        (self.static_type(tested) == Some(Type::Int)).then_some(idiom)
    }

    /// A sign-mask select (see [`sign_idiom`]).
    fn sign_select(&mut self, idiom: SignIdiom<'_>, target: Option<u32>) -> Compilation<(u32, Type)> {
        match idiom {
            SignIdiom::Absolute(value) => {
                let (a, _) = self.expression(value)?;
                let sign = self.temporary();
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: sign, s: a, shift: 31 });
                let flipped = self.temporary();
                self.emit_plain(Instruction::Xor { a: flipped, s: sign, b: a });
                let d = self.result(target);
                self.emit_plain(Instruction::SubtractFrom { d, a: sign, b: flipped });
                Ok((d, Type::Int))
            }
            SignIdiom::Masked { relation, tested, value, keep_when_true } => {
                let (a, _) = self.expression(tested)?;
                let mask = self.temporary();
                match relation {
                    BinaryOperator::Less => {
                        self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: a, shift: 31 });
                    }
                    BinaryOperator::Greater | BinaryOperator::LessEqual => {
                        let negated = self.temporary();
                        self.emit_plain(Instruction::Negate { d: negated, a });
                        let combined = self.temporary();
                        self.emit_plain(if relation == BinaryOperator::Greater {
                            Instruction::AndComplement { a: combined, s: negated, b: a }
                        } else {
                            Instruction::OrComplement { a: combined, s: a, b: negated }
                        });
                        self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: combined, shift: 31 });
                    }
                    BinaryOperator::GreaterEqual => {
                        let sign = self.temporary();
                        self.emit_plain(Instruction::ShiftRightLogicalImmediate { a: sign, s: a, shift: 31 });
                        self.emit_based(Instruction::AddImmediate { d: mask, a: sign, immediate: -1 }, sign);
                    }
                    _ => unreachable!("sign_idiom relations"),
                }
                let (b, ty) = self.expression(value)?;
                let d = self.result(target);
                self.emit_plain(if keep_when_true {
                    Instruction::And { a: d, s: b, b: mask }
                } else {
                    Instruction::AndComplement { a: d, s: b, b: mask }
                });
                Ok((d, promote(ty)))
            }
        }
    }

    /// A comparison's 0/1 value, branch-free as MWCC generates it.
    fn comparison_value(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        use BinaryOperator::*;
        let (operator, left, right) = match (left, right) {
            (Expression::IntegerLiteral(_), other) if !matches!(other, Expression::IntegerLiteral(_)) => {
                (mirror(operator), right, left)
            }
            _ => (operator, left, right),
        };
        let unsigned = self.static_type(left).is_some_and(|ty| is_unsigned(promote(ty)))
            || self.static_type(right).is_some_and(|ty| is_unsigned(promote(ty)));
        let constant = match right {
            Expression::IntegerLiteral(value) => Some(*value),
            _ => None,
        };
        let (p, left_type) = self.expression(left)?;
        let unsigned = unsigned || is_unsigned(promote(left_type));
        let small = |value: i64| i16::try_from(value).is_ok() && i16::try_from(-value).is_ok();
        // Forms against constants that need no second register.
        match (operator, constant, unsigned) {
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
                // `1 rotl clz(p)` keeps bit 31 only when p <= 0, computed in place.
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
        match (operator, unsigned) {
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

    /// Whether `expression` is known to be non-negative after promotion.
    fn non_negative(&self, expression: &Expression) -> bool {
        match expression {
            Expression::IntegerLiteral(value) => *value >= 0,
            other => self.static_type(other).is_some_and(is_unsigned_narrow),
        }
    }

    /// Turn the current block's last instruction into its record form when it
    /// defines `register` (so cr0 compares that result with 0).
    fn record_form(&mut self, register: u32) -> bool {
        if std::env::var_os("MWCC_PCODE_NO_RECORD").is_some() {
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
            RotateAndMask { a, s, shift, begin, end } if a == register => {
                RotateAndMaskRecord { a, s, shift, begin, end }
            }
            ShiftRightAlgebraicImmediate { a, s, shift } if a == register => {
                ShiftRightAlgebraicImmediateRecord { a, s, shift }
            }
            AndImmediateRecord { a, .. } if a == register => return true,
            _ => return false,
        };
        last.instruction = record;
        true
    }

    /// `x` rotated and masked, as `rlwimi`'s inserted field: `x << n`,
    /// unsigned `x >> n`, `x & mask`, `(x >> n) & mask`.
    fn insert_field<'e>(&self, expression: &'e Expression) -> Option<(&'e Expression, u8, u8, u8)> {
        let Expression::Binary { operator, left, right } = expression else { return None };
        let amount = match right.as_ref() {
            Expression::IntegerLiteral(value) => Some(*value),
            _ => None,
        };
        match (operator, amount) {
            (BinaryOperator::ShiftLeft, Some(n)) if (1..32).contains(&n) => {
                Some((left, n as u8, 0, 31 - n as u8))
            }
            (BinaryOperator::ShiftRight, Some(n))
                if (1..32).contains(&n) && self.static_type(left).is_some_and(|ty| is_unsigned(promote(ty))) =>
            {
                Some((left, (32 - n) as u8, n as u8, 31))
            }
            (BinaryOperator::BitAnd, _) => {
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
    fn known_zero(&self, expression: &Expression) -> u32 {
        let Expression::Binary { operator, left, right } = expression else { return 0 };
        match (operator, right.as_ref()) {
            (BinaryOperator::ShiftLeft, Expression::IntegerLiteral(n)) if (1..32).contains(n) => (1u32 << n) - 1,
            (BinaryOperator::ShiftRight, Expression::IntegerLiteral(n))
                if (1..32).contains(n) && self.static_type(left).is_some_and(|ty| is_unsigned(promote(ty))) =>
            {
                !(u32::MAX >> n)
            }
            (BinaryOperator::BitAnd, Expression::IntegerLiteral(mask)) => !(*mask as u32),
            _ => 0,
        }
    }

    /// The type an expression evaluates to, where it is known without
    /// lowering it (enough to recognize pointer operands).
    fn static_type(&self, expression: &Expression) -> Option<Type> {
        match expression {
            Expression::Variable(name) => self
                .variables
                .get(name)
                .map(|variable| variable.ty)
                .or_else(|| {
                    self.context
                        .globals
                        .get(name)
                        .filter(|global| !global.is_array)
                        .map(|global| global.ty)
                }),
            Expression::Member { member_type, .. } => Some(*member_type),
            Expression::Cast { target_type, .. } => Some(*target_type),
            Expression::Call { name, .. } => self.context.call_return_types.get(name).copied(),
            Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right } => {
                let left = self.static_type(left);
                if left.is_some_and(|ty| element_size(ty).is_some()) {
                    return left;
                }
                self.static_type(right).filter(|ty| element_size(*ty).is_some())
            }
            _ => None,
        }
    }

    /// A parameter or local reference: already in a register.
    fn is_register_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name) if self.variables.contains_key(name))
    }

    fn temporary(&mut self) -> u32 {
        self.pcode.fresh(Class::General)
    }

    fn lower_function(&mut self, function: &Function) -> Compilation<()> {
        // Object preallocation: parameters, then locals in reverse declaration
        // order, then the coalescing window opens.
        let mut incoming = Vec::new();
        for (index, parameter) in function.parameters.iter().enumerate() {
            if !is_general_word(parameter.parameter_type) {
                return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
            }
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            if register > LAST_GENERAL_ARGUMENT {
                return Err(unsupported("stack-passed parameters"));
            }
            let virtual_register = self.temporary();
            let raw_narrow = is_narrow(parameter.parameter_type)
                && !may_assign(&function.statements, &parameter.name);
            self.variables.insert(
                parameter.name.clone(),
                Variable { register: virtual_register, ty: parameter.parameter_type, raw_narrow },
            );
            incoming.push((virtual_register, register));
        }
        for local in function.locals.iter().rev() {
            if local.is_static || local.is_volatile || local.array_length.is_some() {
                return Err(unsupported("static, volatile, or array locals"));
            }
            if !is_general_word(local.declared_type) {
                return Err(unsupported(format!("local type {:?}", local.declared_type)));
            }
            let register = self.temporary();
            self.variables
                .insert(local.name.clone(), Variable { register, ty: local.declared_type, raw_narrow: false });
        }
        if function.return_type != Type::Void && returns_from_several_places(function) {
            self.return_register = Some(self.temporary());
        }
        self.pcode.begin_coalesce_window();

        for (virtual_register, register) in incoming {
            // Narrow parameters are re-extended by the callee on entry.
            let (ty, raw_narrow) = self
                .variables
                .values()
                .find(|variable| variable.register == virtual_register)
                .map(|variable| (variable.ty, variable.raw_narrow))
                .unwrap_or((Type::Int, false));
            let ty = if raw_narrow { Type::Int } else { ty };
            let instruction = match ty {
                Type::Char => Instruction::ExtendSignByte { a: virtual_register, s: register },
                Type::Short => Instruction::ExtendSignHalfword { a: virtual_register, s: register },
                Type::UnsignedChar => {
                    Instruction::ClearLeftImmediate { a: virtual_register, s: register, clear: 24 }
                }
                Type::UnsignedShort => {
                    Instruction::ClearLeftImmediate { a: virtual_register, s: register, clear: 16 }
                }
                _ => Instruction::Or { a: virtual_register, s: register, b: register },
            };
            self.emit_plain(instruction);
        }
        for local in &function.locals {
            if let Some(initializer) = &local.initializer {
                self.assign(&local.name, initializer)?;
            }
        }
        self.exit_label = self.new_label();
        for statement in &function.statements {
            self.statement(statement)?;
        }
        let mut final_return = function.return_expression.as_ref();
        for (index, guard) in function.guards.iter().enumerate() {
            if index + 1 == function.guards.len() {
                if let Some(value) = final_return {
                    let exit = self.exit_label;
                    if self.select(
                        &guard.condition,
                        Hoisted::Return(guard.value.clone()),
                        Hoisted::Return(value.clone()),
                        exit,
                    )? {
                        final_return = None;
                        continue;
                    }
                }
            }
            let skip = self.new_label();
            self.branch_unless(&guard.condition, skip)?;
            self.return_value(&guard.value)?;
            self.branch(Instruction::Branch { target: 0 }, self.exit_label)?;
            self.place_label(skip);
        }
        if let Some(value) = final_return {
            if let Expression::Conditional { condition, when_true, when_false, .. } = value {
                let exit = self.exit_label;
                let then_arm = Hoisted::Return(when_true.as_ref().clone());
                let else_arm = Hoisted::Return(when_false.as_ref().clone());
                if !self.select(condition, then_arm, else_arm, exit)? {
                    let otherwise = self.new_label();
                    self.branch_unless(condition, otherwise)?;
                    self.return_value(when_true)?;
                    self.branch(Instruction::Branch { target: 0 }, exit)?;
                    self.place_label(otherwise);
                    self.return_value(when_false)?;
                }
            } else {
                self.return_value(value)?;
            }
        }
        // The exit block: the epilogue is generated here after coloring.
        let last = self.current_block();
        let falls_through = !self.block_ends_in_jump(last);
        let exit = if self.pcode.blocks[last].instructions.is_empty() && !self.block_is_target(last) {
            last
        } else {
            self.start_block(falls_through)
        };
        self.labels[self.exit_label.0] = Some(exit);
        if let Some(register) = self.return_register {
            // The copy to r3 joins all returns; the (empty) return block that
            // receives the epilogue follows it. A copy inside the return block
            // itself would be dead: the result is a use *of* that block.
            self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
            self.start_block(true);
        }
        self.resolve_branches()?;
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Assign { name, value } => self.assign(name, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.call(name, arguments, None).map(|_| ())
            }
            Statement::Store { target, value } => self.store(target, value),
            Statement::If { condition, then_body, else_body } => {
                match (then_body.as_slice(), else_body.as_slice()) {
                    (
                        [Statement::Assign { name, value }],
                        [Statement::Assign { name: other, value: other_value }],
                    ) if name == other && self.variables.contains_key(name) => {
                        let join = self.new_label();
                        if self.select(
                            condition,
                            Hoisted::Assign(name.clone(), value.clone()),
                            Hoisted::Assign(name.clone(), other_value.clone()),
                            join,
                        )? {
                            self.place_label(join);
                            return Ok(());
                        }
                    }
                    ([Statement::Return(Some(value))], [Statement::Return(Some(other_value))])
                        if self.return_register.is_some() =>
                    {
                        let exit = self.exit_label;
                        if self.select(
                            condition,
                            Hoisted::Return(value.clone()),
                            Hoisted::Return(other_value.clone()),
                            exit,
                        )? {
                            return self.branch(Instruction::Branch { target: 0 }, exit);
                        }
                    }
                    _ => {}
                }
                let otherwise = self.new_label();
                self.branch_unless(condition, otherwise)?;
                for statement in then_body {
                    self.statement(statement)?;
                }
                if else_body.is_empty() {
                    self.place_label(otherwise);
                } else {
                    let join = self.new_label();
                    if !self.block_ends_in_jump(self.current_block()) {
                        self.branch(Instruction::Branch { target: 0 }, join)?;
                    }
                    self.place_label(otherwise);
                    for statement in else_body {
                        self.statement(statement)?;
                    }
                    self.place_label(join);
                }
                Ok(())
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    self.return_value(value)?;
                }
                let exit = self.exit_label;
                self.branch(Instruction::Branch { target: 0 }, exit)?;
                Ok(())
            }
            other => Err(unsupported(format!("statement {:?}", std::mem::discriminant(other)))),
        }
    }

    fn return_value(&mut self, value: &Expression) -> Compilation<()> {
        if let Some(destination) = self.return_register {
            self.target = Some(destination);
            let evaluated = self.expression(value);
            self.target = None;
            let (register, ty) = evaluated?;
            let (register, _) = self.convert(register, ty, self.return_type, Some(destination))?;
            if register != destination {
                self.emit_plain(Instruction::Or { a: destination, s: register, b: register });
            }
            return Ok(());
        }
        let (register, ty) = self.expression(value)?;
        let (register, _) = self.convert(register, ty, self.return_type, None)?;
        self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
        Ok(())
    }

    fn assign(&mut self, name: &str, value: &Expression) -> Compilation<()> {
        let Some(variable) = self.variables.get(name) else {
            return Err(unsupported(format!("assignment to non-local '{name}'")));
        };
        let destination = variable.register;
        let result = self.assign_to(name, destination, value);
        // A cached global value held in this variable's register is stale
        // once the variable changes (the new value may itself be cached).
        let _ = name;
        result
    }

    fn assign_to(&mut self, name: &str, destination: u32, value: &Expression) -> Compilation<()> {
        let before: Vec<String> = self
            .loaded_globals
            .iter()
            .filter(|(_, (register, _))| *register == destination)
            .map(|(global, _)| global.clone())
            .collect();
        for global in before {
            self.loaded_globals.remove(&global);
        }
        let Some(variable) = self.variables.get(name) else {
            return Err(unsupported(format!("assignment to non-local '{name}'")));
        };
        let variable_type = variable.ty;
        let narrow = matches!(
            variable_type,
            Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort
        );
        self.target = if narrow { None } else { Some(destination) };
        let evaluated = self.expression(value);
        self.target = None;
        let (source, source_type) = evaluated?;
        if narrow && source_type != variable_type {
            let (converted, _) = self.convert(source, source_type, variable_type, Some(destination))?;
            if converted == destination {
                return Ok(());
            }
            self.emit_plain(Instruction::Or { a: destination, s: converted, b: converted });
            return Ok(());
        }
        if source != destination {
            self.emit_plain(Instruction::Or { a: destination, s: source, b: source });
        }
        Ok(())
    }

    /// Evaluate a word-sized integer/pointer expression into a register.
    fn expression(&mut self, expression: &Expression) -> Compilation<(u32, Type)> {
        let target = self.target.take();
        match expression {
            Expression::IntegerLiteral(value) => {
                let register = self.result(target);
                self.load_constant(register, *value)?;
                Ok((register, Type::Int))
            }
            Expression::Variable(name) => {
                if let Some(variable) = self.variables.get(name) {
                    let (register, ty) = (variable.register, variable.ty);
                    if variable.raw_narrow {
                        if let Some(&extended) = self.extended.get(name) {
                            return Ok((extended, ty));
                        }
                        let extended = self.temporary();
                        self.emit_plain(extension(ty, extended, register));
                        self.extended.insert(name.clone(), extended);
                        return Ok((extended, ty));
                    }
                    return Ok((register, ty));
                }
                if let Some(global) = self.context.globals.get(name).copied() {
                    if global.is_array {
                        return Err(unsupported("array global as a value"));
                    }
                    if !global.is_volatile {
                        if let Some(&(register, ty)) = self.loaded_globals.get(name) {
                            return Ok((register, ty));
                        }
                    }
                    let loaded = self.load_global(name, global, target)?;
                    if !global.is_volatile {
                        self.loaded_globals.insert(name.clone(), loaded);
                    }
                    return Ok(loaded);
                }
                Err(unsupported(format!("unknown variable '{name}'")))
            }
            Expression::Binary { operator, left, right }
                if literal_value(left).is_some() && literal_value(right).is_some() =>
            {
                let folded = fold_literals(*operator, literal_value(left).unwrap(), literal_value(right).unwrap());
                let Some(value) = folded else {
                    return Err(unsupported("constant expression"));
                };
                let register = self.result(target);
                self.load_constant(register, value)?;
                Ok((register, Type::Int))
            }
            Expression::Binary { operator, left, right } if negation_fold(*operator, left, right).is_some() => {
                let folded = negation_fold(*operator, left, right).expect("checked");
                self.expression_with_target(&folded, target)
            }
            Expression::Binary { operator, left, right } => {
                match fold_constants(*operator, left, right) {
                    Some(Expression::Binary { operator, left, right }) => {
                        self.binary(operator, &left, &right, target)
                    }
                    _ => self.binary(*operator, left, right, target),
                }
            }
            Expression::Conditional { condition, when_true, when_false, .. } => {
                let Some(idiom) = self.signed_idiom(condition, when_true, when_false) else {
                    return Err(unsupported("conditional expression"));
                };
                self.sign_select(idiom, target)
            }
            Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => {
                let inverted = match operand.as_ref() {
                    Expression::Binary { operator, left, right } if comparison(*operator).is_some() => {
                        Expression::Binary { operator: invert(*operator), left: left.clone(), right: right.clone() }
                    }
                    other => Expression::Binary {
                        operator: BinaryOperator::Equal,
                        left: Box::new(other.clone()),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    },
                };
                self.expression_with_target(&inverted, target)
            }
            Expression::Unary { operator, operand } => {
                let (source, ty) = self.expression(operand)?;
                let destination = self.result(target);
                match operator {
                    UnaryOperator::Negate => {
                        self.emit_plain(Instruction::Negate { d: destination, a: source })
                    }
                    UnaryOperator::BitNot => {
                        self.emit_plain(Instruction::Nor { a: destination, s: source, b: source })
                    }
                    UnaryOperator::LogicalNot => return Err(unsupported("logical not")),
                }
                Ok((destination, promote(ty)))
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let (base, _) = self.expression(base)?;
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                self.load(*member_type, base, offset, None, target)
            }
            Expression::Index { base, index } => {
                if let Some(loaded) = self.element_load(base, index, target)? {
                    return Ok(loaded);
                }
                Err(unsupported("index of a non-pointer"))
            }
            Expression::Dereference { pointer } => {
                if let Expression::Binary { operator: BinaryOperator::Add, left, right } = pointer.as_ref() {
                    let (base, index) = if self.static_type(left).and_then(element_size).is_some() {
                        (left, right)
                    } else {
                        (right, left)
                    };
                    if let Some(loaded) = self.element_load(base, index, target)? {
                        return Ok(loaded);
                    }
                }
                let (base, ty) = self.expression(pointer)?;
                let Type::Pointer(pointee) = ty else {
                    return Err(unsupported("dereference of a non-scalar pointer"));
                };
                let loaded = pointee_type(pointee).ok_or_else(|| unsupported("pointee type"))?;
                self.load(loaded, base, 0, None, target)
            }
            Expression::Call { name, arguments } => {
                let ty = self.context.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_general_word(ty) {
                    return Err(unsupported("non-integer call result"));
                }
                let register = self.call(name, arguments, target)?;
                Ok((register, ty))
            }
            Expression::Cast { target_type, operand } if is_general_word(*target_type) => {
                let (source, source_type) = self.expression(operand)?;
                self.convert(source, source_type, *target_type, target)
            }
            _ => Err(unsupported("expression form")),
        }
    }

    fn convert(
        &mut self,
        source: u32,
        from: Type,
        to: Type,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        let narrow = match to {
            Type::Char => Some(Instruction::ExtendSignByte { a: 0, s: source }),
            Type::Short => Some(Instruction::ExtendSignHalfword { a: 0, s: source }),
            Type::UnsignedChar => Some(Instruction::ClearLeftImmediate { a: 0, s: source, clear: 24 }),
            Type::UnsignedShort => Some(Instruction::ClearLeftImmediate { a: 0, s: source, clear: 16 }),
            _ => None,
        };
        if from == to || narrow.is_none() {
            return Ok((source, to));
        }
        let destination = self.result(target);
        let instruction = match narrow.expect("checked") {
            Instruction::ExtendSignByte { s, .. } => Instruction::ExtendSignByte { a: destination, s },
            Instruction::ExtendSignHalfword { s, .. } => Instruction::ExtendSignHalfword { a: destination, s },
            Instruction::ClearLeftImmediate { s, clear, .. } => {
                Instruction::ClearLeftImmediate { a: destination, s, clear }
            }
            other => other,
        };
        self.emit_plain(instruction);
        Ok((destination, to))
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

    /// `base[index]` / `*(base + index)` for a pointer `base`: a displacement
    /// load for a constant index, else an indexed (`lwzx`) load of the
    /// scaled index. `None` when `base` is not a scalar pointer.
    fn element_load(
        &mut self,
        base: &Expression,
        index: &Expression,
        target: Option<u32>,
    ) -> Compilation<Option<(u32, Type)>> {
        let Some(pointer_type @ Type::Pointer(pointee)) = self.static_type(base) else {
            return Ok(None);
        };
        if self.static_type(index).and_then(element_size).is_some() {
            return Ok(None);
        }
        let (Some(loaded), Some(size)) = (pointee_type(pointee), element_size(pointer_type)) else {
            return Ok(None);
        };
        if let Expression::IntegerLiteral(value) = index {
            let Ok(offset) = i16::try_from(value * i64::from(size)) else { return Ok(None) };
            let (a, _) = self.expression(base)?;
            return self.load(loaded, a, offset, None, target).map(Some);
        }
        let (a, _) = self.expression(base)?;
        let scaled = scale_index(index, size);
        let (b, _) = self.expression(&scaled)?;
        let extends = loaded == Type::Char;
        let d = if extends { self.temporary() } else { self.result(target) };
        let instruction = match loaded {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) => Instruction::LoadWordIndexed { d, a, b },
            Type::Short => Instruction::LoadHalfwordAlgebraicIndexed { d, a, b },
            Type::UnsignedShort => Instruction::LoadHalfwordZeroIndexed { d, a, b },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZeroIndexed { d, a, b },
            _ => return Ok(None),
        };
        let mut load = PInstr::new(instruction);
        load.not_r0.push(a);
        self.emit(load);
        if extends {
            let extended = self.result(target);
            self.emit_plain(Instruction::ExtendSignByte { a: extended, s: d });
            return Ok(Some((extended, loaded)));
        }
        Ok(Some((d, loaded)))
    }

    fn load_instruction(ty: Type, d: u32, a: u32, offset: i16) -> Compilation<(Instruction, Option<Instruction>)> {
        Ok(match ty {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                (Instruction::LoadWord { d, a, offset }, None)
            }
            Type::Short => (Instruction::LoadHalfwordAlgebraic { d, a, offset }, None),
            Type::UnsignedShort => (Instruction::LoadHalfwordZero { d, a, offset }, None),
            Type::UnsignedChar => (Instruction::LoadByteZero { d, a, offset }, None),
            Type::Char => (
                Instruction::LoadByteZero { d, a, offset },
                Some(Instruction::ExtendSignByte { a: d, s: d }),
            ),
            other => return Err(unsupported(format!("load of {other:?}"))),
        })
    }

    fn load(
        &mut self,
        ty: Type,
        base: u32,
        offset: i16,
        relocation: Option<AttachedRelocation>,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        let extends = matches!(ty, Type::Char);
        let destination = if extends { self.temporary() } else { self.result(target) };
        let (load, extend) = Self::load_instruction(ty, destination, base, offset)?;
        let mut instruction = PInstr::new(load);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        if let Some(extend) = extend {
            let extended = self.result(target);
            let extend = match extend {
                Instruction::ExtendSignByte { .. } => Instruction::ExtendSignByte { a: extended, s: destination },
                other => other,
            };
            self.emit_plain(extend);
            return Ok((extended, ty));
        }
        Ok((destination, ty))
    }

    fn load_global(
        &mut self,
        name: &str,
        global: GlobalInfo,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        if global.small_data {
            return self.load(
                global.ty,
                0,
                0,
                Some(AttachedRelocation {
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::External(name.to_owned()),
                }),
                target,
            );
        }
        let high = self.temporary();
        let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
        lis.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(name.to_owned()),
        });
        self.emit(lis);
        self.load(
            global.ty,
            high,
            0,
            Some(AttachedRelocation {
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External(name.to_owned()),
            }),
            target,
        )
    }

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        if comparison(operator).is_some() {
            return self.comparison_value(operator, left, right, target);
        }
        if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
            let left_pointer = self.static_type(left).and_then(element_size);
            let right_pointer = self.static_type(right).and_then(element_size);
            match (left_pointer, right_pointer) {
                (Some(_), Some(_)) => return Err(unsupported("pointer difference")),
                (Some(0), None) | (None, Some(0)) => {
                    return Err(unsupported("arithmetic on an unsized pointee"))
                }
                (Some(size), None) if size != 1 => {
                    let scaled = scale_index(right, size);
                    return self.binary_scaled(operator, left, &scaled, target);
                }
                (None, Some(size)) if size != 1 && operator == BinaryOperator::Add => {
                    let scaled = scale_index(left, size);
                    return self.binary_scaled(operator, &scaled, right, target);
                }
                (None, Some(_)) if operator == BinaryOperator::Subtract => {
                    return Err(unsupported("integer minus pointer"))
                }
                _ => {}
            }
        }
        self.binary_scaled(operator, left, right, target)
    }

    /// `binary` once pointer operands are already in byte units.
    fn binary_scaled(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        // A constant operand goes on the right of a commutative operation so
        // it can be an immediate; `k - x` is `subfic`.
        if let (Expression::IntegerLiteral(value), false) =
            (left, matches!(right, Expression::IntegerLiteral(_)))
        {
            match operator {
                BinaryOperator::Add
                | BinaryOperator::Multiply
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor => {
                    return self.binary_scaled(operator, right, left, target);
                }
                BinaryOperator::Subtract if i16::try_from(*value).is_ok() => {
                    let (a, ty) = self.expression(right)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::SubtractFromImmediate { d, a, immediate: *value as i16 });
                    return Ok((d, promote(ty)));
                }
                _ => {}
            }
        }
        let immediate = match right {
            Expression::IntegerLiteral(value) => i16::try_from(*value).ok(),
            _ => None,
        };
        // IRO algebra: `(x + y) - y` and `(x - y) + y` are `x`.
        if let Some(simplified) = cancel(operator, left, right) {
            return self.expression_with_target(simplified, target);
        }
        // `field | base` where the base is known zero under the field's
        // mask inserts the field: `rlwimi base,x,shift,mb,me`.
        if operator == BinaryOperator::BitOr {
            if let Some((source, shift, begin, end)) = self.insert_field(left) {
                if field_bits(begin, end) & !self.known_zero(right) == 0 {
                    let (x, ty) = self.expression(source)?;
                    let (base, _) = self.expression(right)?;
                    let d = self.result(target);
                    if base != d {
                        self.emit_plain(Instruction::Or { a: d, s: base, b: base });
                    }
                    self.emit_plain(Instruction::RotateAndMaskInsert { a: d, s: x, shift, begin, end });
                    return Ok((d, promote(ty)));
                }
            }
        }
        // `a & ~b` / `a | ~b` are single instructions.
        if let (BinaryOperator::BitAnd | BinaryOperator::BitOr, Expression::Unary { operator: UnaryOperator::BitNot, operand }) =
            (operator, right)
        {
            let (a, ty) = self.expression(left)?;
            let (b, _) = self.expression(operand)?;
            let d = self.result(target);
            self.emit_plain(if operator == BinaryOperator::BitAnd {
                Instruction::AndComplement { a: d, s: a, b }
            } else {
                Instruction::OrComplement { a: d, s: a, b }
            });
            return Ok((d, promote(ty)));
        }
        // A mask within a narrow load's width does not observe its sign
        // extension: load zero-extended and mask.
        if operator == BinaryOperator::BitAnd {
            if let (Some((begin, end)), Some(width)) = (mask_bounds(right), narrow_load_width(left)) {
                if begin >= 32 - width {
                    let widened = zero_extended_load(left);
                    let (a, _) = self.expression(&widened)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                    return Ok((d, Type::Int));
                }
            }
        }
        if operator == BinaryOperator::BitAnd {
            if let Some((operand, shift, begin, end)) = shift_mask(left, right) {
                let (a, operand_type) = self.expression(operand)?;
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift, begin, end });
                return Ok((d, promote(operand_type)));
            }
        }
        // `x * 0` folds to 0 (IRO) when `x` has no effects.
        if operator == BinaryOperator::Multiply && immediate == Some(0) && speculable(left) {
            let d = self.result(target);
            self.load_constant(d, 0)?;
            let ty = self.static_type(left).map_or(Type::Int, promote);
            return Ok((d, ty));
        }
        // A raw unsigned narrow parameter shifted right folds its extension.
        let raw_unsigned = match left {
            Expression::Variable(name) if operator == BinaryOperator::ShiftRight => {
                self.variables.get(name).filter(|variable| variable.raw_narrow).and_then(|variable| {
                    let width = match variable.ty {
                        Type::UnsignedChar => 8u8,
                        Type::UnsignedShort => 16u8,
                        _ => return None,
                    };
                    immediate
                        .filter(|&shift| (0..i16::from(width)).contains(&shift))
                        .map(|_| (variable.register, width))
                })
            }
            _ => None,
        };
        let (a, left_type) = if raw_unsigned.is_some() {
            (0, self.static_type(left).expect("a variable"))
        } else {
            self.expression(left)?
        };
        let result_type = promote(left_type);
        match (operator, immediate) {
            // `x * -2^k` is a shift and a negation (`neg` alone for -1).
            (BinaryOperator::Multiply, Some(value))
                if value < 0 && value != i16::MIN && (-value as u16).is_power_of_two() =>
            {
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
                return Ok((d, result_type));
            }
            (BinaryOperator::Add, Some(value)) => {
                let d = self.result(target);
                self.emit_based(Instruction::AddImmediate { d, a, immediate: value }, a);
                return Ok((d, result_type));
            }
            (BinaryOperator::Add, None) if wide_constant(right).is_some() => {
                let (high, low) = wide_constant(right).expect("checked");
                let d = self.result(target);
                let middle = if low == 0 { d } else { self.temporary() };
                self.emit_based(Instruction::AddImmediateShifted { d: middle, a, immediate: high }, a);
                if low != 0 {
                    self.emit_based(Instruction::AddImmediate { d, a: middle, immediate: low }, middle);
                }
                return Ok((d, result_type));
            }
            (BinaryOperator::Subtract, Some(value)) if value != i16::MIN => {
                let d = self.result(target);
                self.emit_based(Instruction::AddImmediate { d, a, immediate: -value }, a);
                return Ok((d, result_type));
            }
            (BinaryOperator::Multiply, Some(value)) if value > 0 && (value as u16).is_power_of_two() => {
                let d = self.result(target);
                let shift = (value as u16).trailing_zeros() as u8;
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift });
                return Ok((d, result_type));
            }
            (BinaryOperator::Multiply, Some(value)) => {
                let d = self.result(target);
                self.emit_plain(Instruction::MultiplyImmediate { d, a, immediate: value });
                return Ok((d, result_type));
            }
            (BinaryOperator::BitOr | BinaryOperator::BitXor, _) if unsigned_immediate(right).is_some() => {
                let (value, shifted) = unsigned_immediate(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(match (operator, shifted) {
                    (BinaryOperator::BitOr, false) => Instruction::OrImmediate { a: d, s: a, immediate: value },
                    (BinaryOperator::BitOr, true) => Instruction::OrImmediateShifted { a: d, s: a, immediate: value },
                    (_, false) => Instruction::XorImmediate { a: d, s: a, immediate: value },
                    (_, true) => Instruction::XorImmediateShifted { a: d, s: a, immediate: value },
                });
                return Ok((d, result_type));
            }
            (BinaryOperator::BitAnd, _) if mask_bounds(right).is_some() => {
                let (begin, end) = mask_bounds(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftLeft, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.result(target);
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift: shift as u8 });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftRight, Some(shift)) if (0..32).contains(&shift) && raw_unsigned.is_some() => {
                let (raw, width) = raw_unsigned.expect("checked");
                let d = self.result(target);
                let shift = shift as u8;
                self.emit_plain(Instruction::RotateAndMask {
                    a: d,
                    s: raw,
                    shift: (32 - shift) % 32,
                    begin: 32 - width + shift,
                    end: 31,
                });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftRight, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.result(target);
                let shift = shift as u8;
                self.emit_plain(if is_unsigned(result_type) {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: a, shift }
                } else {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: a, shift }
                });
                return Ok((d, result_type));
            }
            _ => {}
        }
        let (b, right_type) = self.expression(right)?;
        let result_type = if is_unsigned(result_type) || is_unsigned(promote(right_type)) {
            if matches!(result_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                result_type
            } else {
                Type::UnsignedInt
            }
        } else {
            result_type
        };
        // MWCC places a leaf operand first in a commutative operation whose
        // other operand is computed (`a*b + c` -> `add r3,c,t`).
        let commutative = matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Multiply
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor
        );
        let (a, b) = if commutative && !self.is_register_leaf(left) && self.is_register_leaf(right) {
            (b, a)
        } else {
            (a, b)
        };
        let d = self.result(target);
        let instruction = match operator {
            BinaryOperator::Add => Instruction::Add { d, a, b },
            BinaryOperator::Subtract => Instruction::SubtractFrom { d, a: b, b: a },
            BinaryOperator::Multiply => Instruction::MultiplyLow { d, a, b },
            BinaryOperator::BitAnd => Instruction::And { a: d, s: a, b },
            BinaryOperator::BitOr => Instruction::Or { a: d, s: a, b },
            BinaryOperator::BitXor => Instruction::Xor { a: d, s: a, b },
            BinaryOperator::ShiftLeft => Instruction::ShiftLeftWord { a: d, s: a, b },
            BinaryOperator::ShiftRight if is_unsigned(result_type) => {
                Instruction::ShiftRightWord { a: d, s: a, b }
            }
            BinaryOperator::ShiftRight => Instruction::ShiftRightAlgebraicWord { a: d, s: a, b },
            other => return Err(unsupported(format!("binary operator {other:?}"))),
        };
        self.emit_plain(instruction);
        Ok((d, result_type))
    }

    fn store(&mut self, target: &Expression, value: &Expression) -> Compilation<()> {
        let width = store_width(target, self.context.globals);
        let value = strip_narrowing_for_store(value, width);
        let (source, _) = self.expression(value)?;
        let (base, offset, ty, relocation) = match target {
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let (base, _) = self.expression(base)?;
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                (base, offset, *member_type, None)
            }
            Expression::Dereference { pointer } => {
                let (base, ty) = self.expression(pointer)?;
                let Type::Pointer(pointee) = ty else {
                    return Err(unsupported("store through a non-scalar pointer"));
                };
                (base, 0, pointee_type(pointee).ok_or_else(|| unsupported("pointee type"))?, None)
            }
            Expression::Variable(name) if self.context.globals.contains_key(name) => {
                let global = self.context.globals[name];
                if global.is_array {
                    return Err(unsupported("store to an array global"));
                }
                if !global.small_data {
                    return Err(unsupported("absolute global store"));
                }
                (
                    0,
                    0,
                    global.ty,
                    Some(AttachedRelocation {
                        kind: RelocationKind::EmbSda21,
                        target: RelocationTarget::External(name.clone()),
                    }),
                )
            }
            _ => return Err(unsupported("store target")),
        };
        let store = match ty {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                Instruction::StoreWord { s: source, a: base, offset }
            }
            Type::Short | Type::UnsignedShort => Instruction::StoreHalfword { s: source, a: base, offset },
            Type::Char | Type::UnsignedChar => Instruction::StoreByte { s: source, a: base, offset },
            other => return Err(unsupported(format!("store of {other:?}"))),
        };
        let mut instruction = PInstr::new(store);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        self.loaded_globals.clear();
        // A stored word-sized global's value is still in `source`.
        if let Expression::Variable(name) = target {
            if let Some(global) = self.context.globals.get(name) {
                if !global.is_volatile && matches!(ty, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }) {
                    self.loaded_globals.insert(name.clone(), (source, ty));
                }
            }
        }
        Ok(())
    }

    fn call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        target: Option<u32>,
    ) -> Compilation<u32> {
        if (self.context.is_intrinsic)(name, arguments.len()) {
            return Err(unsupported(format!("intrinsic '{name}'")));
        }
        if self.context.variadic_callees.contains(name) {
            return Err(unsupported("call to a variadic function"));
        }
        if !self.context.prototyped.contains(name) {
            return Err(unsupported("call without a prototype"));
        }
        if self.context.call_parameter_types.get(name).is_some_and(|types| {
            types.iter().any(|ty| {
                matches!(ty, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
            })
        }) {
            return Err(unsupported("call with narrow parameters"));
        }
        if arguments.len() > (LAST_GENERAL_ARGUMENT - FIRST_GENERAL_ARGUMENT + 1) as usize {
            return Err(unsupported("stack-passed arguments"));
        }
        let mut values = Vec::new();
        for argument in arguments {
            let (register, ty) = self.expression(argument)?;
            if !is_general_word(ty) {
                return Err(unsupported("non-integer argument"));
            }
            values.push(register);
        }
        let mut argument_registers = Vec::new();
        for (index, value) in values.into_iter().enumerate() {
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            self.emit_plain(Instruction::Or { a: register, s: value, b: value });
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
        let result = self.result(target);
        self.emit_plain(Instruction::Or { a: result, s: 3, b: 3 });
        Ok(result)
    }
}

/// `x | k` / `x ^ k` immediates: a low halfword (`ori`) or a high halfword
/// with a zero low half (`oris`).
fn unsigned_immediate(expression: &Expression) -> Option<(u16, bool)> {
    let Expression::IntegerLiteral(value) = expression else { return None };
    let value = u32::try_from(*value).ok().or_else(|| i32::try_from(*value).ok().map(|v| v as u32))?;
    if value <= 0xFFFF {
        Some((value as u16, false))
    } else if value & 0xFFFF == 0 {
        Some(((value >> 16) as u16, true))
    } else {
        None
    }
}

/// A contiguous (possibly wrapping) mask `x & k` as `rlwinm` bounds.
fn mask_bounds(expression: &Expression) -> Option<(u8, u8)> {
    let Expression::IntegerLiteral(value) = expression else { return None };
    let mask = u32::try_from(*value).ok().or_else(|| i32::try_from(*value).ok().map(|v| v as u32))?;
    if mask == 0 || mask == u32::MAX {
        return None;
    }
    // MB..ME in IBM bit numbering (bit 0 = MSB). A non-wrapping run.
    let leading = mask.leading_zeros();
    let trailing = mask.trailing_zeros();
    if (mask >> trailing).count_ones() == 32 - leading - trailing {
        return Some((leading as u8, (31 - trailing) as u8));
    }
    None
}

/// IRO constant folding of a constant operation applied to another constant
/// operation of the same kind: `(x >> 2) >> 3` -> `x >> 5`,
/// `(x + 3) + 5` -> `x + 8`, `x + 10 - 3` -> `x + 7`, `(x | 5) | 2` -> `x | 7`.
fn fold_constants(
    operator: BinaryOperator,
    left: &Expression,
    right: &Expression,
) -> Option<Expression> {
    let Expression::IntegerLiteral(outer) = right else { return None };
    let Expression::Binary { operator: inner_operator, left: inner_left, right: inner_right } = left
    else {
        return None;
    };
    let Expression::IntegerLiteral(inner) = inner_right.as_ref() else { return None };
    use BinaryOperator::*;
    let (operator, value) = match (*inner_operator, operator) {
        (ShiftRight, ShiftRight) | (ShiftLeft, ShiftLeft) if inner + outer < 32 => {
            (operator, inner + outer)
        }
        (Add, Add) => (Add, inner + outer),
        (Add, Subtract) => (Add, inner - outer),
        (Subtract, Add) => (Add, outer - inner),
        (Subtract, Subtract) => (Subtract, inner + outer),
        (BitOr, BitOr) => (BitOr, inner | outer),
        (BitXor, BitXor) => (BitXor, inner ^ outer),
        (BitAnd, BitAnd) => (BitAnd, inner & outer),
        (Multiply, Multiply) => (Multiply, inner * outer),
        _ => return None,
    };
    let folded = Expression::Binary {
        operator,
        left: inner_left.clone(),
        right: Box::new(Expression::IntegerLiteral(value)),
    };
    // Fold repeatedly (`a + 1 + 2 + 3`).
    if let Expression::Binary { operator, left, right } = &folded {
        if let Some(again) = fold_constants(*operator, left, right) {
            return Some(again);
        }
    }
    Some(folded)
}

/// `(x >> s) & m` / `(x << s) & m` as one `rlwinm x, rotate, mb, me` when the
/// mask keeps only bits the shift defines: `(operand, rotate, mb, me)`.
fn shift_mask<'a>(left: &'a Expression, right: &Expression) -> Option<(&'a Expression, u8, u8, u8)> {
    let (begin, end) = mask_bounds(right)?;
    let Expression::IntegerLiteral(mask) = right else { return None };
    let mask = *mask as u32;
    let Expression::Binary { operator, left: operand, right: amount } = left else { return None };
    let Expression::IntegerLiteral(amount) = amount.as_ref() else { return None };
    let amount = u32::try_from(*amount).ok().filter(|amount| (1..32).contains(amount))?;
    match operator {
        BinaryOperator::ShiftRight if mask <= (u32::MAX >> amount) => {
            Some((operand, (32 - amount) as u8, begin, end))
        }
        BinaryOperator::ShiftLeft if mask & ((1 << amount) - 1) == 0 => {
            Some((operand, amount as u8, begin, end))
        }
        _ => None,
    }
}

/// Byte width of a store target, when known.
fn store_width(target: &Expression, globals: &HashMap<String, GlobalInfo>) -> Option<u32> {
    let ty = match target {
        Expression::Member { member_type, .. } => *member_type,
        Expression::Dereference { pointer } => match pointer.as_ref() {
            Expression::Cast { target_type: Type::Pointer(pointee), .. } => pointee_type(*pointee)?,
            _ => return None,
        },
        Expression::Variable(name) => globals.get(name)?.ty,
        _ => return None,
    };
    Some(match ty {
        Type::Char | Type::UnsignedChar => 1,
        Type::Short | Type::UnsignedShort => 2,
        _ => 4,
    })
}

/// A store keeps only its low `width` bytes, so integer conversions to types
/// at least that wide before it are dead.
fn strip_narrowing_for_store(value: &Expression, width: Option<u32>) -> &Expression {
    let Some(width) = width else { return value };
    let mut value = value;
    while let Expression::Cast { target_type, operand } = value {
        let cast_width = match target_type {
            Type::Char | Type::UnsignedChar => 1,
            Type::Short | Type::UnsignedShort => 2,
            Type::Int | Type::UnsignedInt => 4,
            _ => return value,
        };
        if cast_width < width {
            return value;
        }
        value = operand;
    }
    value
}

/// The byte size of a pointer operand's element, `Some(0)` when unknown
/// (opaque struct or function pointer), `None` for a non-pointer.
fn element_size(ty: Type) -> Option<u32> {
    match ty {
        Type::Pointer(pointee) => Some(match pointee {
            Pointee::Char | Pointee::UnsignedChar => 1,
            Pointee::Short | Pointee::UnsignedShort => 2,
            Pointee::Int
            | Pointee::UnsignedInt
            | Pointee::Float
            | Pointee::Pointer
            | Pointee::WordPointer => 4,
            Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong => 8,
        }),
        Type::StructPointer { element_size } => Some(element_size),
        _ => None,
    }
}

/// `index * size` for pointer arithmetic, folded when the index is constant.
fn scale_index(index: &Expression, size: u32) -> Expression {
    match index {
        Expression::IntegerLiteral(value) => Expression::IntegerLiteral(value * i64::from(size)),
        other => Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(other.clone()),
            right: Box::new(Expression::IntegerLiteral(i64::from(size))),
        },
    }
}

/// `(x + y) - y` / `(x - y) + y` -> `x` (matching leaf operands only).
fn cancel<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &Expression,
) -> Option<&'a Expression> {
    let Expression::Binary { operator: inner, left: x, right: y } = left else { return None };
    let leaf = |expression: &Expression| {
        matches!(expression, Expression::Variable(_) | Expression::IntegerLiteral(_))
    };
    let inverse = matches!(
        (inner, operator),
        (BinaryOperator::Add, BinaryOperator::Subtract) | (BinaryOperator::Subtract, BinaryOperator::Add)
    );
    let same = match (y.as_ref(), right) {
        (Expression::Variable(a), Expression::Variable(b)) => a == b,
        (Expression::IntegerLiteral(a), Expression::IntegerLiteral(b)) => a == b,
        _ => false,
    };
    (inverse && leaf(y) && same).then_some(x.as_ref())
}

/// A constant that does not fit `addi`: `(high, low)` for `addis`+`addi`.
fn wide_constant(expression: &Expression) -> Option<(i16, i16)> {
    let Expression::IntegerLiteral(value) = expression else { return None };
    let value = i32::try_from(*value).ok().or_else(|| u32::try_from(*value).ok().map(|v| v as i32))?;
    if i16::try_from(value).is_ok() {
        return None;
    }
    let low = value as i16;
    let high = ((value - i32::from(low)) >> 16) as i16;
    Some((high, low))
}

/// Bit width of a signed narrow memory load (`char`/`short` member or
/// dereference), whose sign extension a narrow mask never observes.
fn narrow_load_width(expression: &Expression) -> Option<u8> {
    match expression {
        Expression::Member { member_type: Type::Char, index_stride: None, .. } => Some(8),
        Expression::Member { member_type: Type::Short, index_stride: None, .. } => Some(16),
        _ => None,
    }
}

/// The same load, zero-extending.
fn zero_extended_load(expression: &Expression) -> Expression {
    match expression {
        Expression::Member { base, offset, member_type, index_stride } => Expression::Member {
            base: base.clone(),
            offset: *offset,
            member_type: match member_type {
                Type::Char => Type::UnsignedChar,
                Type::Short => Type::UnsignedShort,
                other => *other,
            },
            index_stride: *index_stride,
        },
        other => other.clone(),
    }
}

/// For a comparison operator: the cr0 bit it tests and whether the relation
/// holds when the bit is set (`<`: LT set; `>=`: LT clear).
fn comparison(operator: BinaryOperator) -> Option<(u8, bool)> {
    Some(match operator {
        BinaryOperator::Less => (0, true),
        BinaryOperator::GreaterEqual => (0, false),
        BinaryOperator::Greater => (1, true),
        BinaryOperator::LessEqual => (1, false),
        BinaryOperator::Equal => (2, true),
        BinaryOperator::NotEqual => (2, false),
        _ => return None,
    })
}

/// `a op b` == `b mirror(op) a`.
fn mirror(operator: BinaryOperator) -> BinaryOperator {
    match operator {
        BinaryOperator::Less => BinaryOperator::Greater,
        BinaryOperator::Greater => BinaryOperator::Less,
        BinaryOperator::LessEqual => BinaryOperator::GreaterEqual,
        BinaryOperator::GreaterEqual => BinaryOperator::LessEqual,
        other => other,
    }
}

/// Whether the function returns from more than its final expression.
fn returns_from_several_places(function: &Function) -> bool {
    fn has_return(statements: &[Statement]) -> bool {
        statements.iter().any(|statement| match statement {
            Statement::Return(_) => true,
            Statement::If { then_body, else_body, .. } => has_return(then_body) || has_return(else_body),
            Statement::Loop { body, .. } => has_return(body),
            _ => false,
        })
    }
    !function.guards.is_empty()
        || has_return(&function.statements)
        || matches!(function.return_expression, Some(Expression::Conditional { .. }))
}

/// A branch condition MWCC turns into a single compare-and-branch.
fn simple_condition(condition: &Expression) -> bool {
    match condition {
        Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => simple_condition(operand),
        Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, .. } => false,
        _ => true,
    }
}

/// An arm value MWCC evaluates unconditionally in a select: no calls, and
/// nothing that can trap (pointer loads, division).
fn speculable(expression: &Expression) -> bool {
    match expression {
        Expression::IntegerLiteral(_) | Expression::Variable(_) => true,
        Expression::Binary { operator, left, right } => {
            !matches!(
                operator,
                BinaryOperator::Divide
                    | BinaryOperator::Modulo
                    | BinaryOperator::LogicalAnd
                    | BinaryOperator::LogicalOr
            ) && speculable(left)
                && speculable(right)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => speculable(operand),
        _ => false,
    }
}

/// `!(a op b)` == `a invert(op) b`.
fn invert(operator: BinaryOperator) -> BinaryOperator {
    match operator {
        BinaryOperator::Less => BinaryOperator::GreaterEqual,
        BinaryOperator::GreaterEqual => BinaryOperator::Less,
        BinaryOperator::Greater => BinaryOperator::LessEqual,
        BinaryOperator::LessEqual => BinaryOperator::Greater,
        BinaryOperator::Equal => BinaryOperator::NotEqual,
        BinaryOperator::NotEqual => BinaryOperator::Equal,
        other => other,
    }
}

fn is_unsigned_narrow(ty: Type) -> bool {
    matches!(ty, Type::UnsignedChar | Type::UnsignedShort)
}

fn is_narrow(ty: Type) -> bool {
    matches!(ty, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
}

/// Sign or zero extension of a narrow value.
fn extension(ty: Type, d: u32, s: u32) -> Instruction {
    match ty {
        Type::Char => Instruction::ExtendSignByte { a: d, s },
        Type::Short => Instruction::ExtendSignHalfword { a: d, s },
        Type::UnsignedChar => Instruction::ClearLeftImmediate { a: d, s, clear: 24 },
        Type::UnsignedShort => Instruction::ClearLeftImmediate { a: d, s, clear: 16 },
        _ => Instruction::Or { a: d, s, b: s },
    }
}

/// Whether `statements` may assign `name` (conservatively true for forms
/// the scan does not walk).
fn may_assign(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => assigned == name,
        Statement::If { then_body, else_body, .. } => {
            may_assign(then_body, name) || may_assign(else_body, name)
        }
        Statement::Store { .. } | Statement::Expression(_) | Statement::Return(_) => false,
        _ => true,
    })
}

/// Branch-free selects on the sign of a value (MWCC's 2.4.x idioms).
enum SignIdiom<'a> {
    /// `x < 0 ? -x : x` and its spellings: `srawi; xor; subf`.
    Absolute(&'a Expression),
    /// `(a REL 0) ? b : 0` (`and` with the relation's mask) or
    /// `(a REL 0) ? 0 : b` (`andc`).
    Masked {
        relation: BinaryOperator,
        tested: &'a Expression,
        value: &'a Expression,
        keep_when_true: bool,
    },
}

fn sign_idiom<'a>(
    condition: &'a Expression,
    when_true: &'a Expression,
    when_false: &'a Expression,
) -> Option<SignIdiom<'a>> {
    let Expression::Binary { operator, left, right } = condition else { return None };
    let (relation, tested) = match (left.as_ref(), right.as_ref()) {
        (tested @ Expression::Variable(_), Expression::IntegerLiteral(0)) => (*operator, tested),
        (Expression::IntegerLiteral(0), tested @ Expression::Variable(_)) => (mirror(*operator), tested),
        _ => return None,
    };
    if !matches!(
        relation,
        BinaryOperator::Less | BinaryOperator::LessEqual | BinaryOperator::Greater | BinaryOperator::GreaterEqual
    ) {
        return None;
    }
    let negation_of = |expression: &Expression| {
        matches!(expression, Expression::Unary { operator: UnaryOperator::Negate, operand }
            if same_variable(operand, tested))
    };
    let negative_side = matches!(relation, BinaryOperator::Less | BinaryOperator::LessEqual);
    if negative_side && negation_of(when_true) && same_variable(when_false, tested)
        || !negative_side && same_variable(when_true, tested) && negation_of(when_false)
    {
        return Some(SignIdiom::Absolute(tested));
    }
    let leaf = |expression: &Expression| {
        matches!(expression, Expression::Variable(_) | Expression::IntegerLiteral(_))
    };
    match (when_true, when_false) {
        (value, Expression::IntegerLiteral(0)) if leaf(value) && !matches!(value, Expression::IntegerLiteral(_)) => {
            Some(SignIdiom::Masked { relation, tested, value, keep_when_true: true })
        }
        (Expression::IntegerLiteral(0), value)
            if leaf(value)
                && !matches!(value, Expression::IntegerLiteral(_))
                && matches!(relation, BinaryOperator::Less | BinaryOperator::Greater) =>
        {
            Some(SignIdiom::Masked { relation, tested, value, keep_when_true: false })
        }
        _ => None,
    }
}

fn same_variable(left: &Expression, right: &Expression) -> bool {
    matches!((left, right), (Expression::Variable(a), Expression::Variable(b)) if a == b)
}

/// An integer literal, possibly under integer casts (`0x1000UL`).
fn literal_value(expression: &Expression) -> Option<i64> {
    match expression {
        Expression::IntegerLiteral(value) => Some(*value),
        Expression::Cast { target_type, operand } if is_general_word(*target_type) && !is_narrow(*target_type) => {
            literal_value(operand)
        }
        _ => None,
    }
}

/// IRO constant folding of two literals, in 32-bit arithmetic.
fn fold_literals(operator: BinaryOperator, left: i64, right: i64) -> Option<i64> {
    let (a, b) = (left as i32, right as i32);
    let value = match operator {
        BinaryOperator::Add => a.wrapping_add(b),
        BinaryOperator::Subtract => a.wrapping_sub(b),
        BinaryOperator::Multiply => a.wrapping_mul(b),
        BinaryOperator::BitAnd => a & b,
        BinaryOperator::BitOr => a | b,
        BinaryOperator::BitXor => a ^ b,
        BinaryOperator::ShiftLeft if (0..32).contains(&b) => a.wrapping_shl(b as u32),
        _ => return None,
    };
    Some(i64::from(value))
}

/// IRO algebra on negations: `a - -b` = `a + b`, `a + -b` = `a - b`,
/// `-a + b` = `b - a`, `-a - b` = `-(a + b)`.
fn negation_fold(operator: BinaryOperator, left: &Expression, right: &Expression) -> Option<Expression> {
    let negated = |expression: &Expression| match expression {
        Expression::Unary { operator: UnaryOperator::Negate, operand } => Some(operand.as_ref().clone()),
        _ => None,
    };
    let binary = |operator, left: Expression, right: Expression| Expression::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    };
    match operator {
        BinaryOperator::Subtract => {
            if let Some(b) = negated(right) {
                return Some(binary(BinaryOperator::Add, left.clone(), b));
            }
            let a = negated(left)?;
            Some(Expression::Unary {
                operator: UnaryOperator::Negate,
                operand: Box::new(binary(BinaryOperator::Add, a, right.clone())),
            })
        }
        BinaryOperator::Add => {
            if let Some(b) = negated(right) {
                return Some(binary(BinaryOperator::Subtract, left.clone(), b));
            }
            let a = negated(left)?;
            Some(binary(BinaryOperator::Subtract, right.clone(), a))
        }
        _ => None,
    }
}

/// The bits `begin..=end` (IBM numbering, non-wrapping).
fn field_bits(begin: u8, end: u8) -> u32 {
    (u32::MAX >> begin) & (u32::MAX << (31 - end))
}
