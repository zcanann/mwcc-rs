//! Core integer expression evaluation and operand placement.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

/// The displacement load for a pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`).
fn displacement_load(pointee: Pointee, d: u8, a: u8, offset: i16) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::LoadWord { d, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZero { d, a, offset },
        Pointee::Short => Instruction::LoadHalfwordAlgebraic { d, a, offset },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZero { d, a, offset },
        Pointee::Float => Instruction::LoadFloatSingle { d, a, offset },
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
    }
}

/// A scalar type as the matching [`Pointee`] (for global loads/stores).
fn pointee_of_type(value_type: Type) -> Option<Pointee> {
    Some(match value_type {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Float => Pointee::Float,
        _ => return None,
    })
}

/// The displacement store for a pointee type (`stw`/`stb`/`sth`/`stfs`).
fn displacement_store(pointee: Pointee, s: u8, a: u8, offset: i16) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::StoreWord { s, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByte { s, a, offset },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfword { s, a, offset },
        Pointee::Float => Instruction::StoreFloatSingle { s, a, offset },
    }
}

/// The indexed store for a pointee type (`stwx`/`stbx`/`sthx`/`stfsx`).
fn indexed_store(pointee: Pointee, s: u8, a: u8, b: u8) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::StoreWordIndexed { s, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByteIndexed { s, a, b },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfwordIndexed { s, a, b },
        Pointee::Float => Instruction::StoreFloatSingleIndexed { s, a, b },
    }
}

impl Generator {

