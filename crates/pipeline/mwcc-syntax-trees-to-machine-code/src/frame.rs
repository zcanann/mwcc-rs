//! Frame-resident locals: a variable whose address is taken (via `&v`, or a
//! type-pun like `*(int*)&v`) cannot live in a register — it gets a stack-frame
//! slot. `&v` is `addi d, r1, slot`, reads/writes go to the slot, and a spilled
//! parameter is stored there in the prologue.

use std::collections::HashSet;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, Pointee, Statement, Type};
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::generator::*;

impl Generator {
    /// If the function takes the address of any variable, lower it with a stack
    /// frame: lay out a slot per address-taken parameter/local, spill the
    /// parameters in the prologue, and run the body against those slots. Returns
    /// whether this path took over the whole body.
    pub(crate) fn try_frame_resident(&mut self, function: &Function) -> Compilation<bool> {
        let address_taken = collect_address_taken(function);
        if address_taken.is_empty() {
            return Ok(false);
        }
        // This path handles a straight-line body (stores/calls) plus an optional
        // return; guard chains and reassignment-mixed bodies defer for now.
        if !function.guards.is_empty() {
            return Ok(false);
        }

        // Lay out a slot for each address-taken parameter (in argument order),
        // then each address-taken local, above the 8-byte linkage area.
        let mut offset: i16 = 8;
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            if address_taken.contains(parameter.name.as_str()) {
                let size = slot_size(parameter.parameter_type);
                offset = align_to(offset, size);
                self.frame_slots.insert(
                    parameter.name.clone(),
                    FrameSlot { offset, class, size, parameter_register: Some(register) },
                );
                offset += size as i16;
            }
        }
        for local in &function.locals {
            if address_taken.contains(local.name.as_str()) {
                // Only an uninitialized address-taken local is modeled here (its
                // value comes from a store through the taken address).
                if local.initializer.is_some() {
                    return Ok(false);
                }
                let class = class_of(local.declared_type)?;
                let size = slot_size(local.declared_type);
                offset = align_to(offset, size);
                self.frame_slots.insert(
                    local.name.clone(),
                    FrameSlot { offset, class, size, parameter_register: None },
                );
                offset += size as i16;
            }
        }

        // The frame is the linkage area plus the slots, rounded up to 16 bytes.
        let frame_size = (((offset as i32) + 15) / 16 * 16) as i16;
        let non_leaf = function_makes_call(function);
        self.non_leaf = non_leaf;
        self.frame_size = frame_size;

        // Prologue: allocate the frame, save the link register if non-leaf, then
        // spill the address-taken parameters to their slots.
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        if non_leaf {
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: frame_size + 4 });
        }
        for parameter in &function.parameters {
            if let Some(slot) = self.frame_slots.get(&parameter.name).copied() {
                if let Some(register) = slot.parameter_register {
                    self.output.instructions.push(spill_instruction(register, slot));
                }
            }
        }

        // Body statements, then the return value.
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        if function.return_type != Type::Void {
            let result = match function.return_type {
                Type::Float | Type::Double => Eabi::float_result().number,
                _ => Eabi::general_result().number,
            };
            let return_expression = function
                .return_expression
                .as_ref()
                .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
            self.evaluate(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Fold a pointer expression of the form `(n +) (t*) &framevar` to the
    /// `(pointee, byte offset from r1)` it accesses, or `None` if it does not
    /// reduce to a frame-resident address. This is how a type-pun such as
    /// `*(1 + (int*)&x)` becomes a plain displacement load/store from `r1`.
    pub(crate) fn resolve_frame_pointer(&self, pointer: &Expression) -> Option<(Pointee, i16)> {
        match pointer {
            Expression::AddressOf { operand } => {
                let name = match operand.as_ref() {
                    Expression::Variable(name) => name,
                    _ => return None,
                };
                let slot = self.frame_slots.get(name)?;
                let pointee = match (slot.class, slot.size) {
                    (ValueClass::Float, 8) => Pointee::Double,
                    (ValueClass::Float, _) => Pointee::Float,
                    _ => Pointee::Int,
                };
                Some((pointee, slot.offset))
            }
            // A cast retargets the pointee; the address is unchanged.
            Expression::Cast { target_type: Type::Pointer(pointee), operand } => {
                let (_, offset) = self.resolve_frame_pointer(operand)?;
                Some((*pointee, offset))
            }
            // Pointer arithmetic scales the integer addend by the pointee size.
            Expression::Binary { operator: BinaryOperator::Add, left, right } => {
                if let (Some((pointee, offset)), Some(count)) = (self.resolve_frame_pointer(left), constant_value(right)) {
                    return Some((pointee, offset + count as i16 * pointee.size() as i16));
                }
                if let (Some((pointee, offset)), Some(count)) = (self.resolve_frame_pointer(right), constant_value(left)) {
                    return Some((pointee, offset + count as i16 * pointee.size() as i16));
                }
                None
            }
            _ => None,
        }
    }

    /// `&operand` into a register: the address of a frame-resident variable is
    /// `addi d, r1, slot`.
    pub(crate) fn emit_address_of(&mut self, operand: &Expression, destination: u8) -> Compilation<()> {
        if let Expression::Variable(name) = operand {
            if let Some(slot) = self.frame_slots.get(name) {
                let offset = slot.offset;
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 1, immediate: offset });
                return Ok(());
            }
        }
        Err(Diagnostic::error("address-of a non-frame-resident lvalue is not supported yet (roadmap)"))
    }
}

