//! Operand placement and the free-register allocator helpers.

use std::collections::HashSet;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression};
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

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

    pub(crate) fn place_general_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
        // A dereference operand loads into a register but orders like a leaf, not
        // like a reversed sub-expression — handle it before the complexity match.
        if as_dereference(left).is_some() || as_dereference(right).is_some() {
            return self.place_dereference_operands(operator, left, right, destination);
        }
        // A global operand also loads into a register (from the small-data area).
        if self.is_global(left) || self.is_global(right) {
            return self.place_global_operands(operator, left, right, destination);
        }
        match (is_complex(left), is_complex(right)) {
            (false, false) => {
                if self.is_narrow_leaf(left) || self.is_narrow_leaf(right) {
                    return self.place_narrow_leaves(operator, left, right);
                }
                Operands::ordered(self.general_register_of_leaf(left)?, self.general_register_of_leaf(right)?)
            }
            (true, false) => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                // left computed into scratch, right is a leaf: mwcc puts the leaf first.
                Operands::reversed(GENERAL_SCRATCH, self.general_register_of_leaf(right)?)
            }
            (false, true) => {
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Operands::ordered(self.general_register_of_leaf(left)?, GENERAL_SCRATCH)
            }
            (true, true) => {
                // Compute the left side into a free temporary, keeping the right
                // side's inputs live; then the right side into the scratch.
                let temp = self.with_reserved_inputs(right, |generator| {
                    let temp = generator.lowest_free_general()?;
                    generator.evaluate_general(left, temp)?;
                    Ok(temp)
                })?;
                // The temporary holds the left result; keep it live while the right runs.
                let temp_added = self.reserved.insert(temp);
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                if temp_added {
                    self.reserved.remove(&temp);
                }
                Operands::ordered(temp, GENERAL_SCRATCH)
            }
        }
    }

    /// Place a binary node where at least one operand is a `*pointer` load. A
    /// single deref loads into the scratch and the other operand stays in its home
    /// register (the deref keeps source order); two derefs load left into the
    /// destination and right into the scratch.
    fn place_dereference_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
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
                if destination == GENERAL_SCRATCH {
                    return Err(Diagnostic::error("two dereferences need a non-scratch destination (roadmap)"));
                }
                self.emit_load_from_pointer(left_pointer, destination)?;
                self.emit_load_from_pointer(right_pointer, GENERAL_SCRATCH)?;
                Operands::ordered(destination, GENERAL_SCRATCH)
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

    /// Whether `operand` is a reference to a file-scope global.
    fn is_global(&self, operand: &Expression) -> bool {
        matches!(operand, Expression::Variable(name)
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()))
    }

    /// Place a binary node where at least one operand is a global load. A single
    /// global loads into the scratch (the other operand stays home); two globals
    /// load left into the destination and right into the scratch, except for
    /// subtraction, which loads the right into the destination and left into the
    /// scratch (a global has no address register to keep it in place).
    fn place_global_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
        let subtract = operator == BinaryOperator::Subtract;
        match (self.is_global(left), self.is_global(right)) {
            (true, true) => {
                if destination == GENERAL_SCRATCH {
                    return Err(Diagnostic::error("two globals need a non-scratch destination (roadmap)"));
                }
                let left_name = leaf_name(left).unwrap();
                let right_name = leaf_name(right).unwrap();
                if subtract {
                    self.emit_global_load(right_name, destination)?;
                    self.emit_global_load(left_name, GENERAL_SCRATCH)?;
                    Operands::ordered(GENERAL_SCRATCH, destination)
                } else {
                    self.emit_global_load(left_name, destination)?;
                    self.emit_global_load(right_name, GENERAL_SCRATCH)?;
                    Operands::ordered(destination, GENERAL_SCRATCH)
                }
            }
            (true, false) => {
                let right_register = self.wide_leaf_register(right)?;
                self.emit_global_load(leaf_name(left).unwrap(), GENERAL_SCRATCH)?;
                Operands::ordered(GENERAL_SCRATCH, right_register)
            }
            (false, true) => {
                let left_register = self.wide_leaf_register(left)?;
                self.emit_global_load(leaf_name(right).unwrap(), GENERAL_SCRATCH)?;
                Operands::ordered(left_register, GENERAL_SCRATCH)
            }
            (false, false) => unreachable!("caller checked one side is a global"),
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
            _ => {}
        }
    }

    /// The lowest general register (r3..=r12) that is neither reserved nor the scratch.
    pub(crate) fn lowest_free_general(&self) -> Compilation<u8> {
        (3..=12)
            .find(|register| *register != GENERAL_SCRATCH && !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free registers (roadmap M1: spilling)"))
    }

    /// The lowest float register (f1..=f13) that is neither reserved nor the scratch.
    pub(crate) fn lowest_free_float(&self) -> Compilation<u8> {
        (1..=13)
            .find(|register| *register != FLOAT_SCRATCH && !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free float registers (roadmap M1: spilling)"))
    }
}
