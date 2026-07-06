//! Operand placement and the free-register allocator helpers.

use std::collections::HashSet;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};
use mwcc_vreg::Class;
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

/// Whether `expr` is a division by a constant — power-of-two (`srawi`/`addze`) or the
/// non-power-of-two magic-number lowering. Either consumes a temporary register, and in
/// a binary op mwcc orders its result first (`add quotient,b`) rather than the leaf — an
/// operand-order quirk not yet modeled — so such a node alongside another operand is
/// deferred rather than emitted with the wrong order.
fn is_constant_divide(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Binary { operator: BinaryOperator::Divide, right, .. }
            if constant_value(right).is_some_and(|divisor| divisor >= 2)
    )
}

impl Generator {

    /// Place two leaf operands when at least one is narrow, emitting the width
    /// extensions mwcc inserts. A wide leaf stays in its home register; a narrow
    /// leaf is extended — into the scratch when it is the only operand needing
    /// materialization or the non-anchor of a two-narrow pair, in place (its home)
    /// when it is the pair's anchor. The anchor is the left operand for commutative
    /// operators and the right for subtraction, matching mwcc's evaluation order.
    pub(crate) fn place_narrow_leaves(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        let (left_register, left_width, left_signed) = self.leaf_info(left)?;
        let (right_register, right_width, right_signed) = self.leaf_info(right)?;
        let left_narrow = left_width < 32;
        let right_narrow = right_width < 32;
        let subtract = operator == BinaryOperator::Subtract;

        // Where each operand ends up.
        let (left_target, right_target) = if left_narrow && right_narrow {
            if subtract {
                (GENERAL_SCRATCH, right_register) // right is the anchor, kept in place
            } else {
                (left_register, GENERAL_SCRATCH) // left is the anchor, kept in place
            }
        } else if left_narrow {
            (GENERAL_SCRATCH, right_register)
        } else {
            (left_register, GENERAL_SCRATCH)
        };

        // Emit extensions in mwcc's order: the anchor first, then the scratch operand.
        if subtract {
            if right_narrow { self.emit_widen(right_target, right_register, right_width, right_signed); }
            if left_narrow { self.emit_widen(left_target, left_register, left_width, left_signed); }
        } else {
            if left_narrow { self.emit_widen(left_target, left_register, left_width, left_signed); }
            if right_narrow { self.emit_widen(right_target, right_register, right_width, right_signed); }
        }
        Operands::ordered(left_target, right_target)
    }