/// The byte size of a variable's stack slot.
fn slot_size(declared: Type) -> u8 {
    match declared {
        Type::Double => 8,
        _ => 4,
    }
}

/// Round `offset` up to a multiple of `align`.
fn align_to(offset: i16, align: u8) -> i16 {
    let align = align as i16;
    (offset + align - 1) / align * align
}

/// The store that spills a parameter register to its frame slot.
fn spill_instruction(register: u8, slot: FrameSlot) -> Instruction {
    match (slot.class, slot.size) {
        (ValueClass::Float, 8) => Instruction::StoreFloatDouble { s: register, a: 1, offset: slot.offset },
        (ValueClass::Float, _) => Instruction::StoreFloatSingle { s: register, a: 1, offset: slot.offset },
        _ => Instruction::StoreWord { s: register, a: 1, offset: slot.offset },
    }
}

/// The set of variable names whose address is taken anywhere in the function.
fn collect_address_taken(function: &Function) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in &function.statements {
        match statement {
            Statement::Store { target, value } => {
                walk(target, &mut names);
                walk(value, &mut names);
            }
            Statement::Expression(expression) => walk(expression, &mut names),
            Statement::Assign { value, .. } => walk(value, &mut names),
            Statement::Switch { scrutinee, .. } => walk(scrutinee, &mut names),
        }
    }
    if let Some(expression) = &function.return_expression {
        walk(expression, &mut names);
    }
    for GuardedReturn { condition, value } in &function.guards {
        walk(condition, &mut names);
        walk(value, &mut names);
    }
    names
}

/// Record `&variable` occurrences within `expression`.
fn walk(expression: &Expression, names: &mut HashSet<String>) {
    match expression {
        Expression::AddressOf { operand } => {
            if let Expression::Variable(name) = operand.as_ref() {
                names.insert(name.clone());
            }
            walk(operand, names);
        }
        Expression::Binary { left, right, .. } => {
            walk(left, names);
            walk(right, names);
        }
        Expression::Unary { operand, .. } => walk(operand, names),
        Expression::Conditional { condition, when_true, when_false } => {
            walk(condition, names);
            walk(when_true, names);
            walk(when_false, names);
        }
        Expression::Cast { operand, .. } => walk(operand, names),
        Expression::Dereference { pointer } => walk(pointer, names),
        Expression::Index { base, index } => {
            walk(base, names);
            walk(index, names);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => walk(base, names),
        Expression::Assign { target, value } => {
            walk(target, names);
            walk(value, names);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                walk(argument, names);
            }
        }
        Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => {}
    }
}
