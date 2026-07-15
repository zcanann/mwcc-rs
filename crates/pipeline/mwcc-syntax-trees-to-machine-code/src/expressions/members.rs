//! Struct-member and array-subscript loads/stores (local and global).

#[allow(unused_imports)]
use super::*;

impl Generator {
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
                self.output.instructions.push(displacement_load(pointee, destination, 1, slot.offset + offset as i16)?);
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
                self.output.instructions.push(displacement_load(pointee, destination, destination, offset as i16)?);
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
                        self.output.instructions.push(displacement_load(pointee, destination, 0, 0)?);
                        return Ok(());
                    }
                    self.emit_global_array_base(name, size as u32, destination)?;
                    self.output.instructions.push(displacement_load(pointee, destination, destination, offset as i16)?);
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
        let pointee = pointee_of_type(member_type)
            .ok_or_else(|| Diagnostic::error("unsupported struct member type"))?;
        // `get()->field`: evaluate the call so the returned pointer is in the result register (r3),
        // then ONE member load. Only a single post-call instruction is byte-exact — the enclosing
        // non-leaf epilogue places the LR reload before it, matching the byte-exact `get() + 1`. A
        // signed `char` (lbz + a separate `extsb`), a NESTED member (`get()->b->c`, two loads), or a
        // member STORE (the value must survive the call in a callee-saved register) add post-call
        // instructions whose LR-reload/allocation mwcc schedules differently; those defer — the nested
        // and store contexts reach `member_base_register` with a non-bare-Call base (which has no Call
        // arm), and a signed char hits the guard here.
        if let Expression::Call { .. } = base {
            if matches!(pointee, Pointee::Char) {
                return Err(Diagnostic::error("a signed-char member through a call base needs the sign-extend epilogue schedule (roadmap)"));
            }
            let result = Eabi::general_result().number;
            self.evaluate_general(base, result)?;
            self.output.instructions.push(displacement_load(pointee, destination, result, offset as i16)?);
            return Ok(());
        }
        let address = self.member_base_register(base)?;
        self.output.instructions.push(displacement_load(pointee, destination, address, offset as i16)?);
        Ok(())
    }

    /// `array[index].field` for an array/pointer of structs: scale `index` by the
    /// struct `stride`, add to the array base, and load the member at `offset`.
    pub(crate) fn emit_indexed_member_load(&mut self, array: &Expression, index: &Expression, stride: u16, offset: u16, member_type: Type, destination: u8) -> Compilation<()> {
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
            self.output.instructions.push(indexed_load(pointee, destination, array_register, GENERAL_SCRATCH)?);
        } else {
            self.output.instructions.push(Instruction::Add { d: array_register, a: array_register, b: GENERAL_SCRATCH });
            self.output.instructions.push(displacement_load(pointee, destination, array_register, offset as i16)?);
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
    pub(crate) fn emit_global_indexed_member_load(&mut self, name: &str, total_size: u32, index: &Expression, stride: u16, offset: u16, member_type: Type, destination: u8) -> Compilation<()> {
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
            self.output.instructions.push(displacement_load(pointee, destination, destination, total)?);
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
            self.output.instructions.push(indexed_load(pointee, destination, destination, GENERAL_SCRATCH)?);
        } else {
            self.output.instructions.push(Instruction::Add { d: destination, a: destination, b: GENERAL_SCRATCH });
            self.output.instructions.push(displacement_load(pointee, destination, destination, offset as i16)?);
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
    pub(crate) fn emit_global_indexed_member_store(&mut self, name: &str, total_size: u32, index: &Expression, stride: u16, offset: u16, pointee: Pointee, value: &Expression) -> Compilation<()> {
        if let Some(constant) = constant_value(index) {
            // A constant store value interleaves its `li` between the base's `lis` and
            // `addi` (`lis; li; addi; stw`) — that schedule is not modeled, so defer;
            // a register value (the base materializes whole, then `stw`) is byte-exact.
            if !matches!(value, Expression::Variable(_)) {
                return Err(Diagnostic::error("a global struct-array member store at a constant index needs a register value (roadmap)"));
            }
            let total = i16::try_from(constant * stride as i64 + offset as i64)
                .map_err(|_| Diagnostic::error("struct-array member store offset out of range (roadmap)"))?;
            // Phase D: the base is a virtual; the reserve marker keeps the legacy
            // physical chooser (place_store_value) off the virtual's field value.
            let base = self.fresh_virtual_general();
            let restore = self.reserved.insert(base);
            self.emit_global_array_base(name, total_size, base)?;
            let source = self.place_store_value(value, pointee)?;
            if restore {
                self.reserved.remove(&base);
            }
            self.output.instructions.push(displacement_store(pointee, source, base, total)?);
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
        // `@ha` is a VIRTUAL the allocator places: the index/value pinned ranges force
        // it past them, and a constant value's reuse of the register is the same vreg
        // redefined (one spanning range — the allocator keeps the home).
        let high = self.fresh_virtual_general();
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
            self.output.instructions.push(indexed_store(pointee, source, index_register, GENERAL_SCRATCH)?);
        } else {
            self.output.instructions.push(Instruction::Add { d: index_register, a: index_register, b: GENERAL_SCRATCH });
            self.output.instructions.push(displacement_store(pointee, source, index_register, offset as i16)?);
        }
        Ok(())
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
            // A bare `get()->field` is handled in emit_member_load (single-load, byte-exact); any
            // OTHER call context reaching here (a nested `get()->b->c`, an indexed `get()->a[i]`, a
            // member store) has a post-call schedule mwcc places differently — defer.
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
                self.output.instructions.push(displacement_load(*element, destination, address, total)?);
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
                self.output.instructions.push(indexed_load(*element, destination, address, scaled)?);
            } else {
                self.output.instructions.push(Instruction::Add { d: address, a: address, b: scaled });
                self.output.instructions.push(displacement_load(*element, destination, address, *offset as i16)?);
            }
            return Ok(());
        }
        let (pointee, address) = self.resolve_pointer(base)?;
        if let Some(constant) = constant_value(index) {
            let offset = constant * pointee.size() as i64;
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("subscript offset out of range (roadmap)"))?;
            self.output.instructions.push(displacement_load(pointee, destination, address, offset)?);
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
                    self.output.instructions.push(displacement_load(pointee, destination, address, offset)?);
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
                self.output.instructions.push(indexed_load(pointee, destination, address, scaled)?);
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
        self.output.instructions.push(indexed_load(pointee, destination, address, scaled)?);
        Ok(())
    }

    /// `g[index]` for a file-scope array global `g`: materialize `g`'s base address
    /// into `destination` (SDA21 for a small `.sdata` array, ADDR16 `lis`/`addi` for
    /// a large `.data` one — by total size), then load the element. A constant index
    /// folds into the load displacement; a variable index needs mwcc's scale/base
    /// scheduling interleave, which is not modeled yet, so it defers.
    pub(crate) fn emit_global_array_subscript(&mut self, name: &str, total_size: u32, index: &Expression, destination: u8) -> Compilation<()> {
        let element_type = self.globals[name];
        let pointee = pointee_of_type(element_type)
            .ok_or_else(|| Diagnostic::error("a global array of this element type is not supported yet (roadmap)"))?;
        // The base materializes into `destination` and is then its own load base, so
        // `destination` cannot be the scratch r0 (an `addi`/load based on r0 reads
        // literal zero, not the register). A BYTE element's base is a separate
        // (virtual) register, so its variable-index path below tolerates r0.
        if destination == GENERAL_SCRATCH && !(pointee.size() == 1 && constant_value(index).is_none()) {
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
                self.output.instructions.push(displacement_load(pointee, destination, 0, 0)?);
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
                    self.output.instructions.push(displacement_load(pointee, destination, base, 0)?);
                } else {
                    self.emit_global_array_base(name, total_size, base)?;
                    self.output.instructions.push(displacement_load(pointee, destination, base, offset)?);
                }
                return Ok(());
            }
            self.emit_global_array_base(name, total_size, destination)?;
            self.output.instructions.push(displacement_load(pointee, destination, destination, offset)?);
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
            // A BYTE element needs no scale: the index feeds lbzx raw. Measured
            // (ADDR16, the ctype table shape):
            //   plain:  lis b,@ha; addi b,b,@l; lbzx dest,b,index    (one free base)
            //   cast:   lis h,@ha; clrlwi r0,i,24; addi b,h,@l; lbzx dest,b,r0
            // — the u8 cast stages through r0 in the lis latency, and the base's
            // addi lands in the register the dead index frees (allocator-chosen).
            let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
            if small {
                return Err(Diagnostic::error("a variable subscript of a SMALL byte global array is not supported yet (roadmap)"));
            }
            let byte_normalized = match index {
                Expression::Cast { target_type: Type::UnsignedChar, operand } => Some(operand.as_ref()),
                // `i & 0xFF` normalizes identically (the same clrlwi — measured).
                Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                    if constant_value(right) == Some(0xff) => Some(left.as_ref()),
                // A NARROW UNSIGNED parameter index arrives unextended and mwcc
                // re-extends it exactly like the cast (measured: BfBB islower's
                // `unsigned char c` param -> clrlwi r0,r3,24 before the lbzx).
                Expression::Variable(name)
                    if self.locations.get(name.as_str()).is_some_and(|location| location.width == 8 && !location.signed) =>
                {
                    Some(index)
                }
                _ => None,
            };
            // A SIGNED narrow index would need extsb before the lbzx — unprobed.
            if let Expression::Variable(name) = index {
                if self.locations.get(name.as_str()).is_some_and(|location| location.width < 32 && location.signed) {
                    return Err(Diagnostic::error("a signed narrow parameter as an array index is not supported yet (roadmap)"));
                }
                if self.locations.get(name.as_str()).is_some_and(|location| location.width == 16 && !location.signed) {
                    return Err(Diagnostic::error("an unsigned short parameter as a byte-array index is not supported yet (roadmap)"));
                }
            }
            if let Some(operand) = byte_normalized {
                let source = self.general_register_of_leaf(operand)?;
                let high = self.fresh_virtual_general();
                self.emit_address_high(high, name);
                self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: source, clear: 24 });
                let base = self.fresh_virtual_general();
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate { d: base, a: high, immediate: 0 });
                self.output.instructions.push(indexed_load(pointee, destination, base, GENERAL_SCRATCH)?);
                return Ok(());
            }
            let index_register = self.general_register_of_leaf(index)?;
            let base = self.fresh_virtual_general();
            self.emit_address_high(base, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: base, a: base, immediate: 0 });
            self.output.instructions.push(indexed_load(pointee, destination, base, index_register)?);
            return Ok(());
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
        self.output.instructions.push(indexed_load(pointee, destination, base_gpr, GENERAL_SCRATCH)?);
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
    pub(crate) fn emit_global_array_base(&mut self, name: &str, total_size: u32, dest: u8) -> Compilation<()> {
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
    pub(crate) fn emit_global_array_store(&mut self, name: &str, total_size: u32, index: &Expression, value: &Expression) -> Compilation<()> {
        let element_type = self.globals[name];
        let pointee = pointee_of_type(element_type)
            .ok_or_else(|| Diagnostic::error("a global array of this element type is not supported yet (roadmap)"))?;
        // A float/double LITERAL element store at a CONSTANT index. mwcc materializes the value
        // (`lfs`/`lfd` from the `.sdata2` pool) and the array base, scheduling the value load
        // relative to the base differently per shape (all verified version-invariant across
        // 1.3.2/2.6/2.7). `place_store_value` emits the width-correct load and yields the FPR.
        //   - large (ADDR16) array, offset 0:  value ; lis base,name@ha ; stf val,name@l(base)
        //     (`@l` folds into the store's displacement, so no `addi`)
        //   - large (ADDR16) array, offset N:  lis base,name@ha ; value ; addi base,base,name@l ;
        //     stf val,N(base)  (the value load fills the slot between the `lis` and the `addi`)
        //   - small (SDA) array (total <= 8):  value ; li base,name@sda21 ; stf val,offset(base)
        if matches!(pointee, Pointee::Float | Pointee::Double) && matches!(value, Expression::FloatLiteral(_)) {
            if let Some(constant_index) = constant_value(index) {
                let offset = constant_index * pointee.size() as i64;
                let offset = i16::try_from(offset)
                    .map_err(|_| Diagnostic::error("array subscript out of range (roadmap)"))?;
                let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
                // Element 0 of a SMALL (SDA) array folds the relocation directly into the store,
                // like a scalar global — no base register (`lfs val; stf val,name@sda21(r0)`).
                if small && offset == 0 {
                    let source = self.place_store_value(value, pointee)?;
                    self.record_relocation(RelocationKind::EmbSda21, name);
                    self.output.instructions.push(displacement_store(pointee, source, 0, 0)?);
                    return Ok(());
                }
                let base = self.fresh_virtual_general();
                let restore = self.reserved.insert(base);
                if small {
                    // value ; li base,name@sda21 ; stf val,offset(base)
                    let source = self.place_store_value(value, pointee)?;
                    self.record_relocation(RelocationKind::EmbSda21, name);
                    self.output.instructions.push(Instruction::AddImmediate { d: base, a: 0, immediate: 0 });
                    if restore { self.reserved.remove(&base); }
                    self.output.instructions.push(displacement_store(pointee, source, base, offset)?);
                } else if offset == 0 {
                    // value ; lis base,name@ha ; stf val,name@l(base)  (`@l` folds into the store)
                    let source = self.place_store_value(value, pointee)?;
                    self.emit_address_high(base, name);
                    if restore { self.reserved.remove(&base); }
                    self.record_relocation(RelocationKind::Addr16Lo, name);
                    self.output.instructions.push(displacement_store(pointee, source, base, 0)?);
                } else {
                    // lis base,name@ha ; value ; addi base,base,name@l ; stf val,offset(base)
                    self.emit_address_high(base, name);
                    let source = self.place_store_value(value, pointee)?;
                    self.record_relocation(RelocationKind::Addr16Lo, name);
                    self.output.instructions.push(Instruction::AddImmediate { d: base, a: base, immediate: 0 });
                    if restore { self.reserved.remove(&base); }
                    self.output.instructions.push(displacement_store(pointee, source, base, offset)?);
                }
                return Ok(());
            }
        }
        // A CONSTANT value over a VARIABLE index on a large (ADDR16) array is handled in
        // the variable-index path below: the constant materializes into the freed
        // base-high register after the `addi` — `lis r4,@ha; slwi r0,i,2; addi r3,r4,@lo;
        // li r4,C; stwx r4,r3,r0`. Any other non-register value (a computed value, or a
        // constant with a constant index / small array) interleaves through the
        // scheduler in unmodeled orders — defer.
        let constant_store_value = if matches!(value, Expression::Variable(_)) {
            None
        } else {
            Some(
                constant_value(value)
                    .and_then(|constant| i16::try_from(constant).ok())
                    .ok_or_else(|| Diagnostic::error("a global-array store of a non-register value is not supported yet (needs the value/base scheduler)"))?,
            )
        };
        if constant_store_value.is_some()
            && (constant_value(index).is_some()
                || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8))
        {
            return Err(Diagnostic::error("a global-array constant store of this shape is not supported yet (needs the value/base scheduler)"));
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
                self.output.instructions.push(displacement_store(pointee, source, 0, 0)?);
                return Ok(());
            }
            let base = self.fresh_virtual_general();
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
                self.output.instructions.push(displacement_store(pointee, source, base, 0)?);
                return Ok(());
            }
            self.emit_global_array_base(name, total_size, base)?;
            let source = self.place_store_value(value, pointee)?;
            if restore {
                self.reserved.remove(&base);
            }
            self.output.instructions.push(displacement_store(pointee, source, base, offset)?);
            return Ok(());
        }
        // Variable index: the base reuses the (scaled-away) index register and the value stores
        // through it — `stwx`/`stfsx`/`stfdx`. A byte element defers (an unscaled byte index can
        // alias the base register).
        let size = pointee.size();
        if size == 1 {
            return Err(Diagnostic::error("a variable store to a byte global array is not supported yet (roadmap)"));
        }
        // A CONSTANT value (large array; rejected above otherwise): the constant
        // materializes into the freed base-high register after the `addi` —
        // `lis r4,@ha; slwi r0,i,2; addi r3,r4,@lo; li r4,C; stwx r4,r3,r0`. An index
        // with a folded constant offset (`arr[i-1] = 0`) adds the scaled index into the
        // base and rides the element offset on the store's displacement instead:
        // `…; li r4,C; add r3,r3,r0; stw r4,-4(r3)`.
        if let Some(constant) = constant_store_value {
            if matches!(pointee, Pointee::Float | Pointee::Double) {
                return Err(Diagnostic::error("a float global-array constant store is not supported yet (roadmap)"));
            }
            let mut index_leaf = index;
            let mut element_offset: i64 = 0;
            if let Expression::Binary { operator, left, right } = index {
                if let Some(k) = constant_value(right) {
                    match operator {
                        BinaryOperator::Add => {
                            index_leaf = left.as_ref();
                            element_offset = k * size as i64;
                        }
                        BinaryOperator::Subtract => {
                            index_leaf = left.as_ref();
                            element_offset = -k * size as i64;
                        }
                        _ => {}
                    }
                }
            }
            let offset = i16::try_from(element_offset)
                .map_err(|_| Diagnostic::error("a global-array element offset out of displacement range (roadmap)"))?;
            let index_register = self.general_register_of_leaf(index_leaf)?;
            let shift = size.trailing_zeros() as u8;
            // Phase D migration: the base-high register is a VIRTUAL the allocator
            // places (its live range overlaps the pinned index register, so linear
            // scan lands on the same free register the inline choice picked).
            let high = self.fresh_virtual_general();
            self.emit_address_high(high, name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: high, a: 0, immediate: constant });
            if offset == 0 {
                self.output.instructions.push(indexed_store(pointee, high, index_register, GENERAL_SCRATCH)?);
            } else {
                self.output.instructions.push(Instruction::Add { d: index_register, a: index_register, b: GENERAL_SCRATCH });
                self.output.instructions.push(displacement_store(pointee, high, index_register, offset)?);
            }
            return Ok(());
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
            let high = self.fresh_virtual_general();
            self.emit_address_high(high, name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
        }
        self.output.instructions.push(indexed_store(pointee, value_register, index_register, GENERAL_SCRATCH)?);
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
    pub(crate) fn try_emit_indexed_rmw(&mut self, target: &Expression, value: &Expression) -> Compilation<bool> {
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
            self.output.instructions.push(indexed_load(pointee, scratch, address, scaled)?);
            let combined = match operator {
                Add => Instruction::Add { d: scratch, a: scratch, b: rhs_register },
                Subtract => Instruction::SubtractFrom { d: scratch, a: rhs_register, b: scratch },
                Multiply => Instruction::MultiplyLow { d: scratch, a: scratch, b: rhs_register },
                BitAnd => Instruction::And { a: scratch, s: scratch, b: rhs_register },
                BitOr => Instruction::Or { a: scratch, s: scratch, b: rhs_register },
                _ => Instruction::Xor { a: scratch, s: scratch, b: rhs_register },
            };
            self.output.instructions.push(combined);
            self.output.instructions.push(indexed_store(pointee, scratch, address, scaled)?);
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
                self.output.instructions.push(indexed_load(pointee, loaded, address, scaled)?);
                self.output.instructions.push(Instruction::AddImmediate { d: scratch, a: loaded, immediate });
                self.output.instructions.push(indexed_store(pointee, scratch, address, scaled)?);
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
                // `a[i] *= 2^k` strength-reduces to a left shift, like every other multiply
                // context (`slwi r0,r0,k`), NOT `mulli`; a non-power-of-two keeps `mulli`.
                Multiply if constant > 1 && (constant & (constant - 1)) == 0 => Instruction::ShiftLeftImmediate { a: scratch, s: scratch, shift: constant.trailing_zeros() as u8 },
                Multiply if i16::try_from(constant).is_ok() => Instruction::MultiplyImmediate { d: scratch, a: scratch, immediate: constant as i16 },
                BitAnd => match rlwinm_mask(constant) {
                    Some((begin, end)) => Instruction::RotateAndMask { a: scratch, s: scratch, shift: 0, begin, end },
                    None => return Ok(false),
                },
                _ => return Ok(false),
            };
            let scaled = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: scaled, s: index_register, shift: size_shift });
            self.output.instructions.push(indexed_load(pointee, scratch, address, scaled)?);
            self.output.instructions.push(immediate_op);
            self.output.instructions.push(indexed_store(pointee, scratch, address, scaled)?);
            return Ok(true);
        }
        Ok(false)
    }

}