    pub(crate) fn place_general_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        // A dereference operand loads into a register but orders like a leaf, not
        // like a reversed sub-expression — handle it before the complexity match.
        if as_dereference(left).is_some() || as_dereference(right).is_some() {
            return self.place_dereference_operands(operator, left, right);
        }
        // A global operand also loads into a register (from the small-data area).
        if self.is_global(left) || self.is_global(right) {
            return self.place_global_operands(operator, left, right);
        }
        // A struct-member operand loads into a register, like a dereference.
        if as_member(left).is_some() || as_member(right).is_some() {
            return self.place_member_operands(operator, left, right);
        }
        match (is_complex(left), is_complex(right)) {
            (false, false) => {
                if self.is_narrow_leaf(left) || self.is_narrow_leaf(right) {
                    return self.place_narrow_leaves(operator, left, right);
                }
                Operands::ordered(self.general_register_of_leaf(left)?, self.general_register_of_leaf(right)?)
            }
            (true, false) => {
                // A constant-divide left in a commutative op wants its result ordered first
                // (`add quotient,b`), not the leaf — not modeled, so defer (`a/3 - b` is
                // non-commutative and unaffected).
                if is_commutative(operator) && is_constant_divide(left) {
                    return Err(Diagnostic::error("a constant-divide operand in a commutative binary op needs mwcc's result-first order (roadmap)"));
                }
                // Keep the leaf right reserved while the left is computed, so a left that
                // needs a temporary (a magic-number divide) can't pick the leaf's register
                // and clobber it. left into scratch, right is a leaf: mwcc puts the leaf first.
                self.with_reserved_inputs(right, |generator| generator.evaluate_general(left, GENERAL_SCRATCH))?;
                let leaf = self.general_register_of_leaf(right)?;
                // A constant-amount SHIFT left (`(a<<2) + b`) is ordered SHIFT-FIRST by mwcc — the
                // source `<<` drives it, even though it lowers to the same `slwi` as a strength-reduced
                // `a*4`, which mwcc orders leaf-first. Every other complex left (mul, magic divide)
                // stays leaf-first.
                if is_commutative(operator) && crate::analysis::is_constant_shift(left) {
                    return Operands::ordered(GENERAL_SCRATCH, leaf);
                }
                // A SINGLE-REGISTER-computed left (`(a-1)*b`, `-a*b`, `(2-a)&b` — one leaf via
                // addi/subfic/neg) is ordered COMPUTED-FIRST by mwcc (`mullw r3, r0(a-1), r4(b)`),
                // like the shift above; a two-register computed left (a product/sum) stays leaf-first.
                // `+` is EXCLUDED — a `var±const` left there is REASSOCIATED (`(a-1)+b` -> `(a+b)-1`),
                // not merely reordered, so it keeps deferring via analysis::is_constant_hoist_add.
                if matches!(operator, BinaryOperator::Multiply | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                    && crate::analysis::single_register_computed(left)
                {
                    return Operands::ordered(GENERAL_SCRATCH, leaf);
                }
                Operands::reversed(GENERAL_SCRATCH, leaf)
            }
            (false, true) => {
                self.with_reserved_inputs(left, |generator| generator.evaluate_general(right, GENERAL_SCRATCH))?;
                Operands::ordered(self.general_register_of_leaf(left)?, GENERAL_SCRATCH)
            }
            (true, true) => {
                // A constant-divide alongside another complex operand needs liveness-aware
                // temp allocation and result-first ordering that isn't modeled; defer.
                if is_constant_divide(left) || is_constant_divide(right) {
                    return Err(Diagnostic::error("a constant-divide operand alongside another complex operand needs the register allocator (roadmap)"));
                }
                // Sethi-Ullman for a commutative node with operands of different
                // register need: mwcc evaluates the heavier operand first and keeps
                // its result in the scratch, while the lighter goes to a fresh
                // virtual the allocator coalesces onto the destination — emitting
                // `op dest, lighter, heavier`. Both `((c+d)*(e+f))*(a+b)` and its
                // mirror reduce to this. Only safe when the lighter (evaluated
                // second) does not itself need the scratch, or it would clobber the
                // heavier's result there.
                let left_need = register_need(left);
                let right_need = register_need(right);
                if is_commutative(operator) && left_need != right_need {
                    let (heavier, lighter) = if left_need > right_need { (left, right) } else { (right, left) };
                    if !needs_scratch(lighter) {
                        self.with_reserved_inputs(lighter, |generator| generator.evaluate_general(heavier, GENERAL_SCRATCH))?;
                        let lighter_register = self.fresh_virtual_general();
                        self.evaluate_general(lighter, lighter_register)?;
                        return Operands::ordered(lighter_register, GENERAL_SCRATCH);
                    }
                }
                // Default: the left operand into a virtual (keeping the right's
                // inputs live), the right into the scratch.
                let temp = self.with_reserved_inputs(right, |generator| {
                    let temp = generator.fresh_virtual_general();
                    generator.evaluate_general(left, temp)?;
                    Ok(temp)
                })?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Operands::ordered(temp, GENERAL_SCRATCH)
            }
        }
    }

