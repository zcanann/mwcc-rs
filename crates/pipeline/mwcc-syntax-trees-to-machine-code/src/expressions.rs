//! Core integer expression evaluation and operand placement.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
use mwcc_target::Eabi;
use mwcc_versions::GlobalAddressing;
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

/// The base variable a memory load addresses through — `a` for `a[i]`, `s` for
/// `s->x`, `p` for `*p`. Used to recognize two loads that share a base register.
pub(crate) fn load_base_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Index { base, .. } | Expression::Member { base, .. } => leaf_name(base),
        Expression::Dereference { pointer } => leaf_name(pointer),
        _ => None,
    }
}

/// The displacement load for a pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`).
fn displacement_load(pointee: Pointee, d: u8, a: u8, offset: i16) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::LoadWord { d, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZero { d, a, offset },
        Pointee::Short => Instruction::LoadHalfwordAlgebraic { d, a, offset },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZero { d, a, offset },
        Pointee::Float => Instruction::LoadFloatSingle { d, a, offset },
        Pointee::Double => Instruction::LoadFloatDouble { d, a, offset },
    }
}

/// The indexed load for a pointee type (`lwzx`/`lbzx`/`lhax`/`lhzx`/`lfsx`).
fn indexed_load(pointee: Pointee, d: u8, a: u8, b: u8) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::LoadWordIndexed { d, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZeroIndexed { d, a, b },
        Pointee::Short => Instruction::LoadHalfwordAlgebraicIndexed { d, a, b },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZeroIndexed { d, a, b },
        Pointee::Float => Instruction::LoadFloatSingleIndexed { d, a, b },
        Pointee::Double => Instruction::LoadFloatDoubleIndexed { d, a, b },
    }
}

/// A scalar type as the matching [`Pointee`] (for global loads/stores).
pub(crate) fn pointee_of_type(value_type: Type) -> Option<Pointee> {
    Some(match value_type {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Float => Pointee::Float,
        // A pointer value is a 4-byte address (stored/loaded with `stw`/`lwz`).
        Type::Pointer(_) | Type::StructPointer { .. } => Pointee::UnsignedInt,
        // `double` storage (8-byte lfd/stfd) is a later stage.
        Type::Double => Pointee::Double,
        // A struct value is not a scalar pointee (it has no single load/store); neither is a
        // long long (an 8-byte register pair loaded/stored as two words).
        Type::Void | Type::Struct { .. } | Type::LongLong | Type::UnsignedLongLong => return None,
    })
}

/// The scaled-arithmetic stride for a pointer type: a struct pointer's element size
/// (so `p + n` advances by whole structs), or `None` for a scalar pointer (which
/// scales by its `pointee` size) and a non-pointer. A zero element size — an opaque
/// struct or a function pointer — yields `None` so arithmetic stays unscaled.
pub(crate) fn pointer_stride(value_type: Type) -> Option<u16> {
    match value_type {
        Type::StructPointer { element_size } if element_size > 1 => Some(element_size),
        _ => None,
    }
}

/// The displacement store for a pointee type (`stw`/`stb`/`sth`/`stfs`).
fn displacement_store(pointee: Pointee, s: u8, a: u8, offset: i16) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::StoreWord { s, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByte { s, a, offset },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfword { s, a, offset },
        Pointee::Float => Instruction::StoreFloatSingle { s, a, offset },
        Pointee::Double => Instruction::StoreFloatDouble { s, a, offset },
    }
}

/// `*(T *)0xADDR` — a dereference through a constant-address pointer cast (memory-mapped
/// hardware registers, the GX FIFO). Returns the pointee and the absolute address.
fn const_address_pointer(pointer: &Expression) -> Option<(Pointee, u32)> {
    if let Expression::Cast { target_type: Type::Pointer(pointee), operand } = pointer {
        // Integer/char/short pointees only — a float/double const-address access needs an
        // FPR destination and a separate path, so leave those to defer.
        if !matches!(pointee, Pointee::Float | Pointee::Double) {
            return constant_value(operand).map(|address| (*pointee, address as u32));
        }
    }
    None
}

/// Split a 32-bit absolute address into the `lis` high half and the displacement low half,
/// the way mwcc does: the low half is sign-interpreted (so a `lo >= 0x8000` reads back as a
/// negative displacement), and the high half is carry-adjusted to compensate. So
/// `0xCC008000` becomes `lis -13311` + displacement `-32768`.
fn split_address(address: u32) -> (i16, i16) {
    let low = address as i16;
    let high = ((address >> 16) as i16).wrapping_add(if address & 0x8000 != 0 { 1 } else { 0 });
    (high, low)
}

/// The absolute address of any constant-address pointer cast — `(T *)C`, `(struct S *)C`,
/// `(union U *)C` — used as a member base (`(*(struct S *)C).field`) where the access width
/// comes from the member, not the cast. Returns `None` for non-constant or non-pointer casts.
fn const_address_of(pointer: &Expression) -> Option<u32> {
    if let Expression::Cast { target_type, operand } = pointer {
        if matches!(target_type, Type::Pointer(_) | Type::StructPointer { .. }) {
            return constant_value(operand).map(|address| address as u32);
        }
    }
    None
}

/// The indexed store for a pointee type (`stwx`/`stbx`/`sthx`/`stfsx`).
fn indexed_store(pointee: Pointee, s: u8, a: u8, b: u8) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::StoreWordIndexed { s, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByteIndexed { s, a, b },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfwordIndexed { s, a, b },
        Pointee::Float => Instruction::StoreFloatSingleIndexed { s, a, b },
        Pointee::Double => Instruction::StoreFloatDoubleIndexed { s, a, b },
    }
}

impl Generator {

    /// Evaluate an integer expression into general register `destination`.
    /// mwcc collapses the bitwise distributive laws `(x&y)|(x&z) -> x&(y|z)`,
    /// `(x|y)&(x|z) -> x|(y&z)`, and `(x&y)^(x&z) -> x&(y^z)` to one inner op plus the
    /// outer, sharing the common factor `x`. When `x` is the first operand of the left
    /// inner node, rewrite to that form and evaluate it. A common factor in the second
    /// position (`(y&x)|(z&x)`) needs mwcc's value-first operand order, not yet modeled,
    /// so it defers rather than emit the longer un-distributed sequence.
    fn try_emit_distributive_bitwise(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        use BinaryOperator::*;
        let (Expression::Binary { operator: inner, left: la, right: lb }, Expression::Binary { operator: inner_right, left: ra, right: rb }) = (left, right) else {
            return Ok(false);
        };
        if inner != inner_right || !matches!((operator, *inner), (BitOr, BitAnd) | (BitXor, BitAnd) | (BitAnd, BitOr)) {
            return Ok(false);
        }
        let other = if same_operand(la, ra) {
            rb
        } else if same_operand(la, rb) {
            ra
        } else if same_operand(lb, ra) || same_operand(lb, rb) {
            return Err(Diagnostic::error("a distributive bitwise factor in the second operand position needs mwcc's value-first order (roadmap)"));
        } else {
            return Ok(false);
        };
        let combined = Expression::Binary { operator, left: lb.clone(), right: other.clone() };
        let rewritten = Expression::Binary { operator: *inner, left: la.clone(), right: Box::new(combined) };
        self.evaluate_general(&rewritten, destination)?;
        Ok(true)
    }