    /// Evaluate an integer expression into general register `destination`.
    pub(crate) fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(destination, *value);
                Ok(())
            }
            Expression::Variable(name) => {
                if let Some(location) = self.locations.get(name) {
                    if location.class != ValueClass::General {
                        return Err(Diagnostic::error(format!("'{name}' is not an integer")));
                    }
                    let (source, width, signed) = (location.register, location.width, location.signed);
                    self.emit_widen(destination, source, width, signed);
                    Ok(())
                } else {
                    self.emit_global_load(name, destination)
                }
            }
            Expression::Unary { operator, operand } => self.emit_unary(*operator, operand, destination),
            Expression::Conditional { condition, when_true, when_false } => {
                self.emit_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { target_type, operand } => self.emit_cast_to_integer(*target_type, operand, destination),
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
            Expression::Member { base, offset, member_type } => self.emit_member_load(base, *offset, *member_type, destination),
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
            Expression::Binary { operator, left, right } => {
                // Comparisons compile to branchless idioms.
                if is_comparison(*operator) {
                    return self.emit_comparison(*operator, left, right, destination);
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
                // A 16-bit constant operand folds into an immediate instruction.
                if self.try_emit_general_with_constant(*operator, left, right, destination)? {
                    return Ok(());
                }
                if !fits_single_scratch(expression, destination == GENERAL_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_general_operands(*operator, left, right, destination)?;
                self.output.instructions.push(general_combine(*operator, destination, operands)?);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literal in integer context")),
        }
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
        // A global pointer: load the pointer value into the destination (an SDA21
        // word load), then dereference it from there, as mwcc does.
        if let Expression::Variable(name) = pointer {
            if !self.locations.contains_key(name) {
                if let Some(Type::Pointer(pointee)) = self.globals.get(name).copied() {
                    // The pointer and the integer result share the destination, so a
                    // float pointee (which needs a separate general register for the
                    // address) is deferred rather than miscompiled.
                    if pointee != Pointee::Float {
                        self.emit_global_load(name, destination)?;
                        self.output.instructions.push(displacement_load(pointee, destination, destination, 0));
                        return Ok(());
                    }
                }
            }
        }
        let (pointee, address) = self.resolve_pointer(pointer)?;
        self.output.instructions.push(displacement_load(pointee, destination, address, 0));
        Ok(())
    }

    /// Emit `base->field` — a displacement load from the struct pointer's register
    /// at the member's offset, choosing the load by the member type. The base must
    /// be a struct-pointer leaf variable (chained/complex bases are roadmap).
    pub(crate) fn emit_member_load(&mut self, base: &Expression, offset: u16, member_type: Type, destination: u8) -> Compilation<()> {
        let address = self.member_base_register(base)?;
        let pointee = pointee_of_type(member_type)
            .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
        self.output.instructions.push(displacement_load(pointee, destination, address, offset as i16));
        Ok(())
    }

    /// The pointee size of a leaf pointer variable, when greater than one byte
    /// (so its arithmetic needs scaling). A byte pointer returns `None` — its
    /// arithmetic is a plain add.
    fn scaled_pointer(&self, operand: &Expression) -> Option<u8> {
        if let Expression::Variable(name) = operand {
            if let Some(location) = self.locations.get(name) {
                let size = location.pointee?.size();
                if size > 1 {
                    return Some(size);
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
    fn pointer_arithmetic_base(&mut self, operand: &Expression) -> Compilation<Option<(u8, u8)>> {
        if let Expression::MemberAddress { base, offset: 0, element } = operand {
            let register = self.member_base_register(base)?;
            return Ok(Some((register, element.size())));
        }
        if let Some(size) = self.scaled_pointer(operand) {
            return Ok(Some((self.general_register_of_leaf(operand)?, size)));
        }
        Ok(None)
    }

    /// Try to emit `pointer ± integer` with the integer scaled by the pointee
    /// size. Returns `false` for non-pointer (or byte leaf-pointer) operands.
    fn try_emit_pointer_arithmetic(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
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
        // Scale the index by the element size (a byte element needs no shift).
        let scaled_register = if size > 1 {
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: integer_register, shift: size.trailing_zeros() as u8 });
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
            _ => Err(Diagnostic::error("struct member base must be a pointer variable (roadmap)")),
        }
    }

    /// Emit `base[index]` into `destination`. A constant index folds into the load
    /// displacement (`lwz r3,8(r3)`); a variable index is scaled by the element
    /// size and uses an indexed load (`slwi r0,rI,2; lwzx r3,rBase,r0`).
    pub(crate) fn emit_subscript(&mut self, base: &Expression, index: &Expression, destination: u8) -> Compilation<()> {
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

    /// Emit a store: `*p = v;` or `p[i] = v;`. The value goes to memory at the
    /// place addressed by the pointer (with a folded displacement for a constant
    /// index, or a scaled indexed store for a variable one).
    pub(crate) fn emit_store(&mut self, target: &Expression, value: &Expression) -> Compilation<()> {
        // `g = v;` — a store to a file-scope global (SDA21 placeholder `0(r0)`).
        if let Expression::Variable(name) = target {
            if let Some(&global_type) = self.globals.get(name.as_str()) {
                let pointee = pointee_of_type(global_type)
                    .ok_or_else(|| Diagnostic::error("global store of this type is not supported yet"))?;
                let source = self.place_store_value(value, pointee)?;
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output.instructions.push(displacement_store(pointee, source, 0, 0));
                return Ok(());
            }
        }
        // `p->field = v;` — a displacement store to the struct member.
        if let Expression::Member { base, offset, member_type } = target {
            let pointee = pointee_of_type(*member_type)
                .ok_or_else(|| Diagnostic::error("struct member store of this type is not supported yet"))?;
            let address = self.member_base_register(base)?;
            let source = self.place_store_value(value, pointee)?;
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
        match index {
            None => {
                let source = self.place_store_value(value, pointee)?;
                self.output.instructions.push(displacement_store(pointee, source, address, 0));
            }
            Some(index) if constant_value(index).is_some() => {
                let offset = i16::try_from(constant_value(index).unwrap() * pointee.size() as i64)
                    .map_err(|_| Diagnostic::error("store offset out of range (roadmap)"))?;
                let source = self.place_store_value(value, pointee)?;
                self.output.instructions.push(displacement_store(pointee, source, address, offset));
            }
            Some(index) => {
                // A variable index uses the scratch for scaling, so the value must
                // be a leaf (it stays in its own register).
                if !matches!(value, Expression::Variable(_)) {
                    return Err(Diagnostic::error("store with a variable index needs a simple value (roadmap)"));
                }
                let source = self.place_store_value(value, pointee)?;
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
    fn place_store_value(&mut self, value: &Expression, pointee: Pointee) -> Compilation<u8> {
        if pointee == Pointee::Float {
            if matches!(value, Expression::Variable(_)) {
                return self.float_register_of_leaf(value);
            }
            self.evaluate_float(value, FLOAT_SCRATCH)?;
            return Ok(FLOAT_SCRATCH);
        }
        if matches!(value, Expression::Variable(_)) {
            return self.general_register_of_leaf(value);
        }
        self.evaluate_general(value, GENERAL_SCRATCH)?;
        Ok(GENERAL_SCRATCH)
    }

    /// Emit a direct call. Arguments are placed in the EABI argument registers,
    /// then `bl name`; the result (in r3 / f1) is moved to `destination` when one
    /// is wanted (a discarded call statement passes `None`).
    pub(crate) fn emit_call(&mut self, name: &str, arguments: &[Expression], destination: Option<u8>, float_result: bool) -> Compilation<()> {
        self.emit_arguments(arguments)?;
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
    fn emit_arguments(&mut self, arguments: &[Expression]) -> Compilation<()> {
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for (index, argument) in arguments.iter().enumerate() {
            if self.is_float_value(argument) {
                self.evaluate_float(argument, next_float)?;
                next_float += 1;
            } else {
                // Honest guard: evaluating into this argument register must not
                // clobber a register a later argument still needs. mwcc handles
                // that (e.g. two members of one struct) by pre-copying the shared
                // base; that choreography is not modeled yet.
                if arguments[index + 1..].iter().any(|later| self.registers_used_by(later).contains(&next_general)) {
                    return Err(Diagnostic::error("argument would clobber a register a later argument needs (roadmap)"));
                }
                self.evaluate_general(argument, next_general)?;
                next_general += 1;
            }
        }
        Ok(())
    }

    /// Whether an expression yields a float (a float leaf, literal, or load).
    fn is_float_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::FloatLiteral(_) => true,
            Expression::Variable(_) => self.is_float_leaf(expression),
            Expression::Dereference { pointer } => matches!(self.pointee_of(pointer), Ok(Pointee::Float)),
            Expression::Index { base, .. } => matches!(self.pointee_of(base), Ok(Pointee::Float)),
            Expression::Member { member_type, .. } => *member_type == Type::Float,
            _ => false,
        }
    }

    /// Load a file-scope global into `destination`. The instruction carries the
    /// `0(r0)` placeholder that an `R_PPC_EMB_SDA21` relocation fills (r13 + the
    /// small-data offset); the load is chosen by the global's type.
    pub(crate) fn emit_global_load(&mut self, name: &str, destination: u8) -> Compilation<()> {
        let global_type = *self.globals.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        self.record_relocation(RelocationKind::EmbSda21, name);
        let instruction = match global_type {
            Type::Int | Type::UnsignedInt => Instruction::LoadWord { d: destination, a: 0, offset: 0 },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZero { d: destination, a: 0, offset: 0 },
            Type::Short => Instruction::LoadHalfwordAlgebraic { d: destination, a: 0, offset: 0 },
            Type::UnsignedShort => Instruction::LoadHalfwordZero { d: destination, a: 0, offset: 0 },
            Type::Float => Instruction::LoadFloatSingle { d: destination, a: 0, offset: 0 },
            // A pointer global is a 32-bit address word.
            Type::Pointer(_) | Type::StructPointer => Instruction::LoadWord { d: destination, a: 0, offset: 0 },
            other => return Err(Diagnostic::error(format!("global of type {other:?} is not supported yet"))),
        };
        self.output.instructions.push(instruction);
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

    pub(crate) fn place_operand(&mut self, operand: &Expression, destination: u8, prefer_destination: bool) -> Compilation<Option<u8>> {
        if let Expression::Variable(name) = operand {
            // A global is loaded into the consumer's register (the destination for
            // addi-family consumers, otherwise the scratch), like a dereference.
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()) {
                let target = if prefer_destination { destination } else { GENERAL_SCRATCH };
                self.emit_global_load(name, target)?;
                return Ok(Some(target));
            }
            let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
            let (register, width, signed) = (location.register, location.width, location.signed);
            if width == 32 {
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
                let Some(source) = self.place_operand(operand, d, false)? else {
                    return Err(Diagnostic::error("negation operand needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::Negate { d, a: source });
            }
            UnaryOperator::BitNot => {
                // ~(~x) == x
                if let Expression::Unary { operator: UnaryOperator::BitNot, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                let Some(source) = self.place_operand(operand, d, false)? else {
                    return Err(Diagnostic::error("complement operand needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::Nor { a: d, s: source, b: source });
            }
            UnaryOperator::LogicalNot => {
                // !(comparison) is the flipped comparison.
                if let Expression::Binary { operator, left, right } = operand {
                    if let Some(flipped) = (is_comparison(*operator)).then(|| flip_comparison(*operator)).flatten() {
                        return self.emit_comparison(flipped, left, right, d);
                    }
                }
                // !x == (x == 0): cntlzw then srwi by 5.
                let Some(source) = self.place_operand(operand, d, false)? else {
                    return Err(Diagnostic::error("logical-not operand needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
            }
        }
        Ok(())
    }
}