    /// Place a binary node where at least one operand is a `*pointer` load. A
    /// single deref loads into the scratch and the other operand stays in its home
    /// register (the deref keeps source order); two derefs load left into the
    /// destination and right into the scratch.
    fn place_dereference_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        // A dereference paired with a global follows the same anchor model as a
        // member with a global: the anchor stays in a stable register, the other
        // loads into the scratch.
        if self.is_global(left) || self.is_global(right) {
            return self.place_deref_global_operands(operator, left, right);
        }
        // A dereference paired with a struct member: two located operands with
        // different base registers, placed by the anchor model.
        if as_member(left).is_some() || as_member(right).is_some() {
            return self.place_located_pair_operands(operator, left, right);
        }
        match (as_dereference(left), as_dereference(right)) {
            (Some(left_pointer), Some(right_pointer)) => {
                // Subtraction anchors the right operand: it loads in place (into its
                // own pointer register) while the left loads into the scratch.
                if operator == BinaryOperator::Subtract {
                    let right_register = self.general_register_of_leaf(right_pointer)?;
                    self.emit_load_from_pointer(right_pointer, right_register)?;
                    self.emit_load_from_pointer(left_pointer, GENERAL_SCRATCH)?;
                    return Operands::ordered(GENERAL_SCRATCH, right_register);
                }
                // The left dereference loads into a fresh virtual (the allocator
                // coalesces it onto the pointer's own register, which dies at the
                // load); the right into the scratch. No longer needs a non-scratch
                // result register, so `(*p + *q) * x` lowers instead of deferring.
                let anchor = self.fresh_virtual_general();
                self.emit_load_from_pointer(left_pointer, anchor)?;
                self.emit_load_from_pointer(right_pointer, GENERAL_SCRATCH)?;
                Operands::ordered(anchor, GENERAL_SCRATCH)
            }
            (Some(left_pointer), None) => {
                let right_register = self.wide_leaf_register(right)?;
                self.emit_load_from_pointer(left_pointer, GENERAL_SCRATCH)?;
                Operands::ordered(GENERAL_SCRATCH, right_register)
            }
            (None, Some(right_pointer)) => {
                let left_register = self.wide_leaf_register(left)?;
                self.emit_load_from_pointer(right_pointer, GENERAL_SCRATCH)?;
                Operands::ordered(left_register, GENERAL_SCRATCH)
            }
            (None, None) => unreachable!("caller checked one side is a dereference"),
        }
    }

    /// Place a binary node where at least one operand is a `base->field` load.
    /// A member loads like a dereference: a single member loads into the scratch
    /// (the other operand stays home); two members load the first-needed one into
    /// a free temporary (the shared base register must survive the first load) and
    /// the second into the scratch. Subtraction loads the right operand first so
    /// `subf` computes `left - right`.
    fn place_member_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        // A signed-`char` member loads via `lbz` (zero-extend) and needs a separate
        // `extsb` that mwcc batches *after* both loads (`lbz; lha; extsb; add`). That
        // batched, destination-targeted sign-extend is not yet modeled here, so defer
        // rather than drop it (which would zero-extend a negative byte). `unsigned char`
        // (lbz is correct), `short` (lha self-extends), and `int` are unaffected.
        if matches!(as_member(left), Some((_, _, Type::Char))) || matches!(as_member(right), Some((_, _, Type::Char))) {
            return Err(Diagnostic::error("a signed-char member in a binary needs the batched sign-extend (roadmap)"));
        }
        let subtract = operator == BinaryOperator::Subtract;
        match (as_member(left), as_member(right)) {
            (Some((left_base, left_offset, left_type)), Some((right_base, right_offset, right_type))) => {
                // The anchor member loads into a fresh virtual the allocator places;
                // it coalesces onto a free register that avoids both bases (which
                // stay live for the second load) and the result register. So this
                // no longer needs a non-scratch result, and `(p->a + p->b) * x`
                // lowers instead of deferring.
                let temp = self.fresh_virtual_general();
                if subtract {
                    self.emit_member_load(right_base, right_offset, right_type, None, temp)?;
                    self.emit_member_load(left_base, left_offset, left_type, None, GENERAL_SCRATCH)?;
                    Operands::ordered(GENERAL_SCRATCH, temp)
                } else {
                    self.emit_member_load(left_base, left_offset, left_type, None, temp)?;
                    self.emit_member_load(right_base, right_offset, right_type, None, GENERAL_SCRATCH)?;
                    Operands::ordered(temp, GENERAL_SCRATCH)
                }
            }
            (Some((base, offset, member_type)), None) => {
                let right_register = self.wide_leaf_register(right)?;
                self.emit_member_load(base, offset, member_type, None, GENERAL_SCRATCH)?;
                Operands::ordered(GENERAL_SCRATCH, right_register)
            }
            (None, Some((base, offset, member_type))) => {
                let left_register = self.wide_leaf_register(left)?;
                self.emit_member_load(base, offset, member_type, None, GENERAL_SCRATCH)?;
                Operands::ordered(left_register, GENERAL_SCRATCH)
            }
            (None, None) => unreachable!("caller checked one side is a member"),
        }
    }

    /// The lowest free general register that avoids the registers read by the
    /// given base expressions (so a member's shared base survives the first load).
    pub(crate) fn free_register_avoiding(&mut self, bases: &[&Expression]) -> Compilation<u8> {
        let mut reserved_registers = HashSet::new();
        for base in bases {
            reserved_registers.extend(self.registers_used_by(base));
        }
        let newly: Vec<u8> = reserved_registers.iter().copied().filter(|register| self.reserved.insert(*register)).collect();
        let register = self.lowest_free_general();
        for register in &newly {
            self.reserved.remove(register);
        }
        register
    }

    /// Whether `operand` is a reference to a file-scope global.
    pub(crate) fn is_global(&self, operand: &Expression) -> bool {
        matches!(operand, Expression::Variable(name)
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()))
    }

    /// Place a binary node where at least one operand is a global load. A single
    /// global loads into the scratch (the other operand stays home); two globals
    /// load left into the destination and right into the scratch, except for
    /// subtraction, which loads the right into the destination and left into the
    /// scratch (a global has no address register to keep it in place).
    fn place_global_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        let subtract = operator == BinaryOperator::Subtract;
        // One global with one struct member: handled by the anchor placement below.
        if (self.is_global(left) && as_member(right).is_some()) || (self.is_global(right) && as_member(left).is_some()) {
            return self.place_global_member_operands(operator, left, right);
        }
        match (self.is_global(left), self.is_global(right)) {
            (true, true) => {
                let left_name = leaf_name(left).unwrap();
                let right_name = leaf_name(right).unwrap();
                // mwcc loads both globals first, then applies any signed-char
                // sign-extensions — the loads are batched ahead of the extends.
                // The anchor (left for commutative, right for subtraction) loads
                // into a fresh virtual register the allocator places — so this no
                // longer needs the result register to be non-scratch (a two-global
                // sub-expression like `(g+h)*x` now lowers instead of deferring),
                // and the other operand takes the scratch.
                let anchor_register = self.fresh_virtual_general();
                let (first, first_register, second, second_register) = if subtract {
                    (right_name, anchor_register, left_name, GENERAL_SCRATCH)
                } else {
                    (left_name, anchor_register, right_name, GENERAL_SCRATCH)
                };
                self.emit_global_load_value(first, first_register)?;
                self.emit_global_load_value(second, second_register)?;
                if self.global_char_extend(first)? { self.emit_widen(first_register, first_register, 8, true); }
                if self.global_char_extend(second)? { self.emit_widen(second_register, second_register, 8, true); }
                if subtract {
                    Operands::ordered(GENERAL_SCRATCH, anchor_register)
                } else {
                    Operands::ordered(anchor_register, GENERAL_SCRATCH)
                }
            }
            (true, false) => {
                let right_register = self.wide_leaf_register(right)?;
                // Keep the sibling live so an absolute-addressing base avoids it.
                let restore = self.reserved.insert(right_register);
                let loaded = self.emit_global_load(leaf_name(left).unwrap(), GENERAL_SCRATCH);
                if restore { self.reserved.remove(&right_register); }
                loaded?;
                Operands::ordered(GENERAL_SCRATCH, right_register)
            }
            (false, true) => {
                let left_register = self.wide_leaf_register(left)?;
                let restore = self.reserved.insert(left_register);
                let loaded = self.emit_global_load(leaf_name(right).unwrap(), GENERAL_SCRATCH);
                if restore { self.reserved.remove(&left_register); }
                loaded?;
                Operands::ordered(left_register, GENERAL_SCRATCH)
            }
            (false, false) => unreachable!("caller checked one side is a global"),
        }
    }

    /// The base register of a located operand (`*pointer` or `base->field`): the
    /// register holding the address it loads from.
    fn located_base_register(&mut self, operand: &Expression) -> Compilation<u8> {
        if let Some((base, _, _)) = as_member(operand) {
            self.member_base_register(base)
        } else if let Some(pointer) = as_dereference(operand) {
            self.general_register_of_leaf(pointer)
        } else {
            Err(Diagnostic::error("expected a dereference or member operand"))
        }
    }

    /// Emit the load for a located operand (`*pointer` or `base->field`) into the
    /// given register.
    pub(crate) fn emit_located_operand(&mut self, operand: &Expression, destination: u8) -> Compilation<()> {
        if let Some((base, offset, member_type)) = as_member(operand) {
            self.emit_member_load(base, offset, member_type, None, destination)
        } else if let Some(pointer) = as_dereference(operand) {
            self.emit_load_from_pointer(pointer, destination)
        } else if let Expression::Variable(name) = operand {
            // A file-scope global (used by the float placement for float globals).
            self.emit_global_load(name, destination)
        } else {
            Err(Diagnostic::error("expected a dereference, member, or global operand"))
        }
    }

    /// Place a binary node of two located operands (a `*pointer` and a member with
    /// different bases) by the anchor model: the anchor loads into its own base
    /// register, the other into the scratch.
    fn place_located_pair_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        let subtract = operator == BinaryOperator::Subtract;
        let (anchor, other) = if subtract { (right, left) } else { (left, right) };
        let anchor_register = self.located_base_register(anchor)?;
        self.emit_located_operand(anchor, anchor_register)?;
        self.emit_located_operand(other, GENERAL_SCRATCH)?;
        if subtract {
            Operands::ordered(GENERAL_SCRATCH, anchor_register)
        } else {
            Operands::ordered(anchor_register, GENERAL_SCRATCH)
        }
    }

    /// Place a binary node with one `*pointer` operand and one global operand,
    /// using the anchor model (anchor = left for commutative, right for subtract):
    /// the dereference anchor loads into its pointer register, a global anchor into
    /// a free register avoiding that pointer; the other operand loads into scratch.
    fn place_deref_global_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        let subtract = operator == BinaryOperator::Subtract;
        let (anchor, other) = if subtract { (right, left) } else { (left, right) };
        let (anchor_register, other_register) = if let Some(pointer) = as_dereference(anchor) {
            let pointer_register = self.general_register_of_leaf(pointer)?;
            self.emit_load_from_pointer(pointer, pointer_register)?;
            self.emit_global_load(leaf_name(other).unwrap(), GENERAL_SCRATCH)?;
            (pointer_register, GENERAL_SCRATCH)
        } else {
            let pointer = as_dereference(other).unwrap();
            let global_register = self.fresh_virtual_general();
            self.emit_global_load(leaf_name(anchor).unwrap(), global_register)?;
            self.emit_load_from_pointer(pointer, GENERAL_SCRATCH)?;
            (global_register, GENERAL_SCRATCH)
        };
        if subtract {
            Operands::ordered(other_register, anchor_register)
        } else {
            Operands::ordered(anchor_register, other_register)
        }
    }

    /// Place a binary node with one global operand and one struct-member operand.
    /// The anchor (left for a commutative operator, right for subtraction) is kept
    /// in a stable register — a member in its base register, a global in a free
    /// register that avoids the member's base — and the other operand loads into
    /// the scratch. The anchor loads first, matching mwcc's evaluation order.
    fn place_global_member_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        let subtract = operator == BinaryOperator::Subtract;
        let (anchor, other) = if subtract { (right, left) } else { (left, right) };
        let (anchor_register, other_register) = if let Some((base, offset, member_type)) = as_member(anchor) {
            // The member anchor loads into its base register; the global -> scratch.
            let base_register = self.member_base_register(base)?;
            self.emit_member_load(base, offset, member_type, None, base_register)?;
            self.emit_global_load(leaf_name(other).unwrap(), GENERAL_SCRATCH)?;
            (base_register, GENERAL_SCRATCH)
        } else {
            // The global anchor loads into a free register; the member -> scratch.
            let (base, offset, member_type) = as_member(other).unwrap();
            let global_register = self.fresh_virtual_general();
            self.emit_global_load(leaf_name(anchor).unwrap(), global_register)?;
            self.emit_member_load(base, offset, member_type, None, GENERAL_SCRATCH)?;
            (global_register, GENERAL_SCRATCH)
        };
        // For subtraction the anchor is the right operand (`left - right`).
        if subtract {
            Operands::ordered(other_register, anchor_register)
        } else {
            Operands::ordered(anchor_register, other_register)
        }
    }

    /// The home register of a wide (32-bit) leaf variable; narrow leaves and
    /// non-leaves are deferred (they need extension or their own placement).
    fn wide_leaf_register(&self, operand: &Expression) -> Compilation<u8> {
        if !matches!(operand, Expression::Variable(_)) || self.is_narrow_leaf(operand) {
            return Err(Diagnostic::error("dereference combined with this operand needs the full allocator (roadmap)"));
        }
        self.general_register_of_leaf(operand)
    }

    /// Run `body` with the registers read by `expression` reserved, restoring the
    /// reservation set afterward.
    pub(crate) fn with_reserved_inputs<T>(&mut self, expression: &Expression, body: impl FnOnce(&mut Self) -> Compilation<T>) -> Compilation<T> {
        let registers = self.registers_used_by(expression);
        let newly_reserved: Vec<u8> = registers.iter().copied().filter(|register| self.reserved.insert(*register)).collect();
        let result = body(self);
        for register in &newly_reserved {
            self.reserved.remove(register);
        }
        result
    }

    /// The general registers read by variables in `expression`.
    pub(crate) fn registers_used_by(&self, expression: &Expression) -> HashSet<u8> {
        let mut registers = HashSet::new();
        self.collect_registers(expression, &mut registers);
        registers
    }

    pub(crate) fn collect_registers(&self, expression: &Expression, registers: &mut HashSet<u8>) {
        // Within a single expression all variables share a class, so we record
        // register numbers without filtering by class.
        match expression {
            Expression::Variable(name) => {
                if let Some(location) = self.locations.get(name) {
                    registers.insert(location.register);
                }
            }
            Expression::Binary { left, right, .. } => {
                self.collect_registers(left, registers);
                self.collect_registers(right, registers);
            }
            Expression::Unary { operand, .. } => self.collect_registers(operand, registers),
            Expression::Conditional { condition, when_true, when_false } => {
                self.collect_registers(condition, registers);
                self.collect_registers(when_true, registers);
                self.collect_registers(when_false, registers);
            }
            Expression::Cast { operand, .. } => self.collect_registers(operand, registers),
            // `base->field` reads the base pointer's register.
            Expression::Member { base, .. } => self.collect_registers(base, registers),
            _ => {}
        }
    }

    /// The lowest free general register: the first in the target's general pool
    /// (which already excludes the scratch) that is not reserved.
    pub(crate) fn lowest_free_general(&self) -> Compilation<u8> {
        self.constraints
            .pool(Class::General)
            .iter()
            .copied()
            .find(|register| !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free registers (roadmap M1: spilling)"))
    }

    /// The lowest free general register that also avoids `exclude` (e.g. an operand
    /// register that must survive).
    pub(crate) fn free_general_excluding(&self, exclude: u8) -> Compilation<u8> {
        self.constraints
            .pool(Class::General)
            .iter()
            .copied()
            .find(|register| *register != exclude && !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free registers (roadmap M1: spilling)"))
    }

    /// The lowest free float register: the first in the target's float pool (which
    /// already excludes the scratch) that is not reserved.
    pub(crate) fn lowest_free_float(&self) -> Compilation<u8> {
        self.constraints
            .pool(Class::Float)
            .iter()
            .copied()
            .find(|register| !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free float registers (roadmap M1: spilling)"))
    }
}