    pub(crate) fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        // A compile-time-constant expression — folded constant arithmetic
        // (`2 + 3`, `FLAG_A | FLAG_B`, `1 << 3`) or a side-effect-free identity
        // (`x - x`, `x ^ x`) — materializes the value directly, as mwcc folds it.
        // Bare literals fall through to the arm below.
        if !matches!(expression, Expression::IntegerLiteral(_)) {
            if let Some(value) = constant_value(expression) {
                self.load_integer_constant(destination, value);
                return Ok(());
            }
        }
        // mwcc REASSOCIATES an all-`+` chain of register leaves `v1+v2+…+vN` to `v1 + left-fold(v2..vN)`
        // and allocates it as `add r0,R2,R3; mr R2,R1; <fold R4..R(N-1) into r0>; add D,r0,RN; add D,R2,D`
        // (v1 kept in v2's freed register across the folds that would clobber D). Reproduce it directly
        // for the deferring N>=4 case (N<=3 is byte-exact on the normal path). Each operand must be a
        // DISTINCT register-resident variable — a repeated one is still live, and a constant/global/
        // frame leaf has no ready register — so those fall through to the defer below.
        if let Some(leaves) = crate::analysis::add_chain_leaves(expression) {
            if leaves.len() >= 4 && destination != GENERAL_SCRATCH {
                let names: Vec<&str> = leaves.iter().filter_map(|leaf| leaf_name(leaf)).collect();
                let mut sorted = names.clone();
                sorted.sort_unstable();
                sorted.dedup();
                let distinct = names.len() == leaves.len() && sorted.len() == names.len();
                let registers: Option<Vec<u8>> = leaves.iter().map(|leaf| self.general_register_of_leaf(leaf).ok()).collect();
                if let (true, Some(registers)) = (distinct, registers) {
                    if !registers.contains(&GENERAL_SCRATCH) {
                        let last = registers.len() - 1;
                        self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: registers[1], b: registers[2] });
                        self.output.instructions.push(Instruction::move_register(registers[1], registers[0]));
                        for &register in &registers[3..last] {
                            self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: register });
                        }
                        self.output.instructions.push(Instruction::Add { d: destination, a: GENERAL_SCRATCH, b: registers[last] });
                        self.output.instructions.push(Instruction::Add { d: destination, a: registers[1], b: destination });
                        return Ok(());
                    }
                }
            }
        }
        // Other reassociated add-trees (nested non-leaf operands, mixed with `*`) still diverge in
        // register allocation — defer rather than emit wrong bytes (#20 allocator).
        if crate::analysis::contains_complex_add(expression) {
            return Err(Diagnostic::error("a reassociated integer add-tree needs the keystone allocator (roadmap)"));
        }
        // mwcc keeps a constant-amount shift as the FIRST operand of a commutative op (`(a<<2)+b` ->
        // `add d, shift, b`), but our placement swaps it to second (like `(a*4)+b`). Defer the
        // ordering rather than emit swapped bytes; matching it is the keystone allocator's job.
        if crate::analysis::contains_commutative_shift_left(expression) {
            return Err(Diagnostic::error("a commutative op with a constant-shift left operand orders operands differently (roadmap)"));
        }
        match expression {
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(destination, *value);
                Ok(())
            }
            Expression::StringLiteral(bytes) => self.emit_string_literal(bytes, destination),
            // `&x` for a frame-resident variable is its address: `addi d, r1, slot`.
            Expression::AddressOf { operand } => self.emit_address_of(operand, destination),
            Expression::Variable(name) => {
                // A frame-resident variable is reloaded from its stack slot; a local
                // array decays to the slot's address instead (`addi d,r1,offset`).
                if let Some(slot) = self.frame_slots.get(name).copied() {
                    if slot.is_array {
                        self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 1, immediate: slot.offset });
                    } else {
                        self.output.instructions.push(Instruction::LoadWord { d: destination, a: 1, offset: slot.offset });
                    }
                    return Ok(());
                }
                if let Some(location) = self.locations.get(name) {
                    if location.class != ValueClass::General {
                        return Err(Diagnostic::error(format!("'{name}' is not an integer")));
                    }
                    let (source, width, signed) = (location.register, location.width, location.signed);
                    self.emit_widen(destination, source, width, signed);
                    Ok(())
                } else if let Some(&total_size) = self.global_array_sizes.get(name.as_str()).filter(|_| destination != GENERAL_SCRATCH) {
                    // A bare array variable in value position decays to its address
                    // (`return g;` / `f(g)` for `int g[N]`), not a load of g[0].
                    self.emit_global_array_base(name, total_size, destination)
                } else {
                    self.emit_global_load(name, destination)
                }
            }
            Expression::Unary { operator, operand } => {
                // Negating a COMPARISON value — mwcc folds the negate into the comparison's bool
                // idiom. The SIGN-BIT comparisons `-(x < 0)` / `-(x > 0)` are modeled byte-exactly
                // (a 0/-1 via neg/andc/srawi), but every OTHER negated comparison (`-(a < b)`,
                // `-(a == 0)`, against a non-zero constant) uses a fused srawi/rlwinm we don't model,
                // whereas ours emits the 0/1 value and a separate `neg` — a byte-different sequence.
                if *operator == UnaryOperator::Negate {
                    if let Expression::Binary { operator: inner, right, .. } = operand.as_ref() {
                        let is_sign_bit_comparison = matches!(inner, BinaryOperator::Less | BinaryOperator::Greater)
                            && crate::analysis::constant_value(right) == Some(0);
                        if is_comparison(*inner) && !is_sign_bit_comparison {
                            return Err(Diagnostic::error("negating a comparison value uses a fused bool idiom not modeled (roadmap)"));
                        }
                    }
                }
                self.emit_unary(*operator, operand, destination)
            }
            Expression::Conditional { condition, when_true, when_false } => {
                self.emit_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { target_type, operand } => self.emit_cast_to_integer(*target_type, operand, destination),
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
            Expression::Member { base, offset, member_type, index_stride } => self.emit_member_load(base, *offset, *member_type, *index_stride, destination),
            Expression::MemberAddress { base, offset, .. } => {
                // The array's address: `base + offset` (a `mr` when the array is at
                // the start of the struct).
                let base_register = self.member_base_register(base)?;
                if *offset == 0 {
                    if base_register != destination {
                        self.output.instructions.push(Instruction::move_register(destination, base_register));
                    }
                } else {
                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: base_register, immediate: *offset as i16 });
                }
                Ok(())
            }
            Expression::Index { base, index } => self.emit_subscript(base, index, destination),
            Expression::Call { name, arguments } => self.emit_call(name, arguments, Some(destination), false),
            Expression::Assign { target, value } => self.emit_assign(target, value, destination),
            // The comma operator is byte-exact only in proven value positions: a store
            // value (peeled in `place_store_value`) and a flat-arithmetic binary operand
            // (peeled in the `Binary` arm above). Reaching here means a risky sub-operand
            // position (unary operand, cast, return value, …) where forcing the right into
            // `destination` adds a move mwcc elides — defer rather than ship that diff.
            Expression::Comma { .. } => {
                let _ = destination;
                Err(Diagnostic::error("a comma operator in this position is not supported yet (roadmap)"))
            }
            Expression::Binary { operator, left, right } => {
                // mwcc folds a unary minus into a subtract: `-a + b` -> `b - a` (subf), `a + -b` ->
                // `a - b`. Rewrite to the equivalent Subtract so the byte-exact subtract path emits it
                // (the operand order matches: `subf d, a, b` computes b - a).
                if *operator == BinaryOperator::Add {
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand } = left.as_ref() {
                        let rewritten = Expression::Binary { operator: BinaryOperator::Subtract, left: right.clone(), right: operand.clone() };
                        return self.evaluate_general(&rewritten, destination);
                    }
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand } = right.as_ref() {
                        let rewritten = Expression::Binary { operator: BinaryOperator::Subtract, left: left.clone(), right: operand.clone() };
                        return self.evaluate_general(&rewritten, destination);
                    }
                }
                // `(cmp1) OP (cmp2)` — an arithmetic/bitwise combine of TWO comparison-as-value
                // idioms (`(a>0) - (a<0)`, `(a<b) + (a>b)`, `(a>0) | (a<0)`). mwcc INTERLEAVES the
                // two comparison computations (instruction scheduling + register allocation); ours
                // evaluates them sequentially — a correct value but a byte-different order. Matching
                // the schedule needs the register allocator, so defer. (A logical `&&`/`||` combine
                // routes through the short-circuit path; a comparison mixed with a leaf/constant —
                // `(a<b) - 1` — keeps its single-comparison shape and is unaffected.)
                if !is_comparison(*operator)
                    && !matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr)
                    && matches!(left.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner))
                    && matches!(right.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner))
                {
                    return Err(Diagnostic::error("an arithmetic combine of two comparison values needs the register allocator's interleaving (roadmap)"));
                }
                // `(cmp) << k` and `(cmp) & k` — mwcc fuses the comparison's sign-bit extract with
                // the shift into one `rlwinm`, and drops a `& k` mask that is redundant on a 0/1
                // value. Neither peephole is modeled, so defer. (Other operators on a comparison —
                // `| ^ >> + - *` — keep the plain comparison-value shape and stay byte-exact.)
                if matches!(operator, BinaryOperator::ShiftLeft | BinaryOperator::BitAnd)
                    && (matches!(left.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner))
                        || matches!(right.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner)))
                {
                    return Err(Diagnostic::error("a shift-left or bitwise-and of a comparison value uses a fused rlwinm / dropped mask not modeled (roadmap)"));
                }
                // A repeated NON-LEAF sub-expression (`(a+1)+(a+1)`, `(a + (a>>31)) ^ (a>>31)`) is a
                // common sub-expression mwcc computes ONCE and reuses; our straight-line codegen
                // recomputes it — a byte-different sequence. Defer until the register allocator does
                // CSE. (Leaf repeats like `a + a` are cheap re-reads and stay byte-exact.)
                if crate::analysis::has_repeated_nonleaf_subexpression(expression) {
                    return Err(Diagnostic::error("a repeated common sub-expression needs the register allocator's CSE (roadmap)"));
                }
                // A repeated GLOBAL variable leaf (`gi + gi`, `gi * gi`): unlike a register-resident
                // parameter/local (a free re-read), a global read is a LOAD, and mwcc loads it ONCE,
                // reusing the register (`lwz r0,gi; mullw r3,r0,r0`). The self-folding ops (`& | ^ - / %`
                // and comparisons: `g OP g` -> `g`, `0`, or `1`) collapse before this; the non-folding
                // `+ * << >>` reach here. Reproduce mwcc's load-once for `+`/`*` (load into the scratch,
                // then the op twice); the shifts' `slw`/`sraw` operand handling for this shape isn't
                // modeled, so they defer.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight) {
                    if let (Expression::Variable(left_name), Expression::Variable(right_name)) = (left.as_ref(), right.as_ref()) {
                        if left_name == right_name && self.globals.contains_key(left_name.as_str()) {
                            // A signed plain `char` global reads with `lbz` then a trailing `extsb` that
                            // this load-once path does not emit (`lha`/`lwz` and the unsigned loads
                            // self-extend, so they are fine). Defer the signed-char case — rare.
                            if matches!(self.globals.get(left_name.as_str()), Some(Type::Char)) {
                                return Err(Diagnostic::error("a repeated signed-char global read needs the sign-extended load-once (roadmap)"));
                            }
                            match operator {
                                BinaryOperator::Add => {
                                    self.emit_global_load_value(left_name, GENERAL_SCRATCH)?;
                                    self.output.instructions.push(Instruction::Add { d: destination, a: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                                    return Ok(());
                                }
                                BinaryOperator::Multiply => {
                                    self.emit_global_load_value(left_name, GENERAL_SCRATCH)?;
                                    self.output.instructions.push(Instruction::MultiplyLow { d: destination, a: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                                    return Ok(());
                                }
                                _ => return Err(Diagnostic::error("a repeated global read under a shift needs load-once reuse (roadmap)")),
                            }
                        }
                    }
                }
                // `gp->x OP gp->y` — a GLOBAL POINTER dereferenced on BOTH sides (any op, any members):
                // the pointer is a LOAD, and our per-operand access reloads it each time (`lwz r3,gp;
                // lwz r3,0(r3); lwz r0,gp; lwz r0,4(r0)`), while mwcc loads the pointer ONCE and reads
                // both members from it (`lwz r4,gp; lwz r3,0(r4); lwz r0,4(r4)`). Defer — a register-
                // resident pointer parameter, or two DIFFERENT global pointers, load correctly.
                {
                    fn deref_base(operand: &Expression) -> Option<&str> {
                        let base = match operand {
                            Expression::Member { base, .. } | Expression::Index { base, .. } => base.as_ref(),
                            Expression::Dereference { pointer } => pointer.as_ref(),
                            _ => return None,
                        };
                        match base {
                            Expression::Variable(name) => Some(name.as_str()),
                            _ => None,
                        }
                    }
                    if let (Some(left_base), Some(right_base)) = (deref_base(left), deref_base(right)) {
                        if left_base == right_base && self.globals.contains_key(left_base) {
                            return Err(Diagnostic::error("a global pointer dereferenced on both sides needs load-once reuse (roadmap)"));
                        }
                    }
                }
                // A comma operand with a side-effect-free left is equivalent to its right
                // value; peel it so the right keeps its natural register (`(a,b)+1` == `b+1`,
                // no spurious move). Only a flat arithmetic binary of leaves/constants is
                // provably byte-exact this way — comparisons and computed operands route to
                // codegen shapes with pre-existing gaps, so those (and a side-effecting left)
                // defer rather than ship a guess.
                if matches!(left.as_ref(), Expression::Comma { .. }) || matches!(right.as_ref(), Expression::Comma { .. }) {
                    let peel = |operand: &Expression| -> Compilation<Expression> {
                        let mut current = operand;
                        while let Expression::Comma { left, right } = current {
                            if expression_has_side_effect(left) {
                                return Err(Diagnostic::error("a comma operand with a side effect is not supported yet (roadmap)"));
                            }
                            current = right;
                        }
                        Ok(current.clone())
                    };
                    let (peeled_left, peeled_right) = (peel(left)?, peel(right)?);
                    let is_simple = |operand: &Expression| {
                        matches!(operand, Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_))
                    };
                    if is_comparison(*operator)
                        || matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr)
                        || !is_simple(&peeled_left)
                        || !is_simple(&peeled_right)
                    {
                        return Err(Diagnostic::error("a comma operand in this expression is not supported yet (roadmap)"));
                    }
                    let peeled = Expression::Binary {
                        operator: *operator,
                        left: Box::new(peeled_left),
                        right: Box::new(peeled_right),
                    };
                    return self.evaluate_general(&peeled, destination);
                }
                // Fold `(x OP c1) OP c2` to `x OP (c1 ⊕ c2)` for an associative operation with a
                // constant at each level — mwcc combines consecutive constant operations into a
                // single instruction: `(a+3)+5` is `addi r3,r3,8`, `(a>>2)>>3` is `srawi
                // r3,r3,5`, `(a&0xf0)&0x3c` is one `rlwinm`. Rewrite and re-evaluate so the
                // existing single-constant path emits it. (Left-associative trees keep the
                // constant on the inner/outer right, the common shape.)
                if let Some(outer_constant) = constant_value(right) {
                    if let Expression::Binary { operator: inner_operator, left: inner_left, right: inner_right } = left.as_ref() {
                        if let Some(inner_constant) = constant_value(inner_right) {
                            use BinaryOperator::*;
                            let folded = match (*operator, *inner_operator) {
                                (Add, Add) => Some((Add, inner_constant + outer_constant)),
                                (Subtract, Add) => Some((Add, inner_constant - outer_constant)),
                                (Add, Subtract) => Some((Add, outer_constant - inner_constant)),
                                (Subtract, Subtract) => Some((Subtract, inner_constant + outer_constant)),
                                (Multiply, Multiply) => inner_constant.checked_mul(outer_constant).map(|product| (Multiply, product)),
                                (BitAnd, BitAnd) => Some((BitAnd, inner_constant & outer_constant)),
                                (BitOr, BitOr) => Some((BitOr, inner_constant | outer_constant)),
                                (BitXor, BitXor) => Some((BitXor, inner_constant ^ outer_constant)),
                                (ShiftLeft, ShiftLeft) | (ShiftRight, ShiftRight)
                                    if (1..=31).contains(&(inner_constant + outer_constant)) =>
                                    Some((*operator, inner_constant + outer_constant)),
                                _ => None,
                            };
                            if let Some((result_operator, result_constant)) = folded {
                                let folded_expression = Expression::Binary {
                                    operator: result_operator,
                                    left: inner_left.clone(),
                                    right: Box::new(Expression::IntegerLiteral(result_constant)),
                                };
                                return self.evaluate_general(&folded_expression, destination);
                            }
                        }
                    }
                }
                // Operand cancellation: `(X + Y) - Y` is `X`, `(X + Y) - X` is `Y`, and
                // `(X - Y) + Y` is `X` — the operand that appears with opposite signs cancels,
                // and mwcc folds straight to the survivor (`(a+b)-b` is `blr`; `(a+5)-a` is
                // `li r3,5`). The cancelling operand must be a side-effect-free LEAF (a variable
                // or constant — read twice gives the same value), so dropping its evaluation is
                // safe; the survivor is then evaluated normally.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)) {
                        if let Expression::Binary { operator: inner_operator, left: inner_left, right: inner_right } = left.as_ref() {
                            let survivor = match (*operator, *inner_operator) {
                                (BinaryOperator::Subtract, BinaryOperator::Add) => {
                                    if same_operand(right, inner_right) { Some(inner_left) }
                                    else if same_operand(right, inner_left) { Some(inner_right) }
                                    else { None }
                                }
                                (BinaryOperator::Add, BinaryOperator::Subtract) if same_operand(right, inner_right) => Some(inner_left),
                                _ => None,
                            };
                            if let Some(survivor) = survivor {
                                return self.evaluate_general(survivor, destination);
                            }
                        }
                    }
                }
                // Absorption: `(a & b) | a` is `a`, `(a | b) & a` is `a` (and the commuted
                // `a | (a & b)`, plus the bit-constant forms `(x | c) & c` -> c, `(x & c) | c`
                // -> c when the survivor is the constant). One operand subsumes the other and
                // mwcc folds straight to the survivor (`(a&b)|a` is a bare `blr`; `(x|7)&7` is
                // `li r3,7`). The inner op is dropped, so its operands — and the surviving leaf
                // — must be side-effect-free leaves (variables/constants) for the fold to hold.
                if matches!(operator, BinaryOperator::BitOr | BinaryOperator::BitAnd) {
                    let dual = if matches!(operator, BinaryOperator::BitOr) { BinaryOperator::BitAnd } else { BinaryOperator::BitOr };
                    for (inner, survivor) in [(left.as_ref(), right.as_ref()), (right.as_ref(), left.as_ref())] {
                        let survivor_is_leaf = matches!(survivor, Expression::Variable(_) | Expression::IntegerLiteral(_));
                        if let Expression::Binary { operator: inner_operator, left: p, right: q } = inner {
                            let inner_leaves = matches!(p.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                                && matches!(q.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_));
                            if survivor_is_leaf && *inner_operator == dual && inner_leaves
                                && (same_operand(survivor, p) || same_operand(survivor, q)) {
                                return self.evaluate_general(survivor, destination);
                            }
                        }
                    }
                }
                // `a*c1 + a*c2` / `a*c1 - a*c2` on the same variable distributes to `a*(c1±c2)`
                // — mwcc combines the like terms before strength reduction (`a*3 + a*5` is one
                // `slwi r3,r3,3` for `a*8`, not two `mulli`s). Fold and re-evaluate so the
                // existing `a*const` strength reduction emits the single instruction.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    let as_variable_times_constant = |expression: &Expression| -> Option<(String, i64)> {
                        if let Expression::Binary { operator: BinaryOperator::Multiply, left, right } = expression {
                            if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), constant_value(right)) {
                                return Some((name.clone(), constant));
                            }
                            if let (Some(constant), Expression::Variable(name)) = (constant_value(left), right.as_ref()) {
                                return Some((name.clone(), constant));
                            }
                        }
                        None
                    };
                    if let (Some((left_variable, left_constant)), Some((right_variable, right_constant))) =
                        (as_variable_times_constant(left), as_variable_times_constant(right))
                    {
                        let combined = if *operator == BinaryOperator::Add { left_constant + right_constant } else { left_constant - right_constant };
                        // mwcc folds `a*c1 ± a*c2` to a single `slwi` only in a narrow shape:
                        // both factors ODD and ≥ 3, distinct, with the combined factor a power
                        // of two ≥ 2. Each odd factor would otherwise be its own `mulli`, so
                        // collapsing them to one shift (`a*3 + a*5` -> `slwi r3,r3,3`) is mwcc's
                        // win. An even factor (`a*2`, itself shift-cheap), a factor of 1 (really
                        // `a`), identical terms (CSE'd), or a non-power-of-two sum (a `mulli`
                        // result) are NOT folded — they keep their existing lowering.
                        if left_variable == right_variable
                            && left_constant % 2 == 1
                            && right_constant % 2 == 1
                            && left_constant >= 3
                            && right_constant >= 3
                            && left_constant != right_constant
                            && combined >= 2
                            && (combined as u64).is_power_of_two()
                        {
                            let folded = Expression::Binary {
                                operator: BinaryOperator::Multiply,
                                left: Box::new(Expression::Variable(left_variable)),
                                right: Box::new(Expression::IntegerLiteral(combined)),
                            };
                            return self.evaluate_general(&folded, destination);
                        }
                    }
                }
                // `(a-b)-(c-d)`, `a*b-c*d`: a SUBTRACT whose BOTH operands are computed binary
                // expressions evaluates the two sub-trees in an order/allocation our straight-line path
                // does not match mwcc's (unlike `+`, subtraction is not commutative, so it cannot reuse
                // the overlap idiom). Defer until the keystone allocator schedules it; a leaf/constant
                // operand keeps the byte-exact single-scratch shape (`a-b-c`, `a-(b-c)`). Placed AFTER
                // the distributive fold so `a*5 - a*3` collapses to `a*2` first.
                if *operator == BinaryOperator::Subtract
                    && matches!(left.as_ref(), Expression::Binary { .. })
                    && matches!(right.as_ref(), Expression::Binary { .. })
                {
                    return Err(Diagnostic::error("a subtract of two computed sub-expressions needs the keystone allocator (roadmap)"));
                }
                // A signed char load (member `p->x`, element `a[i]`, deref `*p`) that is a
                // DIRECT operand of a comparison or a signed divide is loaded raw by these
                // branchless idioms — `p->x > 0` / `p->x / 2` operate on the zero-extended byte
                // where mwcc sign-extends first (`lbz r0; extsb r3,r0; <idiom>`), a miscompile
                // for a negative value. The byte-exact form needs mwcc's r0-load register
                // choice (the keystone allocator), so defer. (A masked operand `(p->x & 0xf) >
                // 0` is not a direct load and is unaffected; a short load sign-extends.)
                if is_comparison(*operator) || matches!(operator, BinaryOperator::Divide) {
                    if self.is_signed_byte_load(left.as_ref())? || self.is_signed_byte_load(right.as_ref())? {
                        // `signed_char < 0` is the 1-instruction sign-bit idiom; comparisons.rs loads
                        // the byte into the scratch and sign-extends it in place. The other relations
                        // (and divide) still need per-case handling, so defer them.
                        let against_zero = matches!(operator, BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::GreaterEqual | BinaryOperator::NotEqual | BinaryOperator::Equal | BinaryOperator::LessEqual)
                            && is_zero_literal(right.as_ref());
                        // `signed_char == c` (any small constant): `lbz r0; extsb r0,r0; subfic;
                        // cntlzw; srwi` — handled by the a==c leading-zeros idiom.
                        let equal_constant = matches!(operator, BinaryOperator::Equal)
                            && as_small_integer(right.as_ref()).is_some();
                        let handled = (against_zero || equal_constant)
                            && self.is_signed_byte_load(left.as_ref())?;
                        if !handled {
                            return Err(Diagnostic::error("a signed char load operand of a comparison/divide needs a sign-extension (roadmap)"));
                        }
                    }
                }
                // Comparisons compile to branchless idioms.
                if is_comparison(*operator) {
                    return self.emit_comparison(*operator, left, right, destination);
                }
                // Short-circuit `&&`/`||` as a value (a store, an operand) builds its
                // 0/1 result with forward branches through the scratch and a join, vs the
                // tail form's early `beqlr` returns.
                if matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr) {
                    return self.emit_short_circuit_via_scratch(*operator, left, right, destination);
                }
                // `&global +/- n` is pointer arithmetic: materialize the address into a
                // fresh register, then add the offset scaled by the pointee size
                // (`&ga + 1` is `addi d,&ga,4`). Add is commutative; subtract is ptr-int.
                // A variable index or an offset that overflows the `addi` immediate defers.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    let address = if self.is_global_address_of(left) {
                        Some((left, right))
                    } else if *operator == BinaryOperator::Add && self.is_global_address_of(right) {
                        Some((right, left))
                    } else {
                        None
                    };
                    if let Some((address, offset)) = address {
                        if let (Expression::AddressOf { operand: inner }, Some(count)) = (address.as_ref(), constant_value(offset)) {
                            let size = match inner.as_ref() {
                                Expression::Variable(name) => self.globals.get(name.as_str()).map(|global| (global.width() / 8) as i64),
                                _ => None,
                            };
                            let scaled = size.map(|size| count * size * if *operator == BinaryOperator::Subtract { -1 } else { 1 });
                            if let Some(Ok(immediate)) = scaled.map(i16::try_from) {
                                // mwcc materializes the address in the lowest free register
                                // then adds the offset: the destination in place when it is
                                // a real register (`addi r3,r3,n` for a return or call arg),
                                // else a fresh register (`li r3,0; addi r0,r3,n` for a store).
                                let address_register = if destination == GENERAL_SCRATCH {
                                    self.fresh_virtual_general()
                                } else {
                                    destination
                                };
                                self.emit_address_of(inner, address_register)?;
                                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: address_register, immediate });
                                return Ok(());
                            }
                        }
                        return Err(Diagnostic::error("pointer arithmetic on a global's address needs offset scaling (roadmap)"));
                    }
                }
                // Identical simple loads on both sides (`*p op *p`, `a[0]+a[0]`):
                // mwcc loads the value ONCE and folds operator identities, rather
                // than the two-operand double load.
                if self.try_emit_identical_load_binary(*operator, left, right, destination)? {
                    return Ok(());
                }
                // Negation folds in a subtraction: `X - (-Y)` is `X + Y`, and
                // `(-a) - Y` (non-constant Y) is `-(a + Y)` — mwcc cancels the
                // double negative / hoists the negate over the sum.
                if *operator == BinaryOperator::Subtract {
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = right.as_ref() {
                        let sum = Expression::Binary { operator: BinaryOperator::Add, left: left.clone(), right: inner.clone() };
                        return self.evaluate_general(&sum, destination);
                    }
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = left.as_ref() {
                        if constant_value(right).is_none() {
                            let sum = Expression::Binary { operator: BinaryOperator::Add, left: inner.clone(), right: right.clone() };
                            let negated = Expression::Unary { operator: UnaryOperator::Negate, operand: Box::new(sum) };
                            return self.evaluate_general(&negated, destination);
                        }
                    }
                }
                // A shift fused with a mask — `(x >> n) & m`, `(x & m) << n`, etc. —
                // is a single rotate-and-mask (`rlwinm`). Caught before the per-shift
                // paths so the fused form wins over a plain shift.
                if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight)
                    && self.try_emit_rotate_mask(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                // Right shift, divide, and modulo select instructions by signedness.
                if *operator == BinaryOperator::ShiftRight {
                    return self.emit_shift_right(left, right, destination);
                }
                if *operator == BinaryOperator::Divide {
                    return self.emit_divide(left, right, destination);
                }
                if *operator == BinaryOperator::Modulo {
                    return self.emit_modulo(left, right, destination);
                }
                // Pointer arithmetic scales the integer operand by the pointee
                // size (e.g. `int* + i` -> `slwi i,2; add`); byte pointers need no
                // scaling and fall through to plain addition.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.try_emit_pointer_arithmetic(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                // `x & ~y` / `x | ~y` fuse into andc/orc.
                if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::BitOr)
                    && self.try_emit_complement_logical(*operator, left, right, destination)
                {
                    return Ok(());
                }
                // `(x & m1) | (x & m2)` for the same leaf folds to `x & (m1|m2)`.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_same_leaf_mask_or(left, right, destination)?
                {
                    return Ok(());
                }
                // An OR of two complementary bit fields (shifts and/or masks) —
                // including a constant rotate — merges via one rlwimi.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_field_merge(left, right, destination)?
                {
                    return Ok(());
                }
                // The same merge where the operands are memory loads (the pointer-pun
                // `__HI`/`__LO` merge): load both, then rlwimi.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_field_merge_loads(left, right, destination)?
                {
                    return Ok(());
                }
                // mwcc reassociates a left-leaning pure-addition chain before the
                // generic paths see it (`a+b+c` -> `a+(b+c)`).
                if *operator == BinaryOperator::Add
                    && self.try_emit_additive_chain(left, right, destination)?
                {
                    return Ok(());
                }
                // `(cond ? c1 : c2) +/- k` distributes the constant into the arms
                // (mwcc folds the select's trailing `addi`).
                if self.try_emit_select_constant_fold(*operator, left, right, destination)? {
                    return Ok(());
                }
                // A 16-bit constant operand folds into an immediate instruction.
                if self.try_emit_general_with_constant(*operator, left, right, destination)? {
                    return Ok(());
                }
                // Two memory loads from a common base (`a[i] op a[j]`, `s->x op s->y`)
                // — the first multi-operand shape on the register allocator.
                if self.try_emit_two_load_binary(*operator, left, right, destination)? {
                    return Ok(());
                }
                // A constant-index subscript load paired with a wide-int leaf
                // (`a[k] op x`) — the load goes to the scratch like a dereference.
                if self.try_emit_subscript_leaf_binary(*operator, left, right, destination)? {
                    return Ok(());
                }
                // `(x & y) | (x & z)` and the other bitwise distributive laws collapse to
                // `x & (y | z)` (one fewer instruction), as mwcc does.
                if self.try_emit_distributive_bitwise(*operator, left, right, destination)? {
                    return Ok(());
                }
                // Two COMPOUND-load operands (each wrapping a load in an op, `p->x*p->x +
                // p->y*p->y`) reach the generic combine only as genuinely complex shapes — the
                // simple two-load idioms above already took `*p op *q`, `s->x op s->y`, and two BARE
                // loads (`*(p+1)+*(p+2)`) stay byte-exact (loads adjacent). mwcc hoists both loads to
                // the top with an allocator-chosen register assignment; the generic combine
                // interleaves load/op/load/op — same result, different schedule. Defer, don't ship.
                if is_compound_load(left) && is_compound_load(right) {
                    return Err(Diagnostic::error("a binary over two compound-load operands needs the allocator (roadmap)"));
                }
                if !fits_single_scratch(expression, destination == GENERAL_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_general_operands(*operator, left, right)?;
                self.output.instructions.push(general_combine(*operator, destination, operands)?);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literal in integer context")),
        }
    }

    /// mwcc reassociates a left-leaning pure-addition chain `(x + y) + z` into
    /// `x + (y + z)`: it evaluates the tail `y + z` into the destination first,
    /// then adds the leading operand `x`. `x` is copied to the scratch beforehand
    /// only when it lives in the destination (which the tail overwrites). Only a
    /// full-width integer leaf `x` with simple integer tail operands is taken;
    /// pointers (scaled arithmetic), narrow leaves, and deeper or right-leaning
    /// chains keep the generic paths.
    fn try_emit_additive_chain(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        let Expression::Binary { operator: BinaryOperator::Add, left: x, right: y } = left else {
            return Ok(false);
        };
        let (x, y, z) = (x.as_ref(), y.as_ref(), right);
        // `(loadA + loadB) + Z` reassociates in mwcc (`A + (B + Z)`) with an
        // allocator-specific register assignment we do not reproduce yet. Defer it
        // rather than fall through to the constant-fold path, which would emit the
        // left-associated form (a mismatch) now that the two-load add is selected.
        if self.is_word_load(x) && self.is_word_load(y) {
            return Err(Diagnostic::error("additive chain over two loads needs the allocator (roadmap)"));
        }
        let Some(x_register) = self.plain_integer_leaf_register(x) else { return Ok(false) };
        // The tail operands must be simple: a full-width integer leaf or a constant.
        let simple = |me: &Self, operand: &Expression| {
            constant_value(operand).is_some() || me.plain_integer_leaf_register(operand).is_some()
        };
        if !simple(self, y) || !simple(self, z) {
            return Ok(false);
        }
        // Saving x needs a scratch register distinct from the destination.
        if x_register == destination && destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let leading = if x_register == destination {
            self.output.instructions.push(Instruction::move_register(GENERAL_SCRATCH, x_register));
            GENERAL_SCRATCH
        } else {
            x_register
        };
        let tail = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(y.clone()),
            right: Box::new(z.clone()),
        };
        self.evaluate_general(&tail, destination)?;
        self.output.instructions.push(Instruction::Add { d: destination, a: leading, b: destination });
        Ok(true)
    }

    /// `loadA op loadB` where both operands are full-word memory loads from a
    /// COMMON base (`a[i] op a[j]`, `s->x op s->y`). mwcc's binary-node convention
    /// puts the secondary source in the scratch (r0) and the primary in a register
    /// the allocator colors; the shared base stays live, so the primary takes the
    /// next free volatile (r4). The primary is the left operand for `add` and the
    /// right operand for `subf` (which computes `b - a`), loaded first.
    /// `L op L` with both operands the identical side-effect-free value: mwcc
    /// uses it ONCE. `x&x`/`x|x` are the value itself; for a memory LOAD, `x+x`/
    /// `x*x`/`x<<x`/`x>>x` load once into the scratch then apply the op to that
    /// register twice (`add d,r0,r0`, `mullw`, `slw`, `sraw`/`srw`). `x-x`/`x^x`
    /// fold to 0 in `constant_value` before reaching here (kept as a fallback). A
    /// leaf `x+x`/`x*x`/… falls through — its operand is already in a register so
    /// the generic path emits `op d,a,a`.
    fn try_emit_identical_load_binary(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        use BinaryOperator::*;
        if !same_operand(left, right) {
            return Ok(false);
        }
        match operator {
            BitAnd | BitOr => self.evaluate_general(left, destination)?,
            Subtract | BitXor => self.load_integer_constant(destination, 0),
            Add | Multiply | ShiftLeft | ShiftRight if self.is_simple_word_load(left) => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                let r = GENERAL_SCRATCH;
                let instruction = match operator {
                    Add => Instruction::Add { d: destination, a: r, b: r },
                    Multiply => Instruction::MultiplyLow { d: destination, a: r, b: r },
                    ShiftLeft => Instruction::ShiftLeftWord { a: destination, s: r, b: r },
                    _ if self.signedness_of(left)? => Instruction::ShiftRightAlgebraicWord { a: destination, s: r, b: r },
                    _ => Instruction::ShiftRightWord { a: destination, s: r, b: r },
                };
                self.output.instructions.push(instruction);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `(x & m1) | (x & m2)` over the same leaf `x` is `x & (m1 | m2)` — mwcc
    /// merges the masks into one `rlwinm`/`clrlwi` rather than extracting both
    /// fields and OR-ing them.
    fn try_emit_same_leaf_mask_or(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        let (Some((left_leaf, m1)), Some((right_leaf, m2))) = (as_masked_leaf(left), as_masked_leaf(right)) else {
            return Ok(false);
        };
        if leaf_name(left_leaf).is_none() || leaf_name(left_leaf) != leaf_name(right_leaf) {
            return Ok(false);
        }
        let combined = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(left_leaf.clone()),
            right: Box::new(Expression::IntegerLiteral((m1 | m2) as i64)),
        };
        self.evaluate_general(&combined, destination)?;
        Ok(true)
    }

    fn try_emit_two_load_binary(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        use BinaryOperator::*;
        if !matches!(operator, Add | Subtract | BitAnd | BitOr | BitXor | Multiply) {
            return Ok(false);
        }
        // Single-instruction loads only: a variable-index subscript scales to
        // `slwi; lwzx`, and two of those mis-schedule against each other.
        if !self.is_simple_word_load(left) || !self.is_simple_word_load(right) {
            return Ok(false);
        }
        if load_base_name(left).is_none() || load_base_name(right).is_none() {
            return Ok(false);
        }
        // `subf` computes `b - a`; to get `left - right` the right operand is the
        // primary (first source) and the left is the secondary (in r0). The
        // commutative operators keep the left operand as the primary.
        let (primary, secondary) = match operator {
            Subtract => (right, left),
            _ => (left, right),
        };
        let primary_register = self.fresh_virtual_general();
        self.evaluate_general(primary, primary_register)?;
        self.evaluate_general(secondary, GENERAL_SCRATCH)?;
        let (p, s) = (primary_register, GENERAL_SCRATCH);
        let combined = match operator {
            Add => Instruction::Add { d: destination, a: p, b: s },
            Subtract => Instruction::SubtractFrom { d: destination, a: p, b: s },
            Multiply => Instruction::MultiplyLow { d: destination, a: p, b: s },
            BitAnd => Instruction::And { a: destination, s: p, b: s },
            BitOr => Instruction::Or { a: destination, s: p, b: s },
            _ => Instruction::Xor { a: destination, s: p, b: s },
        };
        self.output.instructions.push(combined);
        Ok(true)
    }

    /// `a[k] op x` — a constant-index subscript word-load combined with a wide
    /// integer leaf. The subscript loads into the scratch (`lwz r0,off(base)`) and
    /// the leaf stays in its register, like the dereference/member + leaf paths
    /// (subscripts just were not routed there). Source operand order is kept.
    fn try_emit_subscript_leaf_binary(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        use BinaryOperator::*;
        if !matches!(operator, Add | Subtract | BitAnd | BitOr | BitXor | Multiply) {
            return Ok(false);
        }
        // A full-word subscript load with either a constant index (`a[3]`, a plain
        // `lwz off(base)`) or a variable index (`a[i]`, scaled `slwi r0,i,2; lwzx
        // r0,base,r0`). Either way `evaluate_general` lands it in the scratch, and
        // the binary then reads the wide leaf in place — matching mwcc's
        // `…; add d,leaf,r0`.
        let is_word_subscript = |me: &Self, expression: &Expression| {
            matches!(expression, Expression::Index { .. }) && me.is_word_load(expression)
        };
        // Exactly one operand is the subscript load; the other a wide integer leaf.
        let (load, leaf, load_is_left) = match (is_word_subscript(self, left), is_word_subscript(self, right)) {
            (true, false) => (left, right, true),
            (false, true) => (right, left, false),
            _ => return Ok(false),
        };
        let Some(leaf_register) = self.plain_integer_leaf_register(leaf) else {
            return Ok(false);
        };
        // A scaled (variable-index) subscript is the heavier subtree, so mwcc
        // canonicalizes it to the SECOND operand of a commutative op (leaf first),
        // regardless of source order — `add d,leaf,r0`. A plain (constant-index)
        // subscript keeps source order. Subtract is non-commutative either way.
        let variable_index = matches!(load, Expression::Index { index, .. } if constant_value(index).is_none());
        let commutative = !matches!(operator, BinaryOperator::Subtract);
        self.evaluate_general(load, GENERAL_SCRATCH)?;
        let (a, b) = if commutative && variable_index {
            (leaf_register, GENERAL_SCRATCH)
        } else if load_is_left {
            (GENERAL_SCRATCH, leaf_register)
        } else {
            (leaf_register, GENERAL_SCRATCH)
        };
        let combined = match operator {
            Add => Instruction::Add { d: destination, a, b },
            Subtract => Instruction::SubtractFrom { d: destination, a: b, b: a },
            Multiply => Instruction::MultiplyLow { d: destination, a, b },
            BitAnd => Instruction::And { a: destination, s: a, b },
            BitOr => Instruction::Or { a: destination, s: a, b },
            _ => Instruction::Xor { a: destination, s: a, b },
        };
        self.output.instructions.push(combined);
        Ok(true)
    }

    /// `(cond ? c1 : c2) +/- k` with both arms constant distributes the constant
    /// into the arms — `cond ? (c1±k) : (c2±k)` — so the select's trailing `addi`
    /// absorbs it, as mwcc does (a leaf `(x?1:2)+5` is `…; addi r3,r3,7`).
    fn try_emit_select_constant_fold(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        let constant_select = |expression: &Expression| matches!(expression,
            Expression::Conditional { when_true, when_false, .. }
                if constant_value(when_true).is_some() && constant_value(when_false).is_some());
        // Add commutes (select either side); subtract distributes only `select - k`.
        let (select, delta) = match operator {
            BinaryOperator::Add if constant_select(left) => match constant_value(right) { Some(k) => (left, k), None => return Ok(false) },
            BinaryOperator::Add if constant_select(right) => match constant_value(left) { Some(k) => (right, k), None => return Ok(false) },
            BinaryOperator::Subtract if constant_select(left) => match constant_value(right) { Some(k) => (left, -k), None => return Ok(false) },
            _ => return Ok(false),
        };
        let Expression::Conditional { condition, when_true, when_false } = select else { return Ok(false) };
        let shifted_true = Expression::IntegerLiteral(constant_value(when_true).unwrap() + delta);
        let shifted_false = Expression::IntegerLiteral(constant_value(when_false).unwrap() + delta);
        self.emit_conditional(condition, &shifted_true, &shifted_false, destination, false)?;
        Ok(true)
    }

    /// Place an operand and return the register holding it. A leaf stays in its
    /// own register. A sub-expression is computed into the destination when the
    /// consumer can fold it there (`addi`), otherwise into the scratch register —
    /// mwcc keeps `addi` operands in place but routes `rlwinm`/logical operands
    /// through `r0`. Returns `None` when a scratch operand does not fit.
    /// Emit `*pointer` — load the pointed-to value into `destination`, choosing
    /// the load by the pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`). The pointer
    /// must be a leaf variable holding the address; richer addressing is on the
    /// roadmap.
    pub(crate) fn emit_load_from_pointer(&mut self, pointer: &Expression, destination: u8) -> Compilation<()> {
        // A type-pun through a frame-resident address (`*(int*)&x`) is a plain
        // displacement load from r1.
        if let Some((pointee, offset)) = self.resolve_frame_pointer(pointer) {
            self.output.instructions.push(displacement_load(pointee, destination, 1, offset));
            return Ok(());
        }
        // A global pointer: load the pointer value into the destination (an SDA21
        // word load), then dereference it from there, as mwcc does.
        if let Expression::Variable(name) = pointer {
            if !self.locations.contains_key(name) {
                if let Some(Type::Pointer(pointee)) = self.globals.get(name).copied() {
                    // The pointer and the integer result share the destination, so a
                    // float pointee (which needs a separate general register for the
                    // address) is deferred rather than miscompiled.
                    if !matches!(pointee, Pointee::Float | Pointee::Double) {
                        self.emit_global_load(name, destination)?;
                        self.output.instructions.push(displacement_load(pointee, destination, destination, 0));
                        return Ok(());
                    }
                }
            }
        }
        // `*(p + i)` / `*(p + 3)` is exactly `p[i]` / `p[3]` — mwcc emits the identical
        // `slwi; lwzx` (variable index) or displacement `lwz` (constant). Route a
        // pointer-plus-index dereference to the subscript path. The pointer operand is the
        // base (the dereferenced_width-resolvable side), the integer the index; `+` commutes.
        // Narrow char/short pointees are now handled too: dereferenced_width / pointee_of see
        // through the `p + i` pointer, so a narrow `*(p+i)` either extends correctly (a return
        // adds the extsb via is_signed_byte_load) or defers in arithmetic — like `p[i]`.
        if let Expression::Binary { operator: BinaryOperator::Add, left, right } = pointer {
            if self.dereferenced_width(left).is_some() {
                return self.emit_subscript(left, right, destination);
            }
            if self.dereferenced_width(right).is_some() {
                return self.emit_subscript(right, left, destination);
            }
        }
        // `*(p - C)` is `p[-C]` — a displacement load at the negative offset. Subtract does NOT
        // commute (the pointer is always the left operand), and only a CONSTANT offset to a
        // NON-narrow pointee is routed: a variable `*(p - i)` needs a negated, scaled index
        // (`neg; slwi; lwzx`), and a char/short pointee needs the narrow machinery to see
        // through the `p - C` pointer (as it does for `p + C`) — both keep deferring.
        if let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = pointer {
            if let Some(constant) = constant_value(right) {
                if self.dereferenced_width(left) >= Some(32) {
                    return self.emit_subscript(left, &Expression::IntegerLiteral(-constant), destination);
                }
            }
        }
        // `*(T *)0xADDR` — a constant-address load. When the address fits the signed 16-bit
        // displacement (high half zero) mwcc loads straight off the r0=0 base (`ld dest,
        // lo(0)`); otherwise it materializes the sign-adjusted high half with `lis dest, hi`
        // and folds the low half into the displacement (`ld dest, lo(dest)`), reusing the
        // destination as the address register. r0 cannot be an address base, so a high-half
        // load into r0 (the char-return narrowing path, which also masks) is deferred rather
        // than miscompiled.
        if let Some((pointee, address)) = const_address_pointer(pointer) {
            if self.emit_const_address_load(pointee, address, 0, destination)? {
                return Ok(());
            }
            return Err(Diagnostic::error("a constant-address load into r0 is not supported yet (roadmap)"));
        }
        let (pointee, address) = self.resolve_pointer(pointer)?;
        self.output.instructions.push(displacement_load(pointee, destination, address, 0));
        Ok(())
    }

    /// A string literal in expression position: intern it into the function's pooled
    /// `@N` strings (deduplicated by bytes), then load that object's address. Under
    /// small-data addressing this is `addi d,0,0` + an `R_PPC_EMB_SDA21` relocation to
    /// a placeholder `@@strN` name, which the unit's string resolver rewrites to the
    /// real `@N`.
    fn emit_string_literal(&mut self, bytes: &[u8], destination: u8) -> Compilation<()> {
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                let index = self.intern_string_literal(bytes);
                let placeholder = format!("@@str{index}");
                // A string within the small-data threshold (≤ 8 bytes incl. the NUL) lands in
                // `.sdata` and is reached with a single SDA21 `li`; a larger one lands in `.data`
                // (the writer routes by size) and is reached with ADDR16 `lis`/`addi` (`@ha`/`@l`),
                // exactly like a large global array's base.
                if bytes.len() + 1 > 8 {
                    self.emit_address_high(destination, &placeholder);
                    self.record_relocation(RelocationKind::Addr16Lo, &placeholder);
                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: 0 });
                } else {
                    self.record_relocation(RelocationKind::EmbSda21, &placeholder);
                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 0, immediate: 0 });
                }
                // The `@@str{index}` placeholder is resolved to the function's per-function `@N`
                // string symbol by the unit's string resolver (apps/mwcc), which places each
                // function's strings at the FRONT of its anonymous-`@N` block (before its constants
                // and unwind entries) and defers the not-yet-modeled cases (file-scope strings, or a
                // function that also has a jump table).
                Ok(())
            }
            GlobalAddressing::Absolute => Err(Diagnostic::error("a string literal under absolute addressing is not supported yet (roadmap)")),
        }
    }

    /// Intern a string literal into the function's pooled list (by bytes), returning
    /// its index. The unit-wide resolver assigns the `@N` names after lowering.
    fn intern_string_literal(&mut self, bytes: &[u8]) -> usize {
        if let Some(index) = self.output.string_literals.iter().position(|existing| existing.as_slice() == bytes) {
            return index;
        }
        self.output.string_literals.push(bytes.to_vec());
        self.output.string_literals.len() - 1
    }

    /// Emit `base->field` — a displacement load from the struct pointer's register
    /// at the member's offset, choosing the load by the member type. The base must
    /// be a struct-pointer leaf variable (chained/complex bases are roadmap).
    /// Load from constant `address + offset` (a `*(T *)C` deref or a `(*(struct S *)C).field`
    /// member). Materializes the address with the `lis hi` / displacement-`lo` split, folding
    /// the member offset into the displacement; a zero high half loads off the r0=0 base. Returns
    /// `false` (caller defers) when it cannot be byte-exact: the displacement overflows i16, or a
    /// high-half address would have to use r0 (an invalid base) as the destination/base register.
    fn emit_const_address_load(&mut self, pointee: Pointee, address: u32, offset: u16, destination: u8) -> Compilation<bool> {
        let (high, low) = split_address(address);
        let Some(displacement) = (low as i32).checked_add(offset as i32).and_then(|d| i16::try_from(d).ok()) else {
            return Ok(false);
        };
        if high != 0 && destination == 0 {
            return Ok(false); // r0 can't be an address base
        }
        // Only the FIRST constant-address access in a function is byte-exact. mwcc handles a run
        // of them by allocating all the bases up front (chosen by look-ahead over every value)
        // and scheduling them together — keystone-level register allocation. So a second access
        // of any kind defers rather than emit a fresh, mis-scheduled sequence.
        if !self.const_address_bases.is_empty() {
            return Ok(false);
        }
        self.const_address_bases.insert(high);
        if high == 0 {
            self.output.instructions.push(displacement_load(pointee, destination, 0, displacement));
        } else {
            self.output.instructions.push(Instruction::load_immediate_shifted(destination, high));
            self.output.instructions.push(displacement_load(pointee, destination, destination, displacement));
        }
        Ok(true)
    }

    /// Store `value` to constant `address + offset` (a `*(T *)C = v` or `(*(struct S *)C).f = v`).
    /// The address base is materialized before the value and kept clear of the value's input
    /// registers, mirroring the absolute global store. Returns `false` (caller defers) when the
    /// displacement overflows i16.
    fn emit_const_address_store(&mut self, pointee: Pointee, address: u32, offset: u16, value: &Expression) -> Compilation<bool> {
        let (high, low) = split_address(address);
        let Some(displacement) = (low as i32).checked_add(offset as i32).and_then(|d| i16::try_from(d).ok()) else {
            return Ok(false);
        };
        // Only the FIRST constant-address access in a function is byte-exact; a second of any
        // kind needs mwcc's look-ahead base allocation and scheduling (keystone-level). Defer.
        if !self.const_address_bases.is_empty() {
            return Ok(false);
        }
        self.const_address_bases.insert(high);
        if high == 0 {
            let source = self.place_store_value(value, pointee)?;
            self.output.instructions.push(displacement_store(pointee, source, 0, displacement));
            return Ok(true);
        }
        let base = self.free_register_avoiding(&[value])?;
        let restore = self.reserved.insert(base);
        self.output.instructions.push(Instruction::load_immediate_shifted(base, high));
        let source = self.place_store_value(value, pointee)?;
        if restore { self.reserved.remove(&base); }
        self.output.instructions.push(displacement_store(pointee, source, base, displacement));
        Ok(true)
    }

    pub(crate) fn emit_member_load(&mut self, base: &Expression, offset: u16, member_type: Type, index_stride: Option<u16>, destination: u8) -> Compilation<()> {
        // `a[i].field`: scale the index by the struct size, then load at the field
        // offset — `slwi/mulli r0,i,stride; add a,a,r0; lwz d,offset(a)` (or `lwzx`
        // for a zero offset).
        if let (Expression::Index { base: array, index }, Some(stride)) = (base, index_stride) {
            return self.emit_indexed_member_load(array, index, stride, offset, member_type, destination);
        }
        // A nested member through an EMBEDDED struct value (`p->s.b`, `a.b.c`): the
        // intermediate sub-struct sits inline, not behind a pointer, so its member is
        // `base + inner_offset + offset` — fold the offsets and recurse rather than
        // load the sub-struct as if it were a pointer to dereference.
        if let Expression::Member { base: inner, offset: inner_offset, member_type: Type::Struct { .. }, index_stride: None } = base {
            return self.emit_member_load(inner, inner_offset + offset, member_type, index_stride, destination);
        }
        // `v.field` where `v` is a frame-resident struct local: a plain r1-relative
        // load at the slot offset plus the member offset.
        if let Expression::Variable(name) = base {
            if let Some(slot) = self.frame_slots.get(name) {
                let pointee = pointee_of_type(member_type)
                    .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
                self.output.instructions.push(displacement_load(pointee, destination, 1, slot.offset + offset as i16));
                return Ok(());
            }
            // `gp->field` where `gp` is a global struct pointer: load the pointer
            // value through its global addressing, then load the field at its offset
            // from that register — `lwz d, gp@…; lwz d, offset(d)`. (A global struct
            // *value* or *array* base needs an address-of, not a value load, so it
            // falls through to defer.)
            if !self.locations.contains_key(name.as_str())
                && matches!(self.globals.get(name.as_str()), Some(Type::StructPointer { .. }))
            {
                self.emit_global_load_value(name, destination)?;
                let pointee = pointee_of_type(member_type)
                    .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
                self.output.instructions.push(displacement_load(pointee, destination, destination, offset as i16));
                return Ok(());
            }
            // `g.field` where `g` is a global struct VALUE: materialize g's address
            // (SDA21 `li d,g@sda21` small / `lis;addi` large), then load the field at
            // its offset — `li d,g; lwz d,offset(d)`. The base register cannot be the
            // scratch r0 (it is then its own load base).
            if !self.locations.contains_key(name.as_str()) && destination != GENERAL_SCRATCH {
                if let Some(Type::Struct { size, .. }) = self.globals.get(name.as_str()).copied() {
                    let pointee = pointee_of_type(member_type)
                        .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
                    // An offset-0 member of a *small* (SDA-addressed, <= 8 byte) global
                    // struct folds to a single SDA21 load — `lwz d, g@sda21` — exactly
                    // like a scalar global of the member's type (`displacement_load`
                    // already carries any signed-`char` `extsb`). A larger struct is
                    // ADDR16-addressed, and a non-zero offset materializes g's SDA base
                    // and loads at the displacement (the EMB_SDA21 relocation has no
                    // addend) — both fall through.
                    if offset == 0 && size <= 8 && matches!(self.behavior.global_addressing, GlobalAddressing::SmallData) {
                        self.record_relocation(RelocationKind::EmbSda21, name);
                        self.output.instructions.push(displacement_load(pointee, destination, 0, 0));
                        return Ok(());
                    }
                    self.emit_global_array_base(name, size as u32, destination)?;
                    self.output.instructions.push(displacement_load(pointee, destination, destination, offset as i16));
                    return Ok(());
                }
            }
        }
        // `(*(struct S *)0xADDR).field` — a member through a constant-address pointer. Same
        // idiom as a plain const-address load, with the member offset folded into the
        // displacement (the GX FIFO `(*(PPCWGPipe*)ADDR).u8` is offset 0).
        if let Some(address) = const_address_of(base) {
            if let Some(pointee) = pointee_of_type(member_type) {
                if !matches!(pointee, Pointee::Float | Pointee::Double) {
                    if self.emit_const_address_load(pointee, address, offset, destination)? {
                        return Ok(());
                    }
                    return Err(Diagnostic::error("a constant-address member load needing base reuse is not supported yet (roadmap)"));
                }
            }
        }
        let address = self.member_base_register(base)?;
        let pointee = pointee_of_type(member_type)
            .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
        self.output.instructions.push(displacement_load(pointee, destination, address, offset as i16));
        Ok(())
    }

    /// `array[index].field` for an array/pointer of structs: scale `index` by the
    /// struct `stride`, add to the array base, and load the member at `offset`.
    fn emit_indexed_member_load(&mut self, array: &Expression, index: &Expression, stride: u16, offset: u16, member_type: Type, destination: u8) -> Compilation<()> {
        // `arr[i].field` where `arr` is a file-scope struct array: materialize arr's
        // address with the same interleaved base/scale schedule as a plain global
        // subscript, then load the member at its offset.
        if let Expression::Variable(name) = array {
            if let Some(&total_size) = self.global_array_sizes.get(name.as_str()) {
                return self.emit_global_indexed_member_load(name, total_size, index, stride, offset, member_type, destination);
            }
        }
        let array_register = self.general_register_of_leaf(array)?;
        let index_register = self.general_register_of_leaf(index)?;
        if stride.is_power_of_two() {
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: stride.trailing_zeros() as u8 });
        } else {
            self.output.instructions.push(Instruction::MultiplyImmediate { d: GENERAL_SCRATCH, a: index_register, immediate: stride as i16 });
        }
        let pointee = pointee_of_type(member_type)
            .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
        if offset == 0 {
            self.output.instructions.push(indexed_load(pointee, destination, array_register, GENERAL_SCRATCH));
        } else {
            self.output.instructions.push(Instruction::Add { d: array_register, a: array_register, b: GENERAL_SCRATCH });
            self.output.instructions.push(displacement_load(pointee, destination, array_register, offset as i16));
        }
        Ok(())
    }

    /// `arr[index].field` for a file-scope struct array `arr`: a constant index folds
    /// `index*stride + offset` into the load displacement; a variable index runs the
    /// same base/scale interleave as [`Self::emit_global_array_subscript`] (the scale
    /// goes to the scratch before the base lands in `destination`; a large array's
    /// high half avoids the index register) and ends in `lwzx` (offset 0) or
    /// `add; lwz offset`. Power-of-two struct strides only — a non-power stride needs
    /// `mulli`, whose interleave is a follow-up.
    fn emit_global_indexed_member_load(&mut self, name: &str, total_size: u32, index: &Expression, stride: u16, offset: u16, member_type: Type, destination: u8) -> Compilation<()> {
        let pointee = pointee_of_type(member_type)
            .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
        // The base materializes into `destination` and is then its own load base, so
        // `destination` cannot be the scratch r0.
        if destination == GENERAL_SCRATCH {
            return Err(Diagnostic::error("a global struct-array member into the scratch register is not supported yet (roadmap)"));
        }
        // A constant index folds into the load displacement.
        if let Some(constant) = constant_value(index) {
            let total = constant * stride as i64 + offset as i64;
            let total = i16::try_from(total).map_err(|_| Diagnostic::error("struct-array member offset out of range (roadmap)"))?;
            self.emit_global_array_base(name, total_size, destination)?;
            self.output.instructions.push(displacement_load(pointee, destination, destination, total));
            return Ok(());
        }
        if !stride.is_power_of_two() {
            return Err(Diagnostic::error("a global struct-array member with a non-power-of-two stride is not supported yet (roadmap)"));
        }
        let index_register = self.general_register_of_leaf(index)?;
        let shift = stride.trailing_zeros() as u8;
        let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small {
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 0, immediate: 0 });
        } else {
            let high = if destination != index_register { destination } else { self.free_general_excluding(index_register)? };
            self.emit_address_high(high, name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: high, immediate: 0 });
        }
        if offset == 0 {
            self.output.instructions.push(indexed_load(pointee, destination, destination, GENERAL_SCRATCH));
        } else {
            self.output.instructions.push(Instruction::Add { d: destination, a: destination, b: GENERAL_SCRATCH });
            self.output.instructions.push(displacement_load(pointee, destination, destination, offset as i16));
        }
        Ok(())
    }

    /// `arr[index].field = value` for a file-scope struct array `arr`. A constant
    /// index folds `index*stride + offset` into the store displacement, the base in a
    /// register avoiding the value. A variable index runs the interleaved schedule:
    /// `@ha` into a register avoiding the index (and, for a register value, the value);
    /// the base `addi`s into the index register; a constant value then reuses `@ha`'s
    /// register (free once the base lands), matching mwcc's `lis; slwi; addi; li; …`.
    /// Ends in `stwx` (offset 0) or `add; stw offset`. Power-of-two strides, large
    /// (ADDR16) arrays, register/constant values.
    fn emit_global_indexed_member_store(&mut self, name: &str, total_size: u32, index: &Expression, stride: u16, offset: u16, pointee: Pointee, value: &Expression) -> Compilation<()> {
        if let Some(constant) = constant_value(index) {
            // A constant store value interleaves its `li` between the base's `lis` and
            // `addi` (`lis; li; addi; stw`) — that schedule is not modeled, so defer;
            // a register value (the base materializes whole, then `stw`) is byte-exact.
            if !matches!(value, Expression::Variable(_)) {
                return Err(Diagnostic::error("a global struct-array member store at a constant index needs a register value (roadmap)"));
            }
            let total = i16::try_from(constant * stride as i64 + offset as i64)
                .map_err(|_| Diagnostic::error("struct-array member store offset out of range (roadmap)"))?;
            let base = self.free_register_avoiding(&[value])?;
            let restore = self.reserved.insert(base);
            self.emit_global_array_base(name, total_size, base)?;
            let source = self.place_store_value(value, pointee)?;
            if restore {
                self.reserved.remove(&base);
            }
            self.output.instructions.push(displacement_store(pointee, source, base, total));
            return Ok(());
        }
        if !stride.is_power_of_two() {
            return Err(Diagnostic::error("a global struct-array member store with a non-power-of-two stride is not supported yet (roadmap)"));
        }
        if !matches!(value, Expression::Variable(_)) && constant_value(value).is_none() {
            return Err(Diagnostic::error("a global struct-array member store of a computed value is not supported yet (roadmap)"));
        }
        if self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8 {
            return Err(Diagnostic::error("a small global struct-array member store is not supported yet (roadmap)"));
        }
        let index_register = self.general_register_of_leaf(index)?;
        let shift = stride.trailing_zeros() as u8;
        // `@ha` avoids the index (and the value when it is in a register); the base
        // reuses the index register; a constant value reuses `@ha`'s now-free register.
        let high = if constant_value(value).is_some() {
            self.free_register_avoiding(&[index])?
        } else {
            self.free_register_avoiding(&[index, value])?
        };
        self.emit_address_high(high, name);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
        let source = if let Some(constant) = constant_value(value) {
            self.load_integer_constant(high, constant);
            high
        } else {
            self.general_register_of_leaf(value)?
        };
        if offset == 0 {
            self.output.instructions.push(indexed_store(pointee, source, index_register, GENERAL_SCRATCH));
        } else {
            self.output.instructions.push(Instruction::Add { d: index_register, a: index_register, b: GENERAL_SCRATCH });
            self.output.instructions.push(displacement_store(pointee, source, index_register, offset as i16));
        }
        Ok(())
    }

    /// The pointee size of a leaf pointer variable, when greater than one byte
    /// (so its arithmetic needs scaling). A byte pointer returns `None` — its
    /// arithmetic is a plain add.
    fn scaled_pointer(&self, operand: &Expression) -> Option<u16> {
        if let Expression::Variable(name) = operand {
            if let Some(location) = self.locations.get(name) {
                // A struct pointer scales by the struct's byte size; a scalar pointer
                // by its pointee size (a byte element needs no scaling, so > 1).
                if let Some(stride) = location.stride {
                    return Some(stride);
                }
                let size = location.pointee?.size();
                if size > 1 {
                    return Some(size as u16);
                }
            }
        }
        None
    }

    /// The (register, element size) of a pointer operand for arithmetic: a leaf
    /// pointer wider than a byte, or an array member at offset 0 (which decays to a
    /// pointer in its base register). A byte leaf pointer returns `None` (its
    /// arithmetic is a plain add handled elsewhere); a byte *array* member is
    /// handled here, since it is not a plain leaf.
    fn pointer_arithmetic_base(&mut self, operand: &Expression) -> Compilation<Option<(u8, u16)>> {
        if let Expression::MemberAddress { base, offset: 0, element } = operand {
            let register = self.member_base_register(base)?;
            return Ok(Some((register, u16::from(element.size()))));
        }
        if let Some(size) = self.scaled_pointer(operand) {
            return Ok(Some((self.general_register_of_leaf(operand)?, size)));
        }
        Ok(None)
    }

    /// The register and pointee size of a leaf pointer variable, with no side
    /// effects (just the home register). Used to recognize `ptr - ptr`.
    fn pointer_leaf_register_size(&self, operand: &Expression) -> Option<(u8, u16)> {
        if let Expression::Variable(name) = operand {
            let location = self.locations.get(name)?;
            if let Some(stride) = location.stride {
                return Some((location.register, stride));
            }
            return Some((location.register, location.pointee?.size() as u16));
        }
        None
    }

    /// Try to emit `pointer ± integer` with the integer scaled by the pointee
    /// size. Returns `false` for non-pointer (or byte leaf-pointer) operands.
    fn try_emit_pointer_arithmetic(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        // `ptr - ptr` (same pointee) is the element-count difference: the byte
        // difference (`subf`) divided by the element size — a signed power-of-two
        // divide (`srawi; addze`) for sizes above one byte, just the difference for
        // a byte element.
        if operator == BinaryOperator::Subtract {
            if let (Some((left_register, size)), Some((right_register, right_size))) =
                (self.pointer_leaf_register_size(left), self.pointer_leaf_register_size(right))
            {
                if size == right_size {
                    if !size.is_power_of_two() {
                        // A difference by a non-power-of-two struct stride needs the
                        // magic-number divide mwcc emits; defer rather than mis-scale.
                        return Ok(false);
                    }
                    match size.trailing_zeros() {
                        // byte element: the difference is the element count.
                        0 => self.output.instructions.push(Instruction::SubtractFrom { d: destination, a: right_register, b: left_register }),
                        // 2-byte element: signed divide by 2 (`srwi; add; srawi 1`).
                        1 => {
                            self.output.instructions.push(Instruction::SubtractFrom { d: destination, a: right_register, b: left_register });
                            self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: destination, shift: 31 });
                            self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: destination });
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: destination, s: GENERAL_SCRATCH, shift: 1 });
                        }
                        // larger power-of-two element: signed divide via `srawi; addze`.
                        k => {
                            self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: right_register, b: left_register });
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: k as u8 });
                            self.output.instructions.push(Instruction::AddToZeroExtended { d: destination, a: GENERAL_SCRATCH });
                        }
                    }
                    return Ok(true);
                }
            }
        }
        // Identify the pointer and integer operands (`i + p` is commutative).
        let (pointer_register, size, integer) = if let Some((register, size)) = self.pointer_arithmetic_base(left)? {
            (register, size, right)
        } else if operator == BinaryOperator::Add {
            match self.pointer_arithmetic_base(right)? {
                Some((register, size)) => (register, size, left),
                None => return Ok(false),
            }
        } else {
            return Ok(false);
        };
        // A constant index folds its scaled value into an `addi`.
        if let Some(constant) = constant_value(integer) {
            let scaled = constant * size as i64;
            let immediate = i16::try_from(if operator == BinaryOperator::Subtract { -scaled } else { scaled })
                .map_err(|_| Diagnostic::error("pointer offset out of range (roadmap)"))?;
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: pointer_register, immediate });
            return Ok(true);
        }
        let integer_register = self.general_register_of_leaf(integer)?;
        // Scale the index by the element size: a power-of-two element shifts (`slwi`),
        // any other size (a struct stride like 12) multiplies (`mulli`); a byte element
        // needs neither.
        let scaled_register = if size > 1 {
            if size.is_power_of_two() {
                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: integer_register, shift: size.trailing_zeros() as u8 });
            } else {
                let immediate = i16::try_from(size).map_err(|_| Diagnostic::error("pointer stride out of range (roadmap)"))?;
                self.output.instructions.push(Instruction::MultiplyImmediate { d: GENERAL_SCRATCH, a: integer_register, immediate });
            }
            GENERAL_SCRATCH
        } else {
            integer_register
        };
        match operator {
            BinaryOperator::Add => self.output.instructions.push(Instruction::Add { d: destination, a: pointer_register, b: scaled_register }),
            // `p - i`: `subf d, scaled, p` computes `p - scaled`.
            BinaryOperator::Subtract => self.output.instructions.push(Instruction::SubtractFrom { d: destination, a: scaled_register, b: pointer_register }),
            _ => unreachable!("caller restricts to add/subtract"),
        }
        Ok(true)
    }

    /// Emit `target = value` as an expression: compute `value` into the
    /// destination, store it to `target`, and leave the value in the destination
    /// (so the surrounding expression can use it). Global targets only for now.
    pub(crate) fn emit_assign(&mut self, target: &Expression, value: &Expression, destination: u8) -> Compilation<()> {
        if let Expression::Variable(name) = target {
            if let Some(&global_type) = self.globals.get(name.as_str()) {
                let pointee = pointee_of_type(global_type)
                    .ok_or_else(|| Diagnostic::error("global assignment of this type is not supported yet"))?;
                self.evaluate_general(value, destination)?;
                self.emit_global_store(name, pointee, destination)?;
                return Ok(());
            }
        }
        Err(Diagnostic::error("assignment as an expression supports a global target (roadmap)"))
    }

    /// The register holding a struct pointer for member access. A plain variable
    /// is in its own register; a chained base `a->b` is itself a pointer member, so
    /// its value is loaded into the inner base register (reused) before use.
    pub(crate) fn member_base_register(&mut self, base: &Expression) -> Compilation<u8> {
        match base {
            Expression::Variable(name) => self.general_register_of(name),
            Expression::Member { base: inner, offset, .. } => {
                let register = self.member_base_register(inner)?;
                self.output.instructions.push(Instruction::LoadWord { d: register, a: register, offset: *offset as i16 });
                Ok(register)
            }
            // `((struct S *)x)->field`: a pointer cast is transparent — the base is
            // just the operand's pointer value.
            Expression::Cast { operand, .. } => self.member_base_register(operand),
            _ => Err(Diagnostic::error("struct member base must be a pointer variable (roadmap)")),
        }
    }

    /// Emit `base[index]` into `destination`. A constant index folds into the load
    /// displacement (`lwz r3,8(r3)`); a variable index is scaled by the element
    /// size and uses an indexed load (`slwi r0,rI,2; lwzx r3,rBase,r0`).
    pub(crate) fn emit_subscript(&mut self, base: &Expression, index: &Expression, destination: u8) -> Compilation<()> {
        // `g[index]` where `g` is a file-scope array global: its address is
        // materialized by size (SDA21 small / ADDR16 large), then the element load.
        if let Expression::Variable(name) = base {
            if let Some(&total_size) = self.global_array_sizes.get(name.as_str()) {
                return self.emit_global_array_subscript(name, total_size, index, destination);
            }
        }
        // `base->arr[index]` — the array address (`base + offset`) folds into the
        // subscript: the array offset rides in the load displacement.
        if let Expression::MemberAddress { base: struct_base, offset, element } = base {
            let address = self.member_base_register(struct_base)?;
            if let Some(constant) = constant_value(index) {
                let total = *offset as i64 + constant * element.size() as i64;
                let total = i16::try_from(total).map_err(|_| Diagnostic::error("array subscript out of range (roadmap)"))?;
                self.output.instructions.push(displacement_load(*element, destination, address, total));
                return Ok(());
            }
            let index_register = self.general_register_of_leaf(index)?;
            let size = element.size();
            let scaled = if size == 1 {
                index_register
            } else {
                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: size.trailing_zeros() as u8 });
                GENERAL_SCRATCH
            };
            if *offset == 0 {
                self.output.instructions.push(indexed_load(*element, destination, address, scaled));
            } else {
                self.output.instructions.push(Instruction::Add { d: address, a: address, b: scaled });
                self.output.instructions.push(displacement_load(*element, destination, address, *offset as i16));
            }
            return Ok(());
        }
        let (pointee, address) = self.resolve_pointer(base)?;
        if let Some(constant) = constant_value(index) {
            let offset = constant * pointee.size() as i64;
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("subscript offset out of range (roadmap)"))?;
            self.output.instructions.push(displacement_load(pointee, destination, address, offset));
            return Ok(());
        }
        // `a[i + const]` / `a[i - const]`: scale the variable index, add it to the base, and fold the
        // constant into the load displacement — mwcc emits `slwi r0,i,k; add base,base,r0; lwz d,off(base)`.
        // (A bare variable index below uses `lwzx`, which has no displacement field for the constant.)
        if let Expression::Binary { operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract), left, right } = index {
            if constant_value(left).is_none() {
                if let Some(constant) = constant_value(right) {
                    let signed = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                    let offset = signed * pointee.size() as i64;
                    let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("subscript offset out of range (roadmap)"))?;
                    let index_register = self.general_register_of_leaf(left)?;
                    let size = pointee.size();
                    let scaled = if size == 1 {
                        index_register
                    } else {
                        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: size.trailing_zeros() as u8 });
                        GENERAL_SCRATCH
                    };
                    self.output.instructions.push(Instruction::Add { d: address, a: address, b: scaled });
                    self.output.instructions.push(displacement_load(pointee, destination, address, offset));
                    return Ok(());
                }
            }
        }
        // `a[i * const]`: the constant multiplies the element scale (`a[i*2]` of `int` is `i << 3`).
        // Fold it — a power-of-two total scale uses `slwi`, otherwise `mulli` — then the bare `lwzx`.
        if let Expression::Binary { operator: BinaryOperator::Multiply, left, right } = index {
            let variable_and_factor = if let Some(factor) = constant_value(right) {
                Some((left.as_ref(), factor))
            } else if let Some(factor) = constant_value(left) {
                Some((right.as_ref(), factor))
            } else {
                None
            };
            if let Some((variable, factor)) = variable_and_factor {
                let total = factor * pointee.size() as i64;
                let index_register = self.general_register_of_leaf(variable)?;
                let scaled = if total == 1 {
                    index_register
                } else if total > 1 && (total as u64).is_power_of_two() {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: (total as u64).trailing_zeros() as u8 });
                    GENERAL_SCRATCH
                } else {
                    let total = i16::try_from(total).map_err(|_| Diagnostic::error("subscript scale out of range (roadmap)"))?;
                    self.output.instructions.push(Instruction::MultiplyImmediate { d: GENERAL_SCRATCH, a: index_register, immediate: total });
                    GENERAL_SCRATCH
                };
                self.output.instructions.push(indexed_load(pointee, destination, address, scaled));
                return Ok(());
            }
        }
        let index_register = self.general_register_of_leaf(index)?;
        let size = pointee.size();
        let scaled = if size == 1 {
            index_register
        } else {
            self.output.instructions.push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: index_register,
                shift: size.trailing_zeros() as u8,
            });
            GENERAL_SCRATCH
        };
        self.output.instructions.push(indexed_load(pointee, destination, address, scaled));
        Ok(())
    }

    /// `g[index]` for a file-scope array global `g`: materialize `g`'s base address
    /// into `destination` (SDA21 for a small `.sdata` array, ADDR16 `lis`/`addi` for
    /// a large `.data` one — by total size), then load the element. A constant index
    /// folds into the load displacement; a variable index needs mwcc's scale/base
    /// scheduling interleave, which is not modeled yet, so it defers.
    fn emit_global_array_subscript(&mut self, name: &str, total_size: u32, index: &Expression, destination: u8) -> Compilation<()> {
        let element_type = self.globals[name];
        let pointee = pointee_of_type(element_type)
            .ok_or_else(|| Diagnostic::error("a global array of this element type is not supported yet (roadmap)"))?;
        // The base materializes into `destination` and is then its own load base, so
        // `destination` cannot be the scratch r0 (an `addi`/load based on r0 reads
        // literal zero, not the register).
        if destination == GENERAL_SCRATCH {
            return Err(Diagnostic::error("a global-array subscript into the scratch register is not supported yet (roadmap)"));
        }
        // A constant index folds into the load displacement.
        if let Some(constant) = constant_value(index) {
            let offset = constant * pointee.size() as i64;
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("array subscript out of range (roadmap)"))?;
            // The offset-0 element of a SMALL (SDA21-addressed) array folds to a single direct SDA21
            // load — `lwz d, g@sda21(r0)` — exactly like a scalar global or an offset-0 struct member;
            // mwcc does not materialize the base for `g[0]`. A NON-zero element offset can't fold (an
            // SDA21 relocation carries no addend), so it materializes the base and loads at the
            // displacement; a LARGE array is ADDR16 and always materializes the base.
            let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
            if offset == 0 && small {
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output.instructions.push(displacement_load(pointee, destination, 0, 0));
                return Ok(());
            }
            // A float/double element loads into the FPR `destination` from a GPR base, so the base
            // needs its OWN free GPR (the FPR number cannot be the base register). Materialize it,
            // then the float load: a LARGE offset-0 element folds `@l` into the load
            // (`lis b,g@ha; lfs f,g@l(b)`); every other case materializes the full base
            // (`li b,g@sda21; lfs f,off(b)` small, `lis b,g@ha; addi b,b,g@l; lfs f,off(b)` large).
            if matches!(pointee, Pointee::Float | Pointee::Double) {
                let base = self.free_general_excluding(GENERAL_SCRATCH)?;
                if offset == 0 {
                    // The small offset-0 case folded above, so this is the large ADDR16 element.
                    self.emit_address_high(base, name);
                    self.record_relocation(RelocationKind::Addr16Lo, name);
                    self.output.instructions.push(displacement_load(pointee, destination, base, 0));
                } else {
                    self.emit_global_array_base(name, total_size, base)?;
                    self.output.instructions.push(displacement_load(pointee, destination, base, offset));
                }
                return Ok(());
            }
            self.emit_global_array_base(name, total_size, destination)?;
            self.output.instructions.push(displacement_load(pointee, destination, destination, offset));
            return Ok(());
        }
        // A variable index: scale it, materialize the base, and `lwzx`/`lfsx`. mwcc orders these so
        // the scale runs before the base lands in the base register; for a large array the base's
        // high half goes to a register the scale won't clobber. An INTEGER element's base IS the
        // result register (`destination`). A FLOAT/DOUBLE element loads into the FPR `destination`,
        // whose number cannot be a GPR base — its base is the lowest free GPR (the integer-result
        // register r3, unused by a float function), regardless of which register holds the index
        // (mwcc: `slwi r0,r4,2; lis r3,g@ha; addi r3,r3,g@l; lfsx f1,r3,r0`).
        let size = pointee.size();
        if size == 1 {
            // An unscaled `char` element risks clobbering the index — defer.
            return Err(Diagnostic::error("a variable subscript of a byte global array is not supported yet (roadmap)"));
        }
        let index_register = self.general_register_of_leaf(index)?;
        let shift = size.trailing_zeros() as u8;
        let base_gpr = if matches!(pointee, Pointee::Float | Pointee::Double) { self.free_general_excluding(GENERAL_SCRATCH)? } else { destination };
        let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small {
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate { d: base_gpr, a: 0, immediate: 0 });
        } else {
            // The high half goes to the base register when it does not hold the index; otherwise to
            // a free register the scale will read before it is reused.
            let high = if base_gpr != index_register { base_gpr } else { self.free_general_excluding(index_register)? };
            self.emit_address_high(high, name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: base_gpr, a: high, immediate: 0 });
        }
        self.output.instructions.push(indexed_load(pointee, destination, base_gpr, GENERAL_SCRATCH));
        Ok(())
    }

    /// `&g[index]` for a file-scope array global `g`: the ELEMENT ADDRESS `&g + index*size`
    /// — an address computation (`lis;addi;addi` large / `addi;addi` small), NOT the pointer
    /// arithmetic `load(g)+index` an array-as-pointer read would do. Materialize the base, then
    /// add the scaled constant offset. A variable index (a runtime scale+add of an address) is
    /// not modeled yet, so it defers.
    pub(crate) fn emit_global_array_element_address(&mut self, name: &str, total_size: u32, index: &Expression, destination: u8) -> Compilation<()> {
        let element_type = self.globals[name];
        let pointee = pointee_of_type(element_type)
            .ok_or_else(|| Diagnostic::error("address of a global array of this element type is not supported yet (roadmap)"))?;
        // The base materializes into `destination` and is then its own `addi` base, so it cannot
        // be the scratch r0 (an `addi` based on r0 reads literal zero, not the register).
        if destination == GENERAL_SCRATCH {
            return Err(Diagnostic::error("a global-array element address into the scratch register is not supported yet (roadmap)"));
        }
        let Some(constant) = constant_value(index) else {
            return Err(Diagnostic::error("the address of a variable-indexed global-array element is not supported yet (roadmap)"));
        };
        self.emit_global_array_base(name, total_size, destination)?;
        let offset = constant * pointee.size() as i64;
        if offset != 0 {
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("global-array element address offset out of range (roadmap)"))?;
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: offset });
        }
        Ok(())
    }

    /// `&g.field` where `g` is a file-scope struct VALUE global: the field ADDRESS `&g + offset`
    /// — materialize g's base (SDA21 small / ADDR16 large, by the struct's size) then add the
    /// member offset, the same address computation as `&a[i]`. Not the `load(g)+offset` a struct
    /// POINTER would use — `g` is the struct itself, so its address is taken, not loaded.
    pub(crate) fn emit_global_struct_member_address(&mut self, name: &str, size: u32, offset: u16, destination: u8) -> Compilation<()> {
        // The base materializes into `destination` and is then its own `addi` base, so it cannot
        // be the scratch r0 (an `addi` based on r0 reads literal zero, not the register).
        if destination == GENERAL_SCRATCH {
            return Err(Diagnostic::error("a global struct member address into the scratch register is not supported yet (roadmap)"));
        }
        self.emit_global_array_base(name, size, destination)?;
        if offset != 0 {
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("global struct member address offset out of range (roadmap)"))?;
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: offset });
        }
        Ok(())
    }

    /// Materialize a file-scope array global's base address into `dest` (never r0):
    /// a small (`.sdata`) array via a single SDA21 `addi`; a large (`.data`/`.bss`)
    /// one via `lis dest, name@ha` then `addi dest, dest, name@l`.
    fn emit_global_array_base(&mut self, name: &str, total_size: u32, dest: u8) -> Compilation<()> {
        let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small {
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate { d: dest, a: 0, immediate: 0 });
        } else {
            self.emit_address_high(dest, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: dest, a: dest, immediate: 0 });
        }
        Ok(())
    }

    /// `g[index] = value;` for a file-scope array global `g`. A constant index
    /// materializes the base into a free register (avoiding the value's inputs) and
    /// stores at the element offset. A variable index scales into the scratch, lands
    /// the base in the (now-free) index register, and `stwx`es the value; the large
    /// array's base high half goes to a register that avoids both the index and the
    /// value. A float/double element stores from its FPR through the same GPR base
    /// (`stfs`/`stfd`); the base register comes from the general pool regardless.
    /// Register-valued stores only — byte arrays and computed/constant values are follow-ups.
    fn emit_global_array_store(&mut self, name: &str, total_size: u32, index: &Expression, value: &Expression) -> Compilation<()> {
        let element_type = self.globals[name];
        let pointee = pointee_of_type(element_type)
            .ok_or_else(|| Diagnostic::error("a global array of this element type is not supported yet (roadmap)"))?;
        // A non-register (constant/computed) value is materialized with its own
        // instruction, which mwcc's scheduler interleaves into the base
        // materialization (`lis; li value; addi; stw`) — an ordering not modeled
        // here, so only a register-valued store is byte-exact.
        if !matches!(value, Expression::Variable(_)) {
            return Err(Diagnostic::error("a global-array store of a non-register value is not supported yet (needs the value/base scheduler)"));
        }
        // Constant index: base into a free register (avoiding the value), then a
        // displacement store at the element offset.
        if let Some(constant) = constant_value(index) {
            let offset = constant * pointee.size() as i64;
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("array subscript out of range (roadmap)"))?;
            let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
            // The offset-0 element of a SMALL (SDA21-addressed) array folds to a single direct SDA21
            // store — `stw v, g@sda21(r0)` — like a scalar global; no base register is materialized
            // (mwcc does not materialize the base for `g[0] = v`). A nonzero element offset (below) or
            // a large ADDR16 array keeps the base.
            if offset == 0 && small {
                let source = self.place_store_value(value, pointee)?;
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output.instructions.push(displacement_store(pointee, source, 0, 0));
                return Ok(());
            }
            let base = self.free_register_avoiding(&[value])?;
            let restore = self.reserved.insert(base);
            let large = !small;
            if offset == 0 && large {
                // At a zero offset mwcc folds `@l` into the store rather than
                // materializing the whole base: `lis base,a@ha; stw v,a@l(base)`. (A
                // non-zero offset keeps the `addi` so the literal element offset can
                // ride the store's displacement field instead.)
                self.emit_address_high(base, name);
                let source = self.place_store_value(value, pointee)?;
                if restore {
                    self.reserved.remove(&base);
                }
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(displacement_store(pointee, source, base, 0));
                return Ok(());
            }
            self.emit_global_array_base(name, total_size, base)?;
            let source = self.place_store_value(value, pointee)?;
            if restore {
                self.reserved.remove(&base);
            }
            self.output.instructions.push(displacement_store(pointee, source, base, offset));
            return Ok(());
        }
        // Variable index: the base reuses the (scaled-away) index register and the value stores
        // through it — `stwx`/`stfsx`/`stfdx`. A byte element defers (an unscaled byte index can
        // alias the base register).
        let size = pointee.size();
        if size == 1 {
            return Err(Diagnostic::error("a variable store to a byte global array is not supported yet (roadmap)"));
        }
        // A float/double value lives in an FPR (stored via stfsx/stfdx); an integer in a GPR. The
        // base register is the index register either way — a float value doesn't occupy it.
        let value_register = if matches!(pointee, Pointee::Float | Pointee::Double) {
            self.float_register_of_leaf(value)?
        } else {
            self.general_register_of_leaf(value)?
        };
        let index_register = self.general_register_of_leaf(index)?;
        let shift = size.trailing_zeros() as u8;
        let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small {
            // scale → r0; base (SDA21) → the freed index register; `stwx`.
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: 0, immediate: 0 });
        } else {
            // base high → a register avoiding the index and value; scale; base low
            // into the freed index register; `stwx`.
            let high = self.free_register_avoiding(&[index, value])?;
            self.emit_address_high(high, name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
        }
        self.output.instructions.push(indexed_store(pointee, value_register, index_register, GENERAL_SCRATCH));
        Ok(())
    }

    /// Emit a store: `*p = v;` or `p[i] = v;`. The value goes to memory at the
    /// place addressed by the pointer (with a folded displacement for a constant
    /// index, or a scaled indexed store for a variable one).
    /// `a[i] op= rhs` — a read-modify-write of a variable-index word element with
    /// a leaf right-hand side (`a[i] += x`, `a[i] |= flags`). mwcc scales the
    /// index once into its own register and reuses it for both the indexed load
    /// and store, computing the new value in the scratch:
    /// `slwi r4,i,2; lwzx r0,base,r4; <op> r0,r0,rhs; stwx r0,base,r4`. The scaled
    /// index is a fresh virtual the allocator colors (off the base, rhs and r0).
    /// A constant or computed rhs has a different register shape and is deferred.
    fn try_emit_indexed_rmw(&mut self, target: &Expression, value: &Expression) -> Compilation<bool> {
        use BinaryOperator::*;
        let Expression::Index { base, index } = target else { return Ok(false) };
        if leaf_name(base).is_none() || constant_value(index).is_some() {
            return Ok(false);
        }
        let Expression::Binary { operator, left, right } = value else { return Ok(false) };
        if !matches!(operator, Add | Subtract | BitAnd | BitOr | BitXor | Multiply) {
            return Ok(false);
        }
        // The modified value must read the very same element being stored.
        if !same_operand(target, left) {
            return Ok(false);
        }
        let (pointee, address) = self.resolve_pointer(base)?;
        if !matches!(pointee, Pointee::Int | Pointee::UnsignedInt) {
            return Ok(false);
        }
        let index_register = self.general_register_of_leaf(index)?;
        let size_shift = pointee.size().trailing_zeros() as u8;
        let scratch = GENERAL_SCRATCH;

        // `a[i] op= leaf`: the loaded value flows through the scratch and the op
        // works in place — `slwi r4,i,2; lwzx r0,base,r4; <op> r0,r0,rhs; stwx r0`.
        if let Some(rhs_register) = self.plain_integer_leaf_register(right) {
            let scaled = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: scaled, s: index_register, shift: size_shift });
            self.output.instructions.push(indexed_load(pointee, scratch, address, scaled));
            let combined = match operator {
                Add => Instruction::Add { d: scratch, a: scratch, b: rhs_register },
                Subtract => Instruction::SubtractFrom { d: scratch, a: rhs_register, b: scratch },
                Multiply => Instruction::MultiplyLow { d: scratch, a: scratch, b: rhs_register },
                BitAnd => Instruction::And { a: scratch, s: scratch, b: rhs_register },
                BitOr => Instruction::Or { a: scratch, s: scratch, b: rhs_register },
                _ => Instruction::Xor { a: scratch, s: scratch, b: rhs_register },
            };
            self.output.instructions.push(combined);
            self.output.instructions.push(indexed_store(pointee, scratch, address, scaled));
            return Ok(true);
        }

        // `a[i] += C` / `a[i] -= C` / `a[i]++` (a constant addend that fits an
        // immediate): mwcc loads the value into a register (not the scratch) and
        // the `addi` targets the scratch — `slwi r5,i,2; lwzx r4,base,r5; addi
        // r0,r4,C; stwx r0,base,r5`. Both the scaled index and the loaded value
        // are virtuals; at the `slwi` the index source is still live, so the
        // allocator places the index above the value, reproducing mwcc.
        if matches!(operator, Add | Subtract) {
            let immediate = constant_value(right)
                .and_then(|c| if matches!(operator, Subtract) { c.checked_neg() } else { Some(c) })
                .and_then(|c| i16::try_from(c).ok());
            if let Some(immediate) = immediate {
                // The scaled index avoids the index register so the loaded value
                // (not the index) coalesces onto the now-dead index register —
                // mwcc's `slwi r5,i,2; lwzx r4,…` rather than the reverse.
                let scaled = self.fresh_virtual_general_avoiding(vec![index_register]);
                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: scaled, s: index_register, shift: size_shift });
                let loaded = self.fresh_virtual_general();
                self.output.instructions.push(indexed_load(pointee, loaded, address, scaled));
                self.output.instructions.push(Instruction::AddImmediate { d: scratch, a: loaded, immediate });
                self.output.instructions.push(indexed_store(pointee, scratch, address, scaled));
                return Ok(true);
            }
        }

        // `a[i] |= C` / `^= C` / `&= C` / `*= C`: the loaded value flows through
        // the scratch and the op is an in-place immediate (`ori`/`xori`/`mulli`,
        // or `rlwinm` for a contiguous-mask AND) — the leaf-shape coloring, so the
        // scaled index coalesces onto the dead index register.
        if let Some(constant) = constant_value(right) {
            let immediate_op = match operator {
                BitOr if u16::try_from(constant).is_ok() => Instruction::OrImmediate { a: scratch, s: scratch, immediate: constant as u16 },
                BitXor if u16::try_from(constant).is_ok() => Instruction::XorImmediate { a: scratch, s: scratch, immediate: constant as u16 },
                Multiply if i16::try_from(constant).is_ok() => Instruction::MultiplyImmediate { d: scratch, a: scratch, immediate: constant as i16 },
                BitAnd => match rlwinm_mask(constant) {
                    Some((begin, end)) => Instruction::RotateAndMask { a: scratch, s: scratch, shift: 0, begin, end },
                    None => return Ok(false),
                },
                _ => return Ok(false),
            };
            let scaled = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: scaled, s: index_register, shift: size_shift });
            self.output.instructions.push(indexed_load(pointee, scratch, address, scaled));
            self.output.instructions.push(immediate_op);
            self.output.instructions.push(indexed_store(pointee, scratch, address, scaled));
            return Ok(true);
        }
        Ok(false)
    }

    /// Emit an SDA-global store of a value already evaluated into `source`. The
    /// computed-store-fill path evaluates both values (into a virtual and the scratch)
    /// *before* the stores, so it places the store separately from the value.
    pub(crate) fn emit_sda_global_store_from(&mut self, name: &str, pointee: Pointee, source: u8) {
        self.record_relocation(RelocationKind::EmbSda21, name);
        self.output.instructions.push(displacement_store(pointee, source, 0, 0));
    }

    pub(crate) fn emit_store(&mut self, target: &Expression, value: &Expression) -> Compilation<()> {
        // `*(T *)0xADDR = v` — a constant-address store (memory-mapped registers, the GX FIFO).
        // mwcc materializes the address base before the value (`lis base, hi`), keeping the base
        // GPR clear of the value's inputs, then stores `st value, lo(base)`. Mirrors the absolute
        // global store, with a numeric hi/lo split in place of `@ha`/`@l` relocations.
        if let Expression::Dereference { pointer } = target {
            if let Some((pointee, address)) = const_address_pointer(pointer) {
                if self.emit_const_address_store(pointee, address, 0, value)? {
                    return Ok(());
                }
                return Err(Diagnostic::error("a constant-address store needing base reuse is not supported yet (roadmap)"));
            }
        }
        // `(*(struct S *)0xADDR).field = v` — store to a member of a constant-address pointer.
        // Same idiom as the plain const-address store, with the member offset folded into the
        // displacement (the GX FIFO union store `(*(PPCWGPipe*)ADDR).u8 = v` is offset 0).
        if let Expression::Member { base, offset, member_type, index_stride: None } = target {
            if let Some(address) = const_address_of(base) {
                if let Some(pointee) = pointee_of_type(*member_type) {
                    if !matches!(pointee, Pointee::Float | Pointee::Double) {
                        if self.emit_const_address_store(pointee, address, *offset, value)? {
                            return Ok(());
                        }
                        return Err(Diagnostic::error("a constant-address member store needing base reuse is not supported yet (roadmap)"));
                    }
                }
            }
        }
        // `*(p + i) = v` is `p[i] = v`: rewrite a pointer-plus-index dereference target to the
        // subscript store, the symmetric counterpart of the load routing in
        // emit_load_from_pointer. The pointer operand is the base, the integer the index; `+`
        // commutes. The store truncates a narrow value (stb/sth), so unlike the LOAD this has
        // no sign-extension hazard — the rewritten Index store handles every pointee width.
        if let Expression::Dereference { pointer } = target {
            if let Expression::Binary { operator: BinaryOperator::Add, left, right } = pointer.as_ref() {
                let base_index = if self.dereferenced_width(left).is_some() {
                    Some((left.clone(), right.clone()))
                } else if self.dereferenced_width(right).is_some() {
                    Some((right.clone(), left.clone()))
                } else {
                    None
                };
                if let Some((base, index)) = base_index {
                    return self.emit_store(&Expression::Index { base, index }, value);
                }
            }
            // `*(p - C) = v` is `p[-C] = v` — the subtract counterpart, a constant negative
            // index (subtract does not commute; the pointer is the left operand). The store
            // truncates, so every width is fine.
            if let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = pointer.as_ref() {
                if let Some(constant) = constant_value(right) {
                    if self.dereferenced_width(left).is_some() {
                        let index = Box::new(Expression::IntegerLiteral(-constant));
                        return self.emit_store(&Expression::Index { base: left.clone(), index }, value);
                    }
                }
            }
        }
        // A type-pun store through a frame-resident address (`*(int*)&x = v`) is a
        // plain displacement store to r1.
        if let Expression::Dereference { pointer } = target {
            if let Some((pointee, offset)) = self.resolve_frame_pointer(pointer) {
                let source = self.place_store_value(value, pointee)?;
                self.output.instructions.push(displacement_store(pointee, source, 1, offset));
                return Ok(());
            }
        }
        // `g = v;` — a store to a file-scope global.
        if let Expression::Variable(name) = target {
            if let Some(&global_type) = self.globals.get(name.as_str()) {
                let pointee = pointee_of_type(global_type)
                    .ok_or_else(|| Diagnostic::error("global store of this type is not supported yet"))?;
                match self.behavior.global_addressing {
                    GlobalAddressing::SmallData => {
                        let source = self.place_store_value(value, pointee)?;
                        self.record_relocation(RelocationKind::EmbSda21, name);
                        self.output.instructions.push(displacement_store(pointee, source, 0, 0));
                        // The stored value is still in `source`; a following read of
                        // this global reuses it (mwcc does not reload here).
                        self.stored_globals.insert(name.clone(), (source, self.output.instructions.len()));
                    }
                    GlobalAddressing::Absolute => {
                        // mwcc materializes the address base before the value, so the
                        // base GPR (chosen to avoid the value's input registers) is
                        // reserved while the value is placed.
                        let base = self.free_register_avoiding(&[value])?;
                        let restore = self.reserved.insert(base);
                        self.emit_address_high(base, name);
                        let source = self.place_store_value(value, pointee)?;
                        if restore { self.reserved.remove(&base); }
                        self.record_relocation(RelocationKind::Addr16Lo, name);
                        self.output.instructions.push(displacement_store(pointee, source, base, 0));
                    }
                }
                return Ok(());
            }
        }
        // `g[index] = value;` where `g` is a file-scope array global.
        if let Expression::Index { base, index } = target {
            if let Expression::Variable(name) = base.as_ref() {
                if let Some(&total_size) = self.global_array_sizes.get(name.as_str()) {
                    return self.emit_global_array_store(name, total_size, index, value);
                }
            }
        }
        // `a[i] op= rhs` (variable index, leaf rhs) — scale the index once and
        // reuse it for the indexed load and store, the value flowing through r0.
        if self.try_emit_indexed_rmw(target, value)? {
            return Ok(());
        }
        // `a[i].field = v;` — scale the index by the struct size, then store at the
        // field offset (`stwx` for a zero offset, else `add; stw`). The value is
        // placed after the scale, before the address add — mwcc's order.
        if let Expression::Member { base, offset, member_type, index_stride: Some(stride) } = target {
            if let Expression::Index { base: array, index } = base.as_ref() {
                let pointee = pointee_of_type(*member_type)
                    .ok_or_else(|| Diagnostic::error("struct member store of this type is not supported yet"))?;
                // A file-scope struct array `arr[i].field = v`: materialize the base
                // with the interleaved schedule, then store at the member offset.
                if let Expression::Variable(name) = array.as_ref() {
                    if let Some(&total_size) = self.global_array_sizes.get(name.as_str()) {
                        return self.emit_global_indexed_member_store(name, total_size, index, *stride, *offset, pointee, value);
                    }
                }
                let array_register = self.general_register_of_leaf(array)?;
                let index_register = self.general_register_of_leaf(index)?;
                if stride.is_power_of_two() {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: stride.trailing_zeros() as u8 });
                } else {
                    self.output.instructions.push(Instruction::MultiplyImmediate { d: GENERAL_SCRATCH, a: index_register, immediate: *stride as i16 });
                }
                // The scaled index occupies the scratch (r0), so the value cannot use
                // it: a constant goes in a fresh virtual (the allocator reuses the now
                // free index register, as mwcc does); a variable uses its own register.
                let source = if let Some(constant) = constant_value(value) {
                    let register = self.fresh_virtual_general();
                    self.load_integer_constant(register, constant as i64);
                    register
                } else if matches!(value, Expression::Variable(_)) {
                    self.general_register_of_leaf(value)?
                } else {
                    return Err(Diagnostic::error("indexed-member store of a computed value is not supported yet (roadmap)"));
                };
                if *offset == 0 {
                    self.output.instructions.push(indexed_store(pointee, source, array_register, GENERAL_SCRATCH));
                } else {
                    self.output.instructions.push(Instruction::Add { d: array_register, a: array_register, b: GENERAL_SCRATCH });
                    self.output.instructions.push(displacement_store(pointee, source, array_register, *offset as i16));
                }
                return Ok(());
            }
        }
        // `v.field = x;` where `v` is a frame-resident struct local. A field store
        // is only observable when `&v` is later passed to a call (otherwise mwcc
        // dead-store-eliminates it); but before that call mwcc's scheduler
        // materializes the call-argument address (`addi r3,r1,&v`) as early as the
        // registers free up, interleaving it among the field stores. The
        // frame-resident path emits in source order with no scheduler, so it cannot
        // reproduce that interleave yet — defer until the call-argument scheduler
        // lands. (The matching field LOAD elsewhere has no such ordering hazard.)
        if let Expression::Member { base, index_stride: None, .. } = target {
            if let Expression::Variable(name) = base.as_ref() {
                if self.frame_slots.contains_key(name) {
                    return Err(Diagnostic::error("a frame-struct member store before a call is not supported yet (needs the call-argument scheduler)"));
                }
            }
        }
        // `gp->field = v` / `g.field = v` for a file-scope struct base: materialize
        // the base (a struct POINTER's value, or a struct VALUE's address) into a
        // register chosen to avoid the value's inputs, then a displacement store at
        // the member offset — `lwz/li base; <value>; stw src,offset(base)`.
        if let Expression::Member { base, offset, member_type, index_stride: None } = target {
            if let Expression::Variable(name) = base.as_ref() {
                if !self.locations.contains_key(name.as_str()) {
                    let global_type = self.globals.get(name.as_str()).copied();
                    let struct_value_size = match global_type {
                        Some(Type::StructPointer { .. }) => None,
                        Some(Type::Struct { size, .. }) => Some(size as u32),
                        _ => None,
                    };
                    let is_global_struct_base = matches!(global_type, Some(Type::StructPointer { .. } | Type::Struct { .. }));
                    if is_global_struct_base {
                        let pointee = pointee_of_type(*member_type)
                            .ok_or_else(|| Diagnostic::error("struct member store of this type is not supported yet"))?;
                        // A small (<= 8 byte, SDA-addressed) global struct VALUE:
                        // mwcc materializes the stored VALUE first, then the base. An
                        // offset-0 store folds the SDA21 into the store itself
                        // (`stw src, g@sda21`, no base register), mirroring the offset-0
                        // member load; a non-zero offset materializes g's SDA base and
                        // stores at the displacement.
                        if let Some(size) = struct_value_size {
                            if size <= 8 && matches!(self.behavior.global_addressing, GlobalAddressing::SmallData) {
                                let source = self.place_store_value(value, pointee)?;
                                if *offset == 0 {
                                    self.record_relocation(RelocationKind::EmbSda21, name);
                                    self.output.instructions.push(displacement_store(pointee, source, 0, 0));
                                } else {
                                    let restore = self.reserved.insert(source);
                                    let base_reg = self.free_register_avoiding(&[value])?;
                                    self.emit_global_array_base(name, size, base_reg)?;
                                    if restore {
                                        self.reserved.remove(&source);
                                    }
                                    self.output.instructions.push(displacement_store(pointee, source, base_reg, *offset as i16));
                                }
                                return Ok(());
                            }
                            // A large (ADDR16) global struct VALUE materializes the
                            // base address, then the value, then stores at the offset. A
                            // register value matches mwcc; a *constant* value is a known
                            // latent diff — mwcc folds `@l` into the store and interleaves
                            // the `li` between `lis` and the store (a follow-up).
                            let base_reg = self.free_register_avoiding(&[value])?;
                            let restore = self.reserved.insert(base_reg);
                            self.emit_global_array_base(name, size, base_reg)?;
                            let source = self.place_store_value(value, pointee)?;
                            if restore {
                                self.reserved.remove(&base_reg);
                            }
                            self.output.instructions.push(displacement_store(pointee, source, base_reg, *offset as i16));
                            return Ok(());
                        }
                        // struct POINTER base: load the pointer, then the value, then store.
                        let base_reg = self.free_register_avoiding(&[value])?;
                        let restore = self.reserved.insert(base_reg);
                        self.emit_global_load_value(name, base_reg)?;
                        let source = self.place_store_value(value, pointee)?;
                        if restore {
                            self.reserved.remove(&base_reg);
                        }
                        self.output.instructions.push(displacement_store(pointee, source, base_reg, *offset as i16));
                        return Ok(());
                    }
                }
            }
        }
        // `p->field = v;` — a displacement store to the struct member.
        if let Expression::Member { base, offset, member_type, index_stride: None } = target {
            let pointee = pointee_of_type(*member_type)
                .ok_or_else(|| Diagnostic::error("struct member store of this type is not supported yet"))?;
            let address = self.member_base_register(base)?;
            // The base register is live for the store, so reserve it while the value is
            // placed — otherwise a value that needs a temporary (a magic-number divide)
            // could pick it and clobber the store address.
            let restore = address != GENERAL_SCRATCH && self.reserved.insert(address);
            let source = self.place_store_value(value, pointee)?;
            if restore { self.reserved.remove(&address); }
            self.output.instructions.push(displacement_store(pointee, source, address, *offset as i16));
            return Ok(());
        }
        // `p->arr[index] = value` — store to an array member, folding the array
        // offset into the displacement just like the array load.
        if let Expression::Index { base: index_base, index } = target {
            if let Expression::MemberAddress { base: struct_base, offset, element } = index_base.as_ref() {
                let address = self.member_base_register(struct_base)?;
                if let Some(constant) = constant_value(index) {
                    let total = i16::try_from(*offset as i64 + constant * element.size() as i64)
                        .map_err(|_| Diagnostic::error("array store out of range (roadmap)"))?;
                    let source = self.place_store_value(value, *element)?;
                    self.output.instructions.push(displacement_store(*element, source, address, total));
                    return Ok(());
                }
                if !matches!(value, Expression::Variable(_)) {
                    return Err(Diagnostic::error("array store with a variable index needs a simple value (roadmap)"));
                }
                let source = self.place_store_value(value, *element)?;
                let index_register = self.general_register_of_leaf(index)?;
                let size = element.size();
                let scaled = if size == 1 {
                    index_register
                } else {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: size.trailing_zeros() as u8 });
                    GENERAL_SCRATCH
                };
                if *offset == 0 {
                    self.output.instructions.push(indexed_store(*element, source, address, scaled));
                } else {
                    self.output.instructions.push(Instruction::Add { d: address, a: address, b: scaled });
                    self.output.instructions.push(displacement_store(*element, source, address, *offset as i16));
                }
                return Ok(());
            }
        }
        let (base, index) = match target {
            Expression::Dereference { pointer } => (pointer.as_ref(), None),
            Expression::Index { base, index } => (base.as_ref(), Some(index.as_ref())),
            _ => return Err(Diagnostic::error("store target must be `*p`, `p[i]`, a member, or a global")),
        };
        let (pointee, address) = self.resolve_pointer(base)?;
        // The address register is live for the store; reserve it while the value is
        // placed so a value needing a temporary (e.g. a magic-number divide) can't pick
        // it and clobber the store address.
        let restore = address != GENERAL_SCRATCH && self.reserved.insert(address);
        match index {
            None => {
                let source = self.place_store_value(value, pointee)?;
                if restore { self.reserved.remove(&address); }
                self.output.instructions.push(displacement_store(pointee, source, address, 0));
            }
            Some(index) if constant_value(index).is_some() => {
                let offset = i16::try_from(constant_value(index).unwrap() * pointee.size() as i64)
                    .map_err(|_| Diagnostic::error("store offset out of range (roadmap)"))?;
                let source = self.place_store_value(value, pointee)?;
                if restore { self.reserved.remove(&address); }
                self.output.instructions.push(displacement_store(pointee, source, address, offset));
            }
            Some(index) => {
                // A variable index uses the scratch for scaling, so the value must
                // be a leaf (it stays in its own register) — no temporary, so release
                // the address reservation up front.
                if restore { self.reserved.remove(&address); }
                if !matches!(value, Expression::Variable(_)) {
                    return Err(Diagnostic::error("store with a variable index needs a simple value (roadmap)"));
                }
                let source = self.place_store_value(value, pointee)?;
                // `a[i + const] = v` / `a[i - const] = v`: scale the variable index, add it to the base,
                // and fold the constant into the store displacement (`slwi r0,i,k; add a,a,r0; stw v,off(a)`).
                if let Expression::Binary { operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract), left, right } = index {
                    if constant_value(left).is_none() {
                        if let Some(constant) = constant_value(right) {
                            let signed = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                            let offset = i16::try_from(signed * pointee.size() as i64).map_err(|_| Diagnostic::error("store offset out of range (roadmap)"))?;
                            let index_register = self.general_register_of_leaf(left)?;
                            let size = pointee.size();
                            let scaled = if size == 1 {
                                index_register
                            } else {
                                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: size.trailing_zeros() as u8 });
                                GENERAL_SCRATCH
                            };
                            self.output.instructions.push(Instruction::Add { d: address, a: address, b: scaled });
                            self.output.instructions.push(displacement_store(pointee, source, address, offset));
                            return Ok(());
                        }
                    }
                }
                let index_register = self.general_register_of_leaf(index)?;
                let size = pointee.size();
                let scaled = if size == 1 {
                    index_register
                } else {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: index_register,
                        shift: size.trailing_zeros() as u8,
                    });
                    GENERAL_SCRATCH
                };
                self.output.instructions.push(indexed_store(pointee, source, address, scaled));
            }
        }
        Ok(())
    }

    /// The register holding the value to store: a leaf stays in its own register,
    /// anything else is computed into the scratch (`li r0,0; stw r0,…`,
    /// `add r0,…; stw r0,…`) ahead of the store.
    /// Whether `expression` is `&global` — the address of a data global (not a
    /// frame-resident local). Used to defer the not-yet-scaled `&global +/- n`.
    fn is_global_address_of(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::AddressOf { operand }
            if matches!(operand.as_ref(), Expression::Variable(name)
                if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str())))
    }

    /// Whether `expression` is `&global +/- n` — the global-address pointer arithmetic
    /// that materializes as `li rD,0; addi rD,rD,k`.
    fn is_global_address_arithmetic(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
            if self.is_global_address_of(left) || self.is_global_address_of(right))
    }

    /// Emit a comma-operator's discarded left operand for its side effects only: a call
    /// or assignment is emitted, a side-effect-free leaf/literal emits nothing, a nested
    /// comma recurses. A side effect in a form not modeled here defers rather than
    /// silently dropping it.
    pub(crate) fn emit_comma_side_effect(&mut self, expression: &Expression) -> Compilation<()> {
        // A call in the discarded left operand clobbers the caller-saved register holding
        // the comma's surviving right value (`gi = (h(), b)` would store h()'s result, not
        // b). Preserving it needs the callee-saved allocator, so defer over miscompiling.
        if expression_has_call(expression) {
            return Err(Diagnostic::error("a comma-operator call side effect is not supported yet (needs the callee-saved allocator)"));
        }
        match expression {
            Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_) => Ok(()),
            Expression::Comma { left, right } => {
                self.emit_comma_side_effect(left)?;
                self.emit_comma_side_effect(right)
            }
            // A simple `name = leaf/const` store is a single instruction that never
            // reorders against the comma's surviving store. An indexed/member target or a
            // computed value schedules ambiguously against it (mwcc reorders), so defer.
            Expression::Assign { target, value }
                if matches!(target.as_ref(), Expression::Variable(_))
                    && matches!(value.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_)) =>
            {
                self.emit_store(target, value)
            }
            _ => Err(Diagnostic::error("a comma-operator side effect of this form is not supported yet (roadmap)")),
        }
    }

    /// The register of the leaf at the end of a chained assignment's value, walking
    /// through nested `=`. `None` for a computed or non-leaf value (which flows through
    /// the scratch normally). Used to store the same source register to every target.
    fn innermost_assigned_leaf(&self, value: &Expression) -> Option<u8> {
        match value {
            Expression::Assign { value, .. } => self.innermost_assigned_leaf(value),
            Expression::Variable(name) => self.lookup_general(name),
            _ => None,
        }
    }

    fn place_store_value(&mut self, value: &Expression, pointee: Pointee) -> Compilation<u8> {
        // A comma-operator value: emit the left's side effects, then store the right,
        // which keeps its own register — `gi = (a, b)` is `stw b,gi`, no scratch move.
        if let Expression::Comma { left, right } = value {
            self.emit_comma_side_effect(left)?;
            return self.place_store_value(right, pointee);
        }
        // A constant pre-materialized into a fixed register (a distinct-constant
        // store run) reuses that register instead of re-materializing.
        if let Some(constant) = constant_value(value) {
            if let Some(&(_, register)) = self.prematerialized_constants.iter().find(|(c, _)| *c == constant as i32) {
                return Ok(register);
            }
        }
        // During a constant-store-fill run, a constant value reuses the scratch
        // register when it already holds that constant (mwcc materializes a
        // repeated store value once: `li r0,0; stw; stw; stw`). The run guarantees
        // nothing clobbers the scratch between stores, so this is provably valid.
        if self.reuse_scratch_constant {
            if let Some(constant) = constant_value(value) {
                let constant = constant as i32;
                if self.scratch_constant != Some(constant) {
                    self.load_integer_constant(GENERAL_SCRATCH, constant as i64);
                    self.scratch_constant = Some(constant);
                }
                return Ok(GENERAL_SCRATCH);
            }
        }
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            if let Expression::Variable(name) = value {
                // A float parameter/local lives in a register; a float global is not in
                // `locations`, so it falls through to the general float evaluator, which
                // loads it (`lfs`) into the scratch — `gf = gg` is `lfs f0,gg; stfs f0,gf`.
                if self.locations.contains_key(name.as_str()) {
                    return self.float_register_of_leaf(value);
                }
            }
            // A float call result lands in the float return register (f1); store from there
            // directly rather than moving it to f0 first (mwcc emits no `fmr f0,f1`).
            // The store-only LR-reload-hoist barrier keeps the reload after the stfs. An
            // INTEGER-returning call stored to a float global needs an int->float conversion
            // of its r3 result (not yet modeled), so defer rather than mis-store r3 as f1.
            if let Expression::Call { name, arguments } = value {
                if !matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)) {
                    return Err(Diagnostic::error("an integer call result stored to a float global needs an int->float conversion (roadmap)"));
                }
                let result = Eabi::float_result().number;
                self.emit_call(name, arguments, Some(result), true)?;
                return Ok(result);
            }
            self.evaluate_float(value, FLOAT_SCRATCH)?;
            return Ok(FLOAT_SCRATCH);
        }
        // A float VALUE stored to a NON-float (integer) target — `int g; g = *p;` with a
        // float `*p`, or `g = s->fx` — needs a float->int conversion (fctiwz + frame bounce)
        // of the loaded value before the integer store. A float leaf converts in place via
        // the cast path below; a non-leaf float load is not wired, so defer rather than load
        // it as a float and store an integer register that never received the conversion.
        if self.is_float_value(value) && !self.is_float_leaf(value) {
            return Err(Diagnostic::error("a non-leaf float value stored to an integer target needs a float->int conversion (roadmap)"));
        }
        // A NARROW value (char/short parameter, or a narrow memory load) stored to a wider
        // INTEGER target must be widened first — `int gi; char a; gi = a;` is `extsb r0,r3;
        // stw r0,gi` (or `extsb r3,r3` in place when the value is also returned). mwcc picks
        // r0 vs r3 by whether the value is reused, an allocator decision not modeled here, so
        // defer rather than store the raw narrow value (a miscompile: the byte/halfword is
        // stored without the int sign/zero-extension). A signed-narrow GLOBAL source already
        // extends on load, so it is excluded (only params/locals and narrow loads defer).
        if matches!(pointee, Pointee::Int | Pointee::UnsignedInt) {
            let value_is_narrow = match value {
                Expression::Variable(name) if self.locations.contains_key(name.as_str()) => {
                    self.leaf_info(value).is_ok_and(|(_, width, _)| width < 32)
                }
                Expression::Dereference { pointer } => {
                    matches!(self.pointee_of(pointer), Ok(Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort))
                }
                Expression::Index { base, .. } => {
                    matches!(self.pointee_of(base), Ok(Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort))
                }
                Expression::Member { member_type, .. } => {
                    matches!(member_type, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
                }
                _ => false,
            };
            if value_is_narrow {
                return Err(Diagnostic::error("a narrow value stored to a wider integer target needs a widening coercion (roadmap)"));
            }
        }
        if let Expression::Variable(name) = value {
            // A bare identifier that is neither a local nor a known data global is
            // an external symbol (a function, typically) — store its *address*. mwcc
            // materializes it absolutely (`lis t,sym@ha; addi r0,t,sym@lo`) even with
            // small-data on, since functions are not in the small-data area.
            if !self.locations.contains_key(name) && !self.globals.contains_key(name.as_str()) {
                let high = self.fresh_virtual_general();
                self.emit_address_high(high, name);
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: high, immediate: 0 });
                return Ok(GENERAL_SCRATCH);
            }
            // A data GLOBAL value is loaded into the scratch — `gi = gj` is `lwz r0,gj; stw r0,gi`
            // — since a global is not held in a register like a parameter or local. A NARROW store
            // target truncates, so a signed-narrow global source is read RAW under the truncation
            // context (`char gc,hc; gc = hc;` -> `lbz r0,hc; stb r0,gc`, no redundant `extsb` — mwcc
            // drops it), like the `var op const` narrow-store path below.
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()) {
                let saved = self.narrow_truncation_context;
                if matches!(pointee, Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort) {
                    self.narrow_truncation_context = true;
                }
                let evaluated = self.evaluate_general(value, GENERAL_SCRATCH);
                self.narrow_truncation_context = saved;
                evaluated?;
                return Ok(GENERAL_SCRATCH);
            }
            return self.general_register_of_leaf(value);
        }
        // A chained assignment `g = h = a` stores the same source to each target. Emit
        // the inner store, then yield the source register directly so the outer store
        // reuses it (`stw r3,h; stw r3,g`) instead of staging it through the scratch
        // (`mr r0,r3; stw r0; stw r0`). Only when the ultimate assigned value is a leaf;
        // a computed value (`g = h = a+b`) already flows through the scratch as mwcc does.
        if let Expression::Assign { target, value: inner } = value {
            if let Some(register) = self.innermost_assigned_leaf(inner) {
                self.emit_store(target, inner)?;
                return Ok(register);
            }
        }
        // A narrowing integer cast `(short)x`/`(char)x` whose store truncates to the
        // same or fewer bits (`sth`/`stb`): the cast's sign/zero extension is redundant
        // — mwcc stores the low bits directly. A float leaf still converts (fctiwz) but
        // does not narrow (cast to `int`, width 32, skips the `emit_widen`); an integer
        // leaf stores straight from its own register. Wider stores keep the extension
        // (`gi = (short)a` genuinely sign-extends), and non-leaf operands fall through
        // to the cast's own path (still a redundant extension, but never a miscompile).
        if let Expression::Cast { target_type, operand } = value {
            if target_type.width() < 32 && pointee.element().width() <= target_type.width() {
                // An integer leaf stores straight from its own register (no scratch move).
                if matches!(operand.as_ref(), Expression::Variable(name) if self.lookup_general(name).is_some()) {
                    return self.place_store_value(operand, pointee);
                }
                // Otherwise convert to int width (32, so emit_widen is skipped) into the
                // scratch and let the store truncate: a float leaf does fctiwz, an integer
                // arithmetic expression evaluates, a float-arithmetic or non-leaf-float
                // operand defers. A call is left to the normal path — distinguishing an
                // int- from a float-returning call needs the return-type plumbing.
                if !matches!(operand.as_ref(), Expression::Call { .. }) {
                    self.emit_cast_to_integer(Type::Int, operand, GENERAL_SCRATCH)?;
                    return Ok(GENERAL_SCRATCH);
                }
            }
        }
        // A call result lands in the general return register (r3); store from there
        // directly rather than moving it to the scratch first (mwcc emits no `mr r0,r3`).
        if let Expression::Call { name, arguments } = value {
            let result = Eabi::general_result().number;
            self.emit_call(name, arguments, Some(result), false)?;
            return Ok(result);
        }
        // A `cond ? b : c` select with two non-constant leaf arms lands in the false
        // arm's register (the general branch-select path); mwcc stores from there
        // directly — `cmpwi; beq; mr c,b; stw c` — rather than moving it to the scratch
        // first. Pass that register as the select's destination so no redundant
        // `mr r0,c` is emitted, then store from it. (Constant or zero arms take the
        // branch/mask forms, which already land in the requested destination.)
        if let Expression::Conditional { condition, when_true, when_false } = value {
            if leaf_name(when_true).is_some() && leaf_name(when_false).is_some()
                && constant_value(when_true).is_none() && constant_value(when_false).is_none()
            {
                let false_register = self.general_register_of_leaf(when_false)?;
                self.emit_conditional(condition, when_true, when_false, false_register, false)?;
                return Ok(false_register);
            }
        }
        // A truncation-safe `var op constant` (`+ - | ^ * &`) stored to a NARROW target
        // re-truncates through the store (`stb`/`sth`), so a signed-char operand is read raw —
        // `char gc; gc += 1;` is `lbz r3; addi r0,r3,1; stb r0`, no `extsb` (the byte store
        // drops the high bits mwcc would otherwise sign-extend into). Mirror the narrow-return
        // truncation: read raw under the flag and let the store narrow. The operator set
        // excludes div/mod/shift-right (the sign genuinely matters); shift-left is already
        // byte-exact. BitAnd IS included (unlike the return path, which does a trailing
        // emit_widen the `clrlwi` would make redundant — the store has no such widen).
        let narrow_store_truncates = matches!(
            pointee,
            Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort
        ) && matches!(value, Expression::Binary { operator, left, right }
            if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitXor | BinaryOperator::Multiply | BinaryOperator::BitAnd)
                && matches!(left.as_ref(), Expression::Variable(_))
                && matches!(right.as_ref(), Expression::IntegerLiteral(_)));
        let saved_truncation_context = self.narrow_truncation_context;
        if narrow_store_truncates {
            self.narrow_truncation_context = true;
        }
        let evaluated = self.evaluate_general(value, GENERAL_SCRATCH);
        self.narrow_truncation_context = saved_truncation_context;
        evaluated?;
        Ok(GENERAL_SCRATCH)
    }

    /// Emit a direct call. Arguments are placed in the EABI argument registers,
    /// then `bl name`; the result (in r3 / f1) is moved to `destination` when one
    /// is wanted (a discarded call statement passes `None`).
    pub(crate) fn emit_call(&mut self, name: &str, arguments: &[Expression], destination: Option<u8>, float_result: bool) -> Compilation<()> {
        // An indirect call through a function-pointer variable (a parameter/local held in
        // a register): copy it to r12 before the arguments (which would overwrite its
        // register), then `mtctr r12; bctrl`. A named function is the direct `bl` below.
        if let Some(pointer_register) = self.locations.get(name).map(|location| location.register) {
            self.output.instructions.push(Instruction::Or { a: 12, s: pointer_register, b: pointer_register });
            self.emit_arguments(arguments, name)?;
            self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
            self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
            if let Some(destination) = destination {
                let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
                if destination != result {
                    self.output.instructions.push(if float_result {
                        Instruction::FloatMove { d: destination, b: result }
                    } else {
                        Instruction::move_register(destination, result)
                    });
                }
            }
            return Ok(());
        }
        // An indirect call through a GLOBAL function pointer: the pointer lives in
        // memory, so loading it into r12 doesn't clobber the argument registers — set up
        // the arguments, load the pointer, then `mtctr r12; bctrl`. (The saved-LR store
        // stays in the prologue here, since no `mr r12` setup precedes it.)
        if self.globals.contains_key(name) {
            self.emit_arguments(arguments, name)?;
            self.emit_global_load_value(name, 12)?;
            self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
            self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
            if let Some(destination) = destination {
                let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
                if destination != result {
                    self.output.instructions.push(if float_result {
                        Instruction::FloatMove { d: destination, b: result }
                    } else {
                        Instruction::move_register(destination, result)
                    });
                }
            }
            return Ok(());
        }
        self.emit_arguments(arguments, name)?;
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink { target: name.to_string() });
        if let Some(destination) = destination {
            let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
            if destination != result {
                self.output.instructions.push(if float_result {
                    Instruction::FloatMove { d: destination, b: result }
                } else {
                    Instruction::move_register(destination, result)
                });
            }
        }
        Ok(())
    }

    /// Place call arguments in the EABI argument registers (r3.. / f1..). Each is
    /// evaluated into its positional register; passthrough parameters are already
    /// in place, so this is a no-op for them.
    fn emit_arguments(&mut self, arguments: &[Expression], name: &str) -> Compilation<()> {
        // A CALL in a non-first argument clobbers the argument registers already holding earlier
        // arguments (a call returns in r3 and clobbers r3–r12), and its own result lands in r3 rather
        // than the argument's positional register. mwcc evaluates such arguments RIGHT-first, preserving
        // the earlier results in callee-saved registers — a schedule not modeled here. Evaluating them
        // left-to-right would overwrite the earlier arguments (`s(5, f())`, `s(f(), g())`), so defer.
        // (A call in the FIRST argument alone is fine: later constant/in-place arguments do not clobber
        // its r3 result, e.g. `s(f(), 5)`.)
        if arguments.iter().skip(1).any(expression_has_call) {
            return Err(Diagnostic::error("a call in a non-first argument needs the callee-saved argument scheduler (roadmap)"));
        }
        // The SAME global read in two argument positions loads once in mwcc, which copies it to the
        // second register (`lwz r3,g; mr r4,r3`); our per-argument evaluation loads it in each — wrong
        // bytes. Defer a global variable that appears as two arguments. (A register-resident parameter
        // passed twice is a free re-read and stays byte-exact; two DIFFERENT globals load independently.)
        for (index, argument) in arguments.iter().enumerate() {
            if let Expression::Variable(name) = argument {
                if self.globals.contains_key(name.as_str())
                    && arguments[index + 1..].iter().any(|other| matches!(other, Expression::Variable(other_name) if other_name == name))
                {
                    return Err(Diagnostic::error("the same global read in two arguments needs load-once reuse (roadmap)"));
                }
            }
        }
        // A CONSTANT argument that follows a GLOBAL-LOAD argument: mwcc hoists the constant's `li` into
        // the mflr->LR-store latency slot of the non-leaf prologue (ahead of the global load), a
        // schedule our left-to-right emission (load, then `li`) does not reproduce. Defer. (A constant
        // BEFORE the global load — `s(5, gi)` — is already early and stays byte-exact.)
        {
            let mut seen_global_load = false;
            for argument in arguments {
                match argument {
                    Expression::Variable(name) if self.globals.contains_key(name.as_str()) => seen_global_load = true,
                    Expression::IntegerLiteral(_) if seen_global_load => {
                        return Err(Diagnostic::error("a constant argument after a global load needs the LR-store-latency schedule (roadmap)"));
                    }
                    _ => {}
                }
            }
        }
        // A `&global + n` argument materializes as `li rD,0; addi rD,rD,k`. Alongside
        // other arguments mwcc reorders the leading `li`s (the offset arg's base first)
        // in a way not yet modeled, so defer rather than mis-schedule. A lone such
        // argument is fine (the single-`li` hoist matches).
        if arguments.len() >= 2 && arguments.iter().any(|argument| self.is_global_address_arithmetic(argument)) {
            return Err(Diagnostic::error("a `&global + n` argument alongside others needs the multi-arg schedule (roadmap)"));
        }
        // Two word members of one pointer base, where loading the first clobbers the
        // base register (`g(p->a, p->b)` with `p` in r3): mwcc pre-copies the base to
        // the second argument register, then loads each member —
        // `mr r4,r3; lwz r3,off0(r3); lwz r4,off1(r4)`. The pre-copy `mr` is hoisted
        // into the non-leaf prologue slot by the body emitter. (The general N-member
        // / mixed-width choreography is the allocator's; this handles the 2-word case.)
        if let [Expression::Member { base: base0, offset: offset0, member_type: type0, index_stride: None },
                Expression::Member { base: base1, offset: offset1, member_type: type1, index_stride: None }] = arguments
        {
            if let (Expression::Variable(pointer0), Expression::Variable(pointer1)) = (base0.as_ref(), base1.as_ref()) {
                let base_register = Eabi::FIRST_GENERAL_ARGUMENT;
                let copy_register = base_register + 1;
                let is_word = |member: Type| matches!(member, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. });
                if pointer0 == pointer1
                    && is_word(*type0)
                    && is_word(*type1)
                    && self.locations.get(pointer0.as_str()).map(|location| location.register) == Some(base_register)
                {
                    if let (Some(pointee0), Some(pointee1)) = (pointee_of_type(*type0), pointee_of_type(*type1)) {
                        self.output.instructions.push(Instruction::move_register(copy_register, base_register));
                        self.output.instructions.push(displacement_load(pointee0, base_register, base_register, *offset0 as i16));
                        self.output.instructions.push(displacement_load(pointee1, copy_register, copy_register, *offset1 as i16));
                        return Ok(());
                    }
                }
            }
        }
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for (index, argument) in arguments.iter().enumerate() {
            // A call argument whose float-ness does not match the parameter's needs an
            // int<->float conversion at the call site (the int->float magic-constant
            // sequence, or fctiwz). That conversion is not modeled, so defer rather than
            // place the argument in the wrong register file — passing an integer in r3 to a
            // float parameter that reads f1 (or vice versa) is a miscompile. A parameterless
            // / variadic position (no recorded type) keeps the argument-driven placement.
            if let Some(parameter_type) = self.call_parameter_types.get(name).and_then(|types| types.get(index)) {
                if matches!(parameter_type, Type::Float | Type::Double) != self.is_float_value(argument) {
                    return Err(Diagnostic::error("a call argument needs an int<->float conversion to match the parameter type (roadmap)"));
                }
            }
            if self.is_float_value(argument) {
                self.evaluate_float(argument, next_float)?;
                next_float += 1;
            } else {
                // A narrow (char/short) argument to a parameter that is NOT wider is passed
                // WITHOUT the int promotion — `void g(char); g(char_a)` is just `bl g`, no
                // `extsb` (only a wider parameter, e.g. `void g(int)`, widens the argument).
                // Handled for the in-place case (the value already sits in the argument
                // register); a move or a non-leaf falls through to the widening eval.
                if let Some(parameter_type) = self.call_parameter_types.get(name).and_then(|types| types.get(index)) {
                    if let Ok((register, width, _)) = self.leaf_info(argument) {
                        if width < 32 && (parameter_type.width() as u32) <= width as u32 && register == next_general {
                            next_general += 1;
                            continue;
                        }
                    }
                }
                // An argument WIDER than a narrow (char/short) parameter must be narrowed to
                // the parameter type — `void g(char); g(int_a)` is `extsb r3,r3; bl g` (the C
                // conversion to `(char)`). That narrowing is not modeled, and mwcc schedules
                // the `extsb` into the non-leaf prologue (keystone), so defer rather than pass
                // the wide value un-narrowed: `g(256)` to a `char` parameter must pass 0, not
                // 256 (a miscompile). A constant is materialized in range; a narrow leaf /
                // load / global already fits and is handled by the passthrough above.
                if let Some(parameter_type) = self.call_parameter_types.get(name).and_then(|types| types.get(index)) {
                    if (parameter_type.width() as u32) < 32 && constant_value(argument).is_none() {
                        let argument_is_narrow = match argument {
                            Expression::Variable(variable) if self.locations.contains_key(variable.as_str()) => {
                                self.leaf_info(argument).map(|(_, width, _)| width < 32).unwrap_or(false)
                            }
                            Expression::Variable(variable) => self.globals.get(variable.as_str()).map(|global| global.width() < 32).unwrap_or(false),
                            Expression::Dereference { pointer } => self.dereferenced_width(pointer).is_some_and(|width| width < 32),
                            Expression::Index { base, .. } => self.dereferenced_width(base).is_some_and(|width| width < 32),
                            Expression::Member { member_type, .. } => member_type.width() < 32,
                            _ => false,
                        };
                        if !argument_is_narrow {
                            return Err(Diagnostic::error("an argument wider than a narrow parameter needs a narrowing conversion (roadmap)"));
                        }
                    }
                }
                // Honest guard: evaluating into this argument register must not
                // clobber a register a later argument still needs. mwcc handles
                // that (e.g. two members of one struct) by pre-copying the shared
                // base; that choreography is not modeled yet.
                //
                // A passthrough reuse like `f(x, x)` writes nothing for arg0, and
                // the single trailing `mr r4,r3` it produces is now hoisted into the
                // prologue slot — so the two-argument case is byte-exact. But three+
                // arguments (multiple trailing moves) or a computed trailing argument
                // need the full argument scheduler, so this still defers for now to
                // avoid emitting their unscheduled form.
                // A leaf argument already in its target register is a passthrough — no evaluation, so
                // it clobbers nothing and stays live for a later repeat's `mr` (`g(a, a)` is a in r3,
                // then `mr r4,r3`, the pre-copy hoisted into the prologue slot). Only for a 2-argument
                // call: 3+ arguments produce multiple trailing moves that need the full argument
                // scheduler, so those still defer via the clobber guard below.
                let passthrough_in_place = arguments.len() == 2
                    && self.leaf_info(argument).map(|(register, _, _)| register == next_general).unwrap_or(false);
                if !passthrough_in_place
                    && arguments[index + 1..].iter().any(|later| self.registers_used_by(later).contains(&next_general))
                {
                    return Err(Diagnostic::error("argument would clobber a register a later argument needs (roadmap)"));
                }
                self.evaluate_general(argument, next_general)?;
                next_general += 1;
            }
        }
        Ok(())
    }

    /// Whether an expression yields a float (a float leaf, literal, or load).
    pub(crate) fn is_float_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::FloatLiteral(_) => true,
            Expression::Variable(_) => self.is_float_leaf(expression),
            Expression::Dereference { pointer } => matches!(self.pointee_of(pointer), Ok(Pointee::Float | Pointee::Double)),
            Expression::Index { base, .. } => {
                // A pointer/array element whose pointee is float/double — OR an element of a
                // file-scope float/double array (whose base is not in `locations`, so `pointee_of`
                // can't classify it; consult the global's element type directly).
                matches!(self.pointee_of(base), Ok(Pointee::Float | Pointee::Double))
                    || matches!(base.as_ref(), Expression::Variable(name)
                        if self.global_array_sizes.contains_key(name.as_str())
                            && matches!(self.globals.get(name.as_str()), Some(Type::Float | Type::Double)))
            }
            Expression::Member { member_type, .. } => *member_type == Type::Float,
            _ => false,
        }
    }

    /// Load a file-scope global into `destination`. Under small-data addressing a
    /// single instruction carries the `0(r0)` placeholder an `R_PPC_EMB_SDA21`
    /// relocation fills (r13 + the small-data offset); under absolute addressing
    /// (`-sdata 0`) the address is materialized with a `lis`/`addi` pair (see
    /// [`Self::emit_global_load_absolute`]). The load is chosen by the global's type.
    pub(crate) fn emit_global_load(&mut self, name: &str, destination: u8) -> Compilation<()> {
        self.emit_global_load_value(name, destination)?;
        // A signed `char` global promotes to int with a trailing sign-extension:
        // `lbz` zero-extends the byte, so the value must be re-signed (`extsb`). In a
        // truncation context (the consumer re-narrows the result — a narrow return or a
        // narrow store of a truncation-safe op) the extsb is redundant and mwcc omits it:
        // `gc += 1` is `lbz r3; addi r0,r3,1; stb r0`, the byte store dropping the high bits.
        if self.global_char_extend(name)? && !self.narrow_truncation_context {
            self.emit_widen(destination, destination, 8, true);
        }
        Ok(())
    }

    /// Load a global's value *without* the signed-char promotion — just the
    /// addressing sequence and the load. The two-narrow-global path loads both
    /// operands before extending either, matching mwcc's batched schedule, so it
    /// drives the load and the extension separately through this and
    /// [`Self::global_char_extend`].
    pub(crate) fn emit_global_load_value(&mut self, name: &str, destination: u8) -> Compilation<()> {
        let global_type = *self.globals.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                self.record_relocation(RelocationKind::EmbSda21, name);
                let instruction = self.global_load_instruction(global_type, destination, 0)?;
                self.output.instructions.push(instruction);
            }
            GlobalAddressing::Absolute => self.emit_global_load_absolute(name, global_type, destination)?,
        }
        Ok(())
    }

    /// Whether reading global `name` needs a trailing `extsb` — a signed plain
    /// `char` (unsigned char and the self-extending half/word loads need none).
    pub(crate) fn global_char_extend(&self, name: &str) -> Compilation<bool> {
        let global_type = *self.globals.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        Ok(global_type == Type::Char && self.behavior.char_is_signed)
    }

    /// The type-appropriate load of a global from base register `a` (displacement
    /// zero): the small-data and absolute paths share the instruction choice and
    /// differ only in how `a`/the relocation are set up.
    fn global_load_instruction(&self, global_type: Type, d: u8, a: u8) -> Compilation<Instruction> {
        Ok(match global_type {
            Type::Int | Type::UnsignedInt => Instruction::LoadWord { d, a, offset: 0 },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZero { d, a, offset: 0 },
            Type::Short => Instruction::LoadHalfwordAlgebraic { d, a, offset: 0 },
            Type::UnsignedShort => Instruction::LoadHalfwordZero { d, a, offset: 0 },
            Type::Float => Instruction::LoadFloatSingle { d, a, offset: 0 },
            Type::Double => Instruction::LoadFloatDouble { d, a, offset: 0 },
            // A pointer global is a 32-bit address word.
            Type::Pointer(_) | Type::StructPointer { .. } => Instruction::LoadWord { d, a, offset: 0 },
            other => return Err(Diagnostic::error(format!("global of type {other:?} is not supported yet"))),
        })
    }

    /// Emit `lis base, name@ha` — the high-adjusted half of an absolute address,
    /// with its `R_PPC_ADDR16_HA` relocation. `base` must never be r0: an `addi`
    /// or load based on r0 reads literal zero, not the register (the `li` trap).
    fn emit_address_high(&mut self, base: u8, name: &str) {
        self.record_relocation(RelocationKind::Addr16Ha, name);
        self.output.instructions.push(Instruction::load_immediate_shifted(base, 0));
    }

    /// Load a global under absolute (`-sdata 0`) addressing. mwcc's address-mode
    /// selection follows from r0 never being a usable base: when the destination
    /// is a non-r0 GPR, the address materializes into it (`lis dest; addi dest;
    /// load 0(dest)`) — base and destination coincide, so nothing folds; a float
    /// destination (an FPR) takes a separate free GPR base with `name@l` folded
    /// into the load. An integer load whose destination is the scratch r0 would
    /// need a separate base that avoids the (un-reserved) sibling operand — that
    /// liveness is the register allocator's to track, so it defers for now.
    fn emit_global_load_absolute(&mut self, name: &str, global_type: Type, destination: u8) -> Compilation<()> {
        if global_type == Type::Float {
            let base = self.lowest_free_general()?;
            self.emit_address_high(base, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            let load = self.global_load_instruction(global_type, destination, base)?;
            self.output.instructions.push(load);
            return Ok(());
        }
        if destination != GENERAL_SCRATCH {
            self.emit_address_high(destination, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: 0 });
            let load = self.global_load_instruction(global_type, destination, destination)?;
            self.output.instructions.push(load);
            return Ok(());
        }
        // destination == r0 (a scratch operand): a separate base GPR holds the
        // address and `@l` folds into the load. The base is the lowest free GPR,
        // which avoids any sibling operand the caller has reserved — r0 itself can
        // never be the base (the literal-zero trap).
        let base = self.lowest_free_general()?;
        self.emit_address_high(base, name);
        self.record_relocation(RelocationKind::Addr16Lo, name);
        let load = self.global_load_instruction(global_type, destination, base)?;
        self.output.instructions.push(load);
        Ok(())
    }

    /// Store `source` to a file-scope global. Small-data uses the `0(r0)` SDA21
    /// placeholder; absolute addressing materializes the high half into a free
    /// base GPR (avoiding the value register) and folds `name@l` into the store.
    pub(crate) fn emit_global_store(&mut self, name: &str, pointee: Pointee, source: u8) -> Compilation<()> {
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output.instructions.push(displacement_store(pointee, source, 0, 0));
            }
            GlobalAddressing::Absolute => {
                let base = self.free_general_excluding(source)?;
                self.emit_address_high(base, name);
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(displacement_store(pointee, source, base, 0));
            }
        }
        Ok(())
    }

    /// `(pointee, address register)` for a pointer leaf variable.
    fn pointer_leaf(&self, base: &Expression) -> Compilation<(Pointee, u8)> {
        let name = leaf_name(base).ok_or_else(|| Diagnostic::error("pointer access needs a pointer variable (roadmap)"))?;
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        let pointee = location.pointee.ok_or_else(|| Diagnostic::error(format!("'{name}' is not a pointer")))?;
        Ok((pointee, location.register))
    }

    /// Resolve a pointer expression to its (pointee, address register), emitting
    /// any load needed to materialize the address. A leaf pointer variable needs
    /// nothing; a pointer-typed struct member (`*p->q`) loads the pointer value
    /// into the base's register first, reusing it as mwcc does.
    fn resolve_pointer(&mut self, base: &Expression) -> Compilation<(Pointee, u8)> {
        // `*(T*)p` — a pointer cast reinterprets the address; the load/store type is the cast's
        // target POINTEE (`*(int*)p` -> lwz, `*(short*)p` -> lha, `*(char*)p` -> lbz), the address a
        // leaf pointer operand (whose own pointee, e.g. `void*`, is irrelevant to the access).
        if let Expression::Cast { target_type: Type::Pointer(pointee), operand } = base {
            if let Some(register) = leaf_name(operand).and_then(|name| self.lookup_general(name)) {
                return Ok((*pointee, register));
            }
        }
        if let Some((member_base, offset, member_type)) = as_member(base) {
            let pointee = match member_type {
                Type::Pointer(pointee) => pointee,
                _ => return Err(Diagnostic::error("dereferenced member is not a pointer")),
            };
            let register = self.member_base_register(member_base)?;
            self.output.instructions.push(Instruction::LoadWord { d: register, a: register, offset: offset as i16 });
            return Ok((pointee, register));
        }
        self.pointer_leaf(base)
    }

    /// The register a just-stored global is still live in, if reading it now would
    /// reuse it correctly: the value must not have been touched since the store (no
    /// instruction emitted), and a scratch (`r0`) value can only feed a consumer
    /// that does not use it as an `addi` base (where `r0` reads as literal zero).
    fn live_global_register(&self, name: &str, prefer_destination: bool) -> Option<u8> {
        let &(register, at) = self.stored_globals.get(name)?;
        if at != self.output.instructions.len() {
            return None;
        }
        if register == GENERAL_SCRATCH && prefer_destination {
            return None;
        }
        Some(register)
    }

    /// Load a signed-byte operand into the scratch and sign-extend it in place (`lbz r0; extsb
    /// r0,r0`), returning the scratch — for the unary/shift idioms (`neg`, `not`, `srawi`) that
    /// read their operand from r0, where mwcc keeps it. (`addi` cannot take r0 as a source — it
    /// means literal zero — so the Add/Subtract path keeps the value in the destination via
    /// place_operand instead.) Returns None for a non-signed-byte operand or a scratch destination,
    /// so the caller falls back to its normal place_operand/place_operand_or_scratch path.
    pub(crate) fn signed_byte_scratch_source(&mut self, operand: &Expression, destination: u8) -> Compilation<Option<u8>> {
        if destination != GENERAL_SCRATCH && self.is_signed_byte_load(operand)? {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
            Ok(Some(GENERAL_SCRATCH))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn place_operand(&mut self, operand: &Expression, destination: u8, prefer_destination: bool) -> Compilation<Option<u8>> {
        // A same-width 32-bit integer cast (`(unsigned)x` / `(int)u`) is a bit-exact
        // reinterpretation — place its operand directly rather than copying it
        // through the scratch. The consumer takes the signedness from the cast, so
        // e.g. `(unsigned)x >> n` stays a single `srwi`.
        if let Expression::Cast { target_type, operand: inner } = operand {
            if target_type.width() == 32 && self.plain_integer_leaf_register(inner).is_some() {
                return self.place_operand(inner, destination, prefer_destination);
            }
        }
        // A SIGNED CHAR load (member `p->x`, element `a[i]`, deref `*p`) used as an integer
        // operand needs the sign-extension its `lbz`/`lbzx` does not carry — `p->x + 1` is
        // `lbz r0; extsb r3,r0; addi`, and every non-truncating operator (`+ - * << >> | ^ /`,
        // unary, compare) miscompiles on the raw zero-extended byte (`0xFF` reads 255, not -1).
        // mwcc loads it into the scratch and sign-extends into the destination (`lbz r0;
        // extsb d,r0`); the consumer then reads the sign-extended value from the destination. A
        // TRUNCATING consumer (a fitting mask) sets narrow_truncation_context and reads the raw byte
        // — exempt; a SHORT load sign-extends (`lha`) and the direct `return p->x` uses
        // evaluate_general — both unaffected. The scratch destination (value/store context) uses a
        // different mwcc layout, so it still defers there.
        if !self.narrow_truncation_context && self.is_signed_byte_load(operand)? {
            if destination == GENERAL_SCRATCH {
                return Err(Diagnostic::error("a signed char load operand needs a sign-extension (roadmap)"));
            }
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            self.emit_widen(destination, GENERAL_SCRATCH, 8, true);
            return Ok(Some(destination));
        }
        if let Expression::Variable(name) = operand {
            // A global is loaded into the consumer's register (the destination for
            // addi-family consumers, otherwise the scratch), like a dereference —
            // unless it was just stored and is still live in a register, which is
            // reused (no reload), reproducing mwcc.
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()) {
                if let Some(register) = self.live_global_register(name, prefer_destination) {
                    return Ok(Some(register));
                }
                let target = if prefer_destination { destination } else { GENERAL_SCRATCH };
                self.emit_global_load(name, target)?;
                return Ok(Some(target));
            }
            let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
            let (register, width, signed) = (location.register, location.width, location.signed);
            if width == 32 {
                return Ok(Some(register));
            }
            // In a narrow-truncation context the result is truncated, so a narrow
            // operand of a truncation-safe op is read raw (no leading extension) — the
            // final truncation makes it redundant, matching mwcc.
            if self.narrow_truncation_context {
                return Ok(Some(register));
            }
            // A narrow operand is width-extended to 32 bits before use. The
            // extension lands in the consumer's working register: the destination
            // for addi-family consumers that keep their operand in place, otherwise
            // the scratch (mwcc routes `extsb r0,rX` ahead of an `rlwinm`/`mulli`).
            let target = if prefer_destination { destination } else { GENERAL_SCRATCH };
            self.emit_widen(target, register, width, signed);
            return Ok(Some(target));
        }
        // A call result lands in r3, its home. Let the consumer read it there rather than
        // bouncing it through the scratch with a move mwcc does not emit: place it in a
        // fresh virtual the allocator colors to r3 (the resulting `mr r3,r3` coalesces away).
        // For a tail consumer (destination already r3) the move is a self-move that vanishes;
        // for a scratch consumer it keeps the operand in r3, matching mwcc's `<op> d,r3,…`.
        if matches!(operand, Expression::Call { .. }) {
            let home = self.fresh_virtual_general();
            self.evaluate_general(operand, home)?;
            return Ok(Some(home));
        }
        if prefer_destination {
            self.evaluate_general(operand, destination)?;
            Ok(Some(destination))
        } else {
            if !fits_single_scratch(operand, true) {
                return Ok(None);
            }
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            Ok(Some(GENERAL_SCRATCH))
        }
    }

    /// Place a single consumed operand: in its own register if a leaf, otherwise
    /// computed into the scratch. A complex operand that needs temporaries beyond
    /// the scratch is no longer a deferral — the allocator supplies them (its
    /// inner sub-expressions emit virtuals), so the operand simply evaluates into
    /// the scratch like mwcc does (`mullw r0,...; neg r3,r0`). Used by the unary
    /// operators and the compare-against-zero idioms.
    pub(crate) fn place_operand_or_scratch(&mut self, operand: &Expression, destination: u8) -> Compilation<u8> {
        match self.place_operand(operand, destination, false)? {
            Some(source) => Ok(source),
            None => {
                self.evaluate_general(operand, GENERAL_SCRATCH)?;
                Ok(GENERAL_SCRATCH)
            }
        }
    }

    /// Emit a prefix unary operator into `destination`.
    pub(crate) fn emit_unary(&mut self, operator: UnaryOperator, operand: &Expression, destination: u8) -> Compilation<()> {
        let d = destination;
        match operator {
            UnaryOperator::Negate => {
                // Negating a literal folds to loading the negated constant.
                if let Expression::IntegerLiteral(value) = operand {
                    self.load_integer_constant(d, -*value);
                    return Ok(());
                }
                // -(-x) == x
                if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                // -(x < 0) / -(x > 0): the sign-bit comparison idioms end in a logical
                // shift (`srwi 31`, giving 0/1); negating the boolean is just the
                // arithmetic shift (`srawi 31`, giving 0/-1) instead — no separate
                // `neg`, and (for `>`) the operand stays live for the `andc`.
                if let Expression::Binary { operator: comparison @ (BinaryOperator::Less | BinaryOperator::Greater), left, right } = operand {
                    if is_zero_literal(right) && self.signedness_of(left)? {
                        if *comparison == BinaryOperator::Less {
                            // -(x < 0) = srawi d, x, 31
                            let source = self.place_operand_or_scratch(left, d)?;
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: source, shift: 31 });
                        } else {
                            // -(x > 0) = neg r0,x; andc r0,r0,x; srawi d,r0,31
                            self.evaluate_general(left, d)?;
                            self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                            self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                        }
                        return Ok(());
                    }
                }
                let source = match self.signed_byte_scratch_source(operand, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(operand, d)?,
                };
                self.output.instructions.push(Instruction::Negate { d, a: source });
            }
            UnaryOperator::BitNot => {
                // ~(~x) == x
                if let Expression::Unary { operator: UnaryOperator::BitNot, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                // `~(a | b)` / `~(a & b)` / `~(a ^ b)` fuse to a single nor / nand /
                // eqv. Both operands must be in registers: two leaves, or a leaf and
                // a constant materialized into the scratch (`li r0,c; nor d,a,r0`).
                if let Expression::Binary { operator: inner @ (BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor), left, right } = operand {
                    let left_register = leaf_name(left).and_then(|name| self.lookup_general(name));
                    let right_register = leaf_name(right).and_then(|name| self.lookup_general(name));
                    let registers = match (left_register, right_register) {
                        (Some(ra), Some(rb)) => Some((ra, rb)),
                        (Some(ra), None) => constant_value(right).filter(|c| fits_signed_16(*c)).map(|constant| {
                            self.load_integer_constant(GENERAL_SCRATCH, constant);
                            (ra, GENERAL_SCRATCH)
                        }),
                        _ => None,
                    };
                    if let Some((ra, rb)) = registers {
                        self.output.instructions.push(match inner {
                            BinaryOperator::BitOr => Instruction::Nor { a: d, s: ra, b: rb },
                            BinaryOperator::BitAnd => Instruction::Nand { a: d, s: ra, b: rb },
                            _ => Instruction::Eqv { a: d, s: ra, b: rb },
                        });
                        return Ok(());
                    }
                }
                let source = match self.signed_byte_scratch_source(operand, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(operand, d)?,
                };
                self.output.instructions.push(Instruction::Nor { a: d, s: source, b: source });
            }
            UnaryOperator::LogicalNot => {
                // Fold a chain of `!`: mwcc collapses repeated negations by parity.
                // An even chain is the boolean normalization `inner != 0`; an odd
                // chain is `inner == 0` — and `!comparison` flips the comparison.
                let mut negations = 1usize;
                let mut inner = operand;
                while let Expression::Unary { operator: UnaryOperator::LogicalNot, operand: next } = inner {
                    negations += 1;
                    inner = next;
                }
                let odd = negations % 2 == 1;
                // `!(comparison)` is the flipped comparison; an even chain keeps it.
                if let Expression::Binary { operator, left, right } = inner {
                    if is_comparison(*operator) {
                        let resolved = if odd { flip_comparison(*operator) } else { Some(*operator) };
                        if let Some(operator) = resolved {
                            return self.emit_comparison(operator, left, right, d);
                        }
                    }
                }
                // An even chain is `inner != 0` (the `neg; or; srwi` boolean).
                if !odd {
                    let zero = Expression::IntegerLiteral(0);
                    return self.emit_comparison(BinaryOperator::NotEqual, inner, &zero, d);
                }
                // An odd chain is `inner == 0`: cntlzw then srwi by 5.
                let source = match self.signed_byte_scratch_source(inner, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(inner, d)?,
                };
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
            }
        }
        Ok(())
    }
}
