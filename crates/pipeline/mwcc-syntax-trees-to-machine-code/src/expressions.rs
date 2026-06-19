//! Core integer expression evaluation and operand placement.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
use mwcc_target::Eabi;
use mwcc_versions::GlobalAddressing;
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
fn pointee_of_type(value_type: Type) -> Option<Pointee> {
    Some(match value_type {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Float => Pointee::Float,
        // A pointer value is a 4-byte address (stored/loaded with `stw`/`lwz`).
        Type::Pointer(_) | Type::StructPointer => Pointee::UnsignedInt,
        // `double` storage (8-byte lfd/stfd) is a later stage.
        Type::Double => Pointee::Double,
        Type::Void => return None,
    })
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
    pub(crate) fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(destination, *value);
                Ok(())
            }
            // `&x` for a frame-resident variable is its address: `addi d, r1, slot`.
            Expression::AddressOf { operand } => self.emit_address_of(operand, destination),
            Expression::Variable(name) => {
                // A frame-resident variable is reloaded from its stack slot.
                if let Some(slot) = self.frame_slots.get(name).copied() {
                    self.output.instructions.push(Instruction::LoadWord { d: destination, a: 1, offset: slot.offset });
                    return Ok(());
                }
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
            Expression::Assign { target, value } => self.emit_assign(target, value, destination),
            Expression::Binary { operator, left, right } => {
                // Comparisons compile to branchless idioms.
                if is_comparison(*operator) {
                    return self.emit_comparison(*operator, left, right, destination);
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
                // A 16-bit constant operand folds into an immediate instruction.
                if self.try_emit_general_with_constant(*operator, left, right, destination)? {
                    return Ok(());
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
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            if matches!(value, Expression::Variable(_)) {
                return self.float_register_of_leaf(value);
            }
            self.evaluate_float(value, FLOAT_SCRATCH)?;
            return Ok(FLOAT_SCRATCH);
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
    pub(crate) fn is_float_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::FloatLiteral(_) => true,
            Expression::Variable(_) => self.is_float_leaf(expression),
            Expression::Dereference { pointer } => matches!(self.pointee_of(pointer), Ok(Pointee::Float | Pointee::Double)),
            Expression::Index { base, .. } => matches!(self.pointee_of(base), Ok(Pointee::Float | Pointee::Double)),
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
        // `lbz` zero-extends the byte, so the value must be re-signed (`extsb`).
        if self.global_char_extend(name)? {
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
            Type::Pointer(_) | Type::StructPointer => Instruction::LoadWord { d, a, offset: 0 },
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

    pub(crate) fn place_operand(&mut self, operand: &Expression, destination: u8, prefer_destination: bool) -> Compilation<Option<u8>> {
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
            // A narrow operand is width-extended to 32 bits before use. The
            // extension lands in the consumer's working register: the destination
            // for addi-family consumers that keep their operand in place, otherwise
            // the scratch (mwcc routes `extsb r0,rX` ahead of an `rlwinm`/`mulli`).
            let target = if prefer_destination { destination } else { GENERAL_SCRATCH };
            self.emit_widen(target, register, width, signed);
            return Ok(Some(target));
        }
        // A call result already lands in the result register, which is the
        // destination for a tail consumer; compute it there and let the consumer
        // operate in place (mwcc does not bounce it through the scratch).
        if matches!(operand, Expression::Call { .. }) {
            self.evaluate_general(operand, destination)?;
            return Ok(Some(destination));
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
                let source = self.place_operand_or_scratch(operand, d)?;
                self.output.instructions.push(Instruction::Negate { d, a: source });
            }
            UnaryOperator::BitNot => {
                // ~(~x) == x
                if let Expression::Unary { operator: UnaryOperator::BitNot, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                let source = self.place_operand_or_scratch(operand, d)?;
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
                let source = self.place_operand_or_scratch(operand, d)?;
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
            }
        }
        Ok(())
    }
}
