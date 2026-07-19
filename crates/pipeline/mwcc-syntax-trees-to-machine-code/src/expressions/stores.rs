//! Assignment and store placement, comma side effects.

use super::implicit_narrow_store::legacy_narrow_store_binary_alu;
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit `target = value` as an expression: compute `value` into the
    /// destination, store it to `target`, and leave the value in the destination
    /// (so the surrounding expression can use it). Global targets only for now.
    pub(crate) fn emit_assign(
        &mut self,
        target: &Expression,
        value: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        if let Expression::Variable(name) = target {
            if let Some(&global_type) = self.globals.get(name.as_str()) {
                let pointee = pointee_of_type(global_type).ok_or_else(|| {
                    Diagnostic::error("global assignment of this type is not supported yet")
                })?;
                self.evaluate_general(value, destination)?;
                self.emit_global_store(name, pointee, destination)?;
                return Ok(());
            }
        }
        Err(Diagnostic::error(
            "assignment as an expression supports a global target (roadmap)",
        ))
    }

    /// Emit an SDA-global store of a value already evaluated into `source`. The
    /// computed-store-fill path evaluates both values (into a virtual and the scratch)
    /// *before* the stores, so it places the store separately from the value.
    pub(crate) fn emit_sda_global_store_from(
        &mut self,
        name: &str,
        pointee: Pointee,
        source: u8,
    ) -> Compilation<()> {
        self.record_relocation(RelocationKind::EmbSda21, name);
        self.output
            .instructions
            .push(displacement_store(pointee, source, 0, 0)?);
        Ok(())
    }

    pub(crate) fn emit_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<()> {
        let (value, indexed_update_syntax) = match value {
            Expression::IndexedUpdateValue { value } => (value.as_ref(), true),
            value => (value, false),
        };
        // `*(T *)0xADDR = v` — a constant-address store (memory-mapped registers, the GX FIFO).
        // mwcc materializes the address base before the value (`lis base, hi`), keeping the base
        // GPR clear of the value's inputs, then stores `st value, lo(base)`. Mirrors the absolute
        // global store, with a numeric hi/lo split in place of `@ha`/`@l` relocations.
        if let Expression::Dereference { pointer } = target {
            if let Some((pointee, address)) = const_address_pointer(pointer) {
                if self.emit_const_address_store(pointee, address, 0, value)? {
                    return Ok(());
                }
                return Err(Diagnostic::error(
                    "a constant-address store needing base reuse is not supported yet (roadmap)",
                ));
            }
        }
        // `(*(struct S *)0xADDR).field = v` — store to a member of a constant-address pointer.
        // Same idiom as the plain const-address store, with the member offset folded into the
        // displacement (the GX FIFO union store `(*(PPCWGPipe*)ADDR).u8 = v` is offset 0).
        if let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } = target
        {
            if let Some(address) = const_address_of(base) {
                if let Some(pointee) = pointee_of_type(*member_type) {
                    // Every width, including FLOAT/DOUBLE: emit_const_address_store folds the
                    // member offset into a `stfs/stfd f,disp(base)` off the materialized `lis`
                    // base just as it does the integer `stb/sth/stw` (measured: the GX FIFO
                    // `(*(PPCWGPipe*)0xCC008000).f32 = x` -> `lis r3,0xCC01; stfs f1,-0x8000(r3)`).
                    if self.emit_const_address_store(pointee, address, *offset, value)? {
                        return Ok(());
                    }
                    return Err(Diagnostic::error("a constant-address member store needing base reuse is not supported yet (roadmap)"));
                }
            }
        }
        // `*(p + i) = v` is `p[i] = v`: rewrite a pointer-plus-index dereference target to the
        // subscript store, the symmetric counterpart of the load routing in
        // emit_load_from_pointer. The pointer operand is the base, the integer the index; `+`
        // commutes. The store truncates a narrow value (stb/sth), so unlike the LOAD this has
        // no sign-extension hazard — the rewritten Index store handles every pointee width.
        if let Expression::Dereference { pointer } = target {
            if let Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } = pointer.as_ref()
            {
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
            if let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } = pointer.as_ref()
            {
                if let Some(constant) = constant_value(right) {
                    if self.dereferenced_width(left).is_some() {
                        let index = Box::new(Expression::IntegerLiteral(-constant));
                        return self.emit_store(
                            &Expression::Index {
                                base: left.clone(),
                                index,
                            },
                            value,
                        );
                    }
                }
            }
        }
        // A type-pun store through a frame-resident address (`*(int*)&x = v`) is a
        // plain displacement store to r1.
        if let Expression::Dereference { pointer } = target {
            if let Some((pointee, offset)) = self.resolve_frame_pointer(pointer) {
                let source = self.place_store_value(value, pointee)?;
                self.output
                    .instructions
                    .push(displacement_store(pointee, source, 1, offset)?);
                self.written_slots.insert(offset);
                return Ok(());
            }
        }
        // `g = v;` — a store to a file-scope global.
        if let Expression::Variable(name) = target {
            if let Some(&global_type) = self.globals.get(name.as_str()) {
                let pointee = pointee_of_type(global_type).ok_or_else(|| {
                    Diagnostic::error("global store of this type is not supported yet")
                })?;
                match self.behavior.global_addressing {
                    GlobalAddressing::SmallData => {
                        let source = match self
                            .try_legacy_narrow_global_compound_shift(name, pointee, value)?
                        {
                            Some(source) => source,
                            None => self.place_store_value(value, pointee)?,
                        };
                        // Build 163 creates the implicit integer callee while
                        // lowering the conversion, before it creates the float
                        // store target. Later builds retain AST target-first
                        // traversal for these two symbols.
                        if self.behavior.int_call_result_conversion_style
                            == mwcc_versions::IntCallResultConversionStyle::LegacyBiasFirst
                            && matches!(pointee, Pointee::Float | Pointee::Double)
                        {
                            if let Expression::Call {
                                name: callee,
                                arguments,
                            } = value
                            {
                                if arguments.is_empty()
                                    && !self.prototyped_names.contains(callee)
                                    && !is_intrinsic_call(callee)
                                    && !matches!(
                                        self.call_return_types.get(callee),
                                        Some(Type::Float | Type::Double)
                                    )
                                {
                                    self.output.symbol_order =
                                        vec![callee.clone(), name.clone()];
                                    self.output
                                        .early_implicit_external_callees
                                        .push(callee.clone());
                                }
                            }
                        }
                        self.record_relocation(RelocationKind::EmbSda21, name);
                        self.output
                            .instructions
                            .push(displacement_store(pointee, source, 0, 0)?);
                        // The stored value is still in `source`; a following read of
                        // this global reuses it (mwcc does not reload here).
                        self.stored_globals
                            .insert(name.clone(), (source, self.output.instructions.len()));
                    }
                    GlobalAddressing::Absolute => {
                        // mwcc materializes the address base before the value, so the
                        // base GPR (chosen to avoid the value's input registers) is
                        // reserved while the value is placed.
                        let base = self.fresh_virtual_general();
                        let restore = self.reserved.insert(base);
                        self.emit_address_high(base, name);
                        let source = self.place_store_value(value, pointee)?;
                        if restore {
                            self.reserved.remove(&base);
                        }
                        self.record_relocation(RelocationKind::Addr16Lo, name);
                        self.output
                            .instructions
                            .push(displacement_store(pointee, source, base, 0)?);
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
                // `__EXIRegs[index] = value;` — a store to a fixed-address (hardware register) array.
                if let Some(&(address, element_type)) = self.fixed_address_arrays.get(name.as_str())
                {
                    if let Some(element) = pointee_of_type(element_type) {
                        if self.emit_fixed_address_array_subscript_store(
                            element, address, index, value,
                        )? {
                            return Ok(());
                        }
                    }
                }
            }
            // `((T *)ADDR)[index] = value;` — a store to a constant-address (hardware register) pointer.
            if let Expression::Cast {
                target_type: Type::Pointer(element),
                operand,
            } = base.as_ref()
            {
                if let Some(address) = constant_value(operand) {
                    if self.emit_const_address_subscript_store(
                        *element,
                        address as u32,
                        index,
                        value,
                    )? {
                        return Ok(());
                    }
                }
            }
        }
        // Variable-index word read/modify/write, retaining update-vs-explicit
        // syntax for generation-specific instruction selection.
        if self.try_emit_indexed_rmw(target, value, indexed_update_syntax)? {
            return Ok(());
        }
        // `a[i].field = v;` — scale the index by the struct size, then store at the
        // field offset (`stwx` for a zero offset, else `add; stw`). The value is
        // placed after the scale, before the address add — mwcc's order.
        if let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: Some(stride),
        } = target
        {
            if let Expression::Index { base: array, index } = base.as_ref() {
                let pointee = pointee_of_type(*member_type).ok_or_else(|| {
                    Diagnostic::error("struct member store of this type is not supported yet")
                })?;
                // A file-scope struct array `arr[i].field = v`: materialize the base
                // with the interleaved schedule, then store at the member offset.
                if let Expression::Variable(name) = array.as_ref() {
                    if let Some(&total_size) = self.global_array_sizes.get(name.as_str()) {
                        return self.emit_global_indexed_member_store(
                            name, total_size, index, *stride, *offset, pointee, value,
                        );
                    }
                }
                let array_register = self.general_register_of_leaf(array)?;
                let index_register = self.general_register_of_leaf(index)?;
                if stride.is_power_of_two() {
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: GENERAL_SCRATCH,
                            s: index_register,
                            shift: stride.trailing_zeros() as u8,
                        });
                } else {
                    self.output
                        .instructions
                        .push(Instruction::MultiplyImmediate {
                            d: GENERAL_SCRATCH,
                            a: index_register,
                            immediate: *stride as i16,
                        });
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
                    return Err(Diagnostic::error(
                        "indexed-member store of a computed value is not supported yet (roadmap)",
                    ));
                };
                if *offset == 0 {
                    self.output.instructions.push(indexed_store(
                        pointee,
                        source,
                        array_register,
                        GENERAL_SCRATCH,
                    )?);
                } else {
                    self.output.instructions.push(Instruction::Add {
                        d: array_register,
                        a: array_register,
                        b: GENERAL_SCRATCH,
                    });
                    self.output.instructions.push(displacement_store(
                        pointee,
                        source,
                        array_register,
                        *offset as i16,
                    )?);
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
        if let Expression::Member {
            base,
            index_stride: None,
            ..
        } = target
        {
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
        if let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } = target
        {
            if let Expression::Variable(name) = base.as_ref() {
                if !self.locations.contains_key(name.as_str()) {
                    let global_type = self.globals.get(name.as_str()).copied();
                    let struct_value_size = match global_type {
                        Some(Type::StructPointer { .. }) => None,
                        Some(Type::Struct { size, .. }) => Some(size as u32),
                        _ => None,
                    };
                    let is_global_struct_base = matches!(
                        global_type,
                        Some(Type::StructPointer { .. } | Type::Struct { .. })
                    );
                    if is_global_struct_base {
                        let pointee = pointee_of_type(*member_type).ok_or_else(|| {
                            Diagnostic::error(
                                "struct member store of this type is not supported yet",
                            )
                        })?;
                        // A small (<= 8 byte, SDA-addressed) global struct VALUE:
                        // mwcc materializes the stored VALUE first, then the base. An
                        // offset-0 store folds the SDA21 into the store itself
                        // (`stw src, g@sda21`, no base register), mirroring the offset-0
                        // member load; a non-zero offset materializes g's SDA base and
                        // stores at the displacement.
                        if let Some(size) = struct_value_size {
                            if size <= 8
                                && matches!(
                                    self.behavior.global_addressing,
                                    GlobalAddressing::SmallData
                                )
                            {
                                let source = self.place_store_value(value, pointee)?;
                                if *offset == 0 {
                                    self.record_relocation(RelocationKind::EmbSda21, name);
                                    self.output
                                        .instructions
                                        .push(displacement_store(pointee, source, 0, 0)?);
                                } else {
                                    let restore = self.reserved.insert(source);
                                    let base_reg = self.fresh_virtual_general();
                                    self.emit_global_array_base(name, size, base_reg)?;
                                    if restore {
                                        self.reserved.remove(&source);
                                    }
                                    self.output.instructions.push(displacement_store(
                                        pointee,
                                        source,
                                        base_reg,
                                        *offset as i16,
                                    )?);
                                }
                                return Ok(());
                            }
                            // A large (ADDR16) global struct VALUE materializes the
                            // base address, then the value, then stores at the offset. A
                            // register value matches mwcc; a *constant* value is a known
                            // latent diff — mwcc folds `@l` into the store and interleaves
                            // the `li` between `lis` and the store (a follow-up).
                            let base_reg = self.fresh_virtual_general();
                            let restore = self.reserved.insert(base_reg);
                            self.emit_global_array_base(name, size, base_reg)?;
                            let source = self.place_store_value(value, pointee)?;
                            if restore {
                                self.reserved.remove(&base_reg);
                            }
                            self.output.instructions.push(displacement_store(
                                pointee,
                                source,
                                base_reg,
                                *offset as i16,
                            )?);
                            return Ok(());
                        }
                        // struct POINTER base: load the pointer, then the value, then store.
                        let base_reg = self.fresh_virtual_general();
                        let restore = self.reserved.insert(base_reg);
                        self.emit_global_load_value(name, base_reg)?;
                        let source = self.place_store_value(value, pointee)?;
                        if restore {
                            self.reserved.remove(&base_reg);
                        }
                        self.output.instructions.push(displacement_store(
                            pointee,
                            source,
                            base_reg,
                            *offset as i16,
                        )?);
                        return Ok(());
                    }
                }
            }
        }
        // `p->field = v;` — a displacement store to the struct member.
        if let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } = target
        {
            let pointee = pointee_of_type(*member_type).ok_or_else(|| {
                Diagnostic::error("struct member store of this type is not supported yet")
            })?;
            let address = self.member_base_register(base)?;
            // The base register is live for the store, so reserve it while the value is
            // placed — otherwise a value that needs a temporary (a magic-number divide)
            // could pick it and clobber the store address.
            let restore = address != GENERAL_SCRATCH && self.reserved.insert(address);
            let source = self.place_store_value(value, pointee)?;
            if restore {
                self.reserved.remove(&address);
            }
            self.output.instructions.push(displacement_store(
                pointee,
                source,
                address,
                *offset as i16,
            )?);
            return Ok(());
        }
        // `p->arr[index] = value` — store to an array member, folding the array
        // offset into the displacement just like the array load.
        if let Expression::Index {
            base: index_base,
            index,
        } = target
        {
            if let Expression::MemberAddress {
                base: struct_base,
                offset,
                element,
            } = index_base.as_ref()
            {
                let address = self.member_base_register(struct_base)?;
                if let Some(constant) = constant_value(index) {
                    let total = i16::try_from(*offset as i64 + constant * element.size() as i64)
                        .map_err(|_| Diagnostic::error("array store out of range (roadmap)"))?;
                    let source = self.place_store_value(value, *element)?;
                    self.output
                        .instructions
                        .push(displacement_store(*element, source, address, total)?);
                    return Ok(());
                }
                if !matches!(value, Expression::Variable(_)) {
                    return Err(Diagnostic::error(
                        "array store with a variable index needs a simple value (roadmap)",
                    ));
                }
                let source = self.place_store_value(value, *element)?;
                let index_register = self.general_register_of_leaf(index)?;
                let size = element.size();
                let scaled = if size == 1 {
                    index_register
                } else {
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: GENERAL_SCRATCH,
                            s: index_register,
                            shift: size.trailing_zeros() as u8,
                        });
                    GENERAL_SCRATCH
                };
                if *offset == 0 {
                    self.output
                        .instructions
                        .push(indexed_store(*element, source, address, scaled)?);
                } else {
                    self.output.instructions.push(Instruction::Add {
                        d: address,
                        a: address,
                        b: scaled,
                    });
                    self.output.instructions.push(displacement_store(
                        *element,
                        source,
                        address,
                        *offset as i16,
                    )?);
                }
                return Ok(());
            }
        }
        let (base, index) = match target {
            Expression::Dereference { pointer } => (pointer.as_ref(), None),
            Expression::Index { base, index } => (base.as_ref(), Some(index.as_ref())),
            _ => {
                return Err(Diagnostic::error(
                    "store target must be `*p`, `p[i]`, a member, or a global",
                ))
            }
        };
        // The store's pointer address (a param/local pointer) is resolved into a volatile
        // register BEFORE the value is placed. A call in the value clobbers every volatile
        // register, including that address — mwcc preserves it in a callee-saved register
        // across the call (`mr r31,r3; bl; stw r3,0(r31)`). We do not save callee registers
        // yet, so storing through the clobbered address would miscompile; defer instead. A
        // re-materializable address (a global/constant pointer) is handled by an earlier
        // branch and never reaches here, so this cannot regress a byte-exact case.
        if expression_has_call(value) {
            return Err(Diagnostic::error("a store through a register pointer whose value contains a call needs callee-saved preservation (roadmap)"));
        }
        let (pointee, address) = self.resolve_pointer(base)?;
        // The address register is live for the store; reserve it while the value is
        // placed so a value needing a temporary (e.g. a magic-number divide) can't pick
        // it and clobber the store address.
        let restore = address != GENERAL_SCRATCH && self.reserved.insert(address);
        match index {
            None => {
                let source = self.place_store_value(value, pointee)?;
                if restore {
                    self.reserved.remove(&address);
                }
                self.output
                    .instructions
                    .push(displacement_store(pointee, source, address, 0)?);
            }
            Some(index) if constant_value(index).is_some() => {
                let offset = i16::try_from(constant_value(index).unwrap() * pointee.size() as i64)
                    .map_err(|_| Diagnostic::error("store offset out of range (roadmap)"))?;
                let source = self.place_store_value(value, pointee)?;
                if restore {
                    self.reserved.remove(&address);
                }
                self.output
                    .instructions
                    .push(displacement_store(pointee, source, address, offset)?);
            }
            Some(index) => {
                // A SECOND variable-index subscript store defers: mwcc pre-scales the indices
                // of multiple such stores up front (`slwi r4,r4,2; slwi r0,r6,2; stwx…; stwx…`),
                // a look-ahead schedule the per-store just-in-time `slwi r0,i,k` does not model
                // (measured DIFF: `a[i]=x; a[j]=y`). The first is byte-exact; the second emits
                // the wrong interleaved, r0-reusing order — so defer it.
                if self.emitted_variable_index_store {
                    return Err(Diagnostic::error("a second variable-index subscript store needs look-ahead index scheduling (roadmap)"));
                }
                self.emitted_variable_index_store = true;
                // A variable index uses the scratch for scaling, so the value must
                // be a leaf (it stays in its own register) — no temporary, so release
                // the address reservation up front.
                if restore {
                    self.reserved.remove(&address);
                }
                if !matches!(value, Expression::Variable(_)) {
                    return Err(Diagnostic::error(
                        "store with a variable index needs a simple value (roadmap)",
                    ));
                }
                let source = self.place_store_value(value, pointee)?;
                // `a[i + const] = v` / `a[i - const] = v`: scale the variable index, add it to the base,
                // and fold the constant into the store displacement (`slwi r0,i,k; add a,a,r0; stw v,off(a)`).
                if let Expression::Binary {
                    operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract),
                    left,
                    right,
                } = index
                {
                    if constant_value(left).is_none() {
                        if let Some(constant) = constant_value(right) {
                            let signed = if *operator == BinaryOperator::Subtract {
                                -constant
                            } else {
                                constant
                            };
                            let offset =
                                i16::try_from(signed * pointee.size() as i64).map_err(|_| {
                                    Diagnostic::error("store offset out of range (roadmap)")
                                })?;
                            let index_register = self.general_register_of_leaf(left)?;
                            let size = pointee.size();
                            let scaled = if size == 1 {
                                index_register
                            } else {
                                self.output
                                    .instructions
                                    .push(Instruction::ShiftLeftImmediate {
                                        a: GENERAL_SCRATCH,
                                        s: index_register,
                                        shift: size.trailing_zeros() as u8,
                                    });
                                GENERAL_SCRATCH
                            };
                            self.output.instructions.push(Instruction::Add {
                                d: address,
                                a: address,
                                b: scaled,
                            });
                            self.output
                                .instructions
                                .push(displacement_store(pointee, source, address, offset)?);
                            return Ok(());
                        }
                    }
                }
                let index_register = self.general_register_of_leaf(index)?;
                let size = pointee.size();
                let scaled = if size == 1 {
                    index_register
                } else {
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: GENERAL_SCRATCH,
                            s: index_register,
                            shift: size.trailing_zeros() as u8,
                        });
                    GENERAL_SCRATCH
                };
                self.output
                    .instructions
                    .push(indexed_store(pointee, source, address, scaled)?);
            }
        }
        Ok(())
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
            Expression::Variable(_)
            | Expression::IntegerLiteral(_)
            | Expression::FloatLiteral(_)
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
                    && matches!(
                        value.as_ref(),
                        Expression::Variable(_)
                            | Expression::IntegerLiteral(_)
                            | Expression::FloatLiteral(_)
                    ) =>
            {
                self.emit_store(target, value)
            }
            _ => Err(Diagnostic::error(
                "a comma-operator side effect of this form is not supported yet (roadmap)",
            )),
        }
    }

    /// The register of the leaf at the end of a chained assignment's value, walking
    /// through nested `=`. `None` for a computed or non-leaf value (which flows through
    /// the scratch normally). Used to store the same source register to every target.
    pub(crate) fn innermost_assigned_leaf(&self, value: &Expression) -> Option<u8> {
        match value {
            Expression::Assign { value, .. } => self.innermost_assigned_leaf(value),
            Expression::Variable(name) => self.lookup_general(name),
            _ => None,
        }
    }

    pub(crate) fn place_store_value(
        &mut self,
        value: &Expression,
        pointee: Pointee,
    ) -> Compilation<u8> {
        // A comma-operator value: emit the left's side effects, then store the right,
        // which keeps its own register — `gi = (a, b)` is `stw b,gi`, no scratch move.
        if let Expression::Comma { left, right } = value {
            self.emit_comma_side_effect(left)?;
            return self.place_store_value(right, pointee);
        }
        if let Some(source) = self.try_place_converted_narrow_store_constant(value, pointee) {
            return Ok(source);
        }
        // A constant pre-materialized into a fixed register (a distinct-constant
        // store run) reuses that register instead of re-materializing.
        if let Some(constant) = constant_value(value) {
            if let Some(&(_, register)) = self
                .prematerialized_constants
                .iter()
                .find(|(c, _)| *c == constant as i32)
            {
                return Ok(register);
            }
        }
        // A float/double constant pre-loaded into a fixed FPR (a distinct-float-constant store
        // run) reuses that FPR instead of re-pooling and re-loading it. Keyed on the literal's
        // f64 bits (the run is homogeneous float/double, so no float/double key collision).
        if let Expression::FloatLiteral(value) = value {
            let bits = value.to_bits();
            if let Some(&(_, register)) = self
                .prematerialized_float_constants
                .iter()
                .find(|(existing, _)| *existing == bits)
            {
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
            // A `(double)` cast of an already-double value is a no-op; when the target
            // is itself double, see through it so a double leaf/call stores from its own
            // register (mwcc emits no `frsp`/`fmr`). A single (`float*`) target is a real
            // narrowing, so it is left to the cast path.
            let value = if pointee == Pointee::Double {
                self.peel_redundant_double_cast(value)
            } else {
                value
            };
            // A float/double LITERAL loads from the pool at the TARGET's width — `lfd` for a
            // `double` target (8-byte pool entry), `lfs` for `float`. Without this, the general
            // evaluator below defaults to a single `lfs`, storing a 4-byte constant to an 8-byte
            // `double` target (measured DIFF: `gd = 1.0;` emitted `lfs` where mwcc emits `lfd`).
            if let Expression::FloatLiteral(literal) = value {
                self.load_float_literal(FLOAT_SCRATCH, *literal, pointee == Pointee::Double);
                return Ok(FLOAT_SCRATCH);
            }
            if let Expression::Variable(name) = value {
                // A float parameter/local lives in a register; a float global is not in
                // `locations`, so it falls through to the general float evaluator, which
                // loads it (`lfs`) into the scratch — `gf = gg` is `lfs f0,gg; stfs f0,gf`.
                if self.locations.contains_key(name.as_str()) {
                    return self.float_register_of_leaf(value);
                }
                // An INT (non-float) global stored to a float target — `gf = gi` — needs an
                // int->float conversion of the loaded value; evaluate_float would mis-load it
                // as a float (a miscompile). Defer until that conversion is wired (its schedule
                // differs from the leaf/call cases).
                if matches!(self.globals.get(name.as_str()), Some(global_type) if !matches!(global_type, Type::Float | Type::Double))
                {
                    return Err(Diagnostic::error("an integer global stored to a float target needs an int->float conversion (roadmap)"));
                }
            }
            // A float call result lands in the float return register (f1); store from there
            // directly rather than moving it to f0 first (mwcc emits no `fmr f0,f1`).
            // The store-only LR-reload-hoist barrier keeps the reload after the stfs. An
            // INTEGER-returning call stored to a float global needs an int->float conversion
            // of its r3 result (not yet modeled), so defer rather than mis-store r3 as f1.
            // An intrinsic (`__fabs`) is not a real call: fall through to evaluate_float,
            // which lowers it to the `fabs` instruction in the scratch (mwcc: `fabs f0,f1;
            // stfd f0`). Only a REAL call stores its result from the float return register.
            if let Expression::Call { name, arguments } = value {
                if !is_intrinsic_call(name) {
                    if !matches!(
                        self.call_return_types.get(name),
                        Some(Type::Float | Type::Double)
                    ) {
                        // An int-returning (or implicitly-declared -> int) call result stored to
                        // a float target: convert its r3 to the target precision via the magic-
                        // bias sequence (value-store-first for a call result, like the return
                        // case), leaving the result in the scratch f0 for the caller's store.
                        let source = Eabi::general_result().number;
                        self.emit_call(name, arguments, None, false)?;
                        let double = matches!(pointee, Pointee::Double);
                        self.emit_int_to_float_body(
                            source,
                            FLOAT_SCRATCH,
                            double,
                            true,
                            Eabi::float_result().number,
                            crate::casts::IntToFloatSchedule::CallResult,
                        );
                        return Ok(FLOAT_SCRATCH);
                    }
                    let result = Eabi::float_result().number;
                    self.emit_call(name, arguments, Some(result), true)?;
                    return Ok(result);
                }
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
                    matches!(
                        self.pointee_of(pointer),
                        Ok(Pointee::Char
                            | Pointee::UnsignedChar
                            | Pointee::Short
                            | Pointee::UnsignedShort)
                    )
                }
                Expression::Index { base, .. } => {
                    matches!(
                        self.pointee_of(base),
                        Ok(Pointee::Char
                            | Pointee::UnsignedChar
                            | Pointee::Short
                            | Pointee::UnsignedShort)
                    )
                }
                Expression::Member { member_type, .. } => {
                    matches!(
                        member_type,
                        Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort
                    )
                }
                _ => false,
            };
            if value_is_narrow {
                return Err(Diagnostic::error("a narrow value stored to a wider integer target needs a widening coercion (roadmap)"));
            }
        }
        if let Some(source) = self.try_place_implicit_narrow_store_value(value, pointee)? {
            return Ok(source);
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
                self.output.instructions.push(Instruction::AddImmediate {
                    d: GENERAL_SCRATCH,
                    a: high,
                    immediate: 0,
                });
                return Ok(GENERAL_SCRATCH);
            }
            // A data GLOBAL value is loaded into the scratch — `gi = gj` is `lwz r0,gj; stw r0,gi`
            // — since a global is not held in a register like a parameter or local. A NARROW store
            // target truncates, so a signed-narrow global source is read RAW under the truncation
            // context (`char gc,hc; gc = hc;` -> `lbz r0,hc; stb r0,gc`, no redundant `extsb` — mwcc
            // drops it), like the `var op const` narrow-store path below.
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()) {
                let saved = self.narrow_truncation_context;
                if matches!(
                    pointee,
                    Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort
                ) {
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
        if let Expression::Assign {
            target,
            value: inner,
        } = value
        {
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
        if let Expression::Cast {
            target_type,
            operand,
        } = value
        {
            if target_type.width() < 32 && pointee.element().width() <= target_type.width() {
                let legacy_preserves_cast = self.behavior.narrow_store_conversion_style
                    == mwcc_versions::NarrowStoreConversionStyle::PreserveOutsideBinaryAlu
                    && self.signed_of(*target_type)
                    && !self.is_float_value(operand)
                    && !self.is_float_operand(operand)
                    && !matches!(operand.as_ref(), Expression::Call { .. })
                    && !legacy_narrow_store_binary_alu(operand);
                if legacy_preserves_cast {
                    self.emit_cast_to_integer(*target_type, operand, GENERAL_SCRATCH)?;
                    return Ok(GENERAL_SCRATCH);
                }
                // An integer leaf stores straight from its own register (no scratch move).
                if matches!(operand.as_ref(), Expression::Variable(name) if self.lookup_general(name).is_some())
                {
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
        if let Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } = value
        {
            if let Some(phi) = self.try_emit_legacy_store_phi_select(
                condition,
                when_true,
                when_false,
                *origin,
            )? {
                return Ok(phi);
            }
            if leaf_name(when_true).is_some()
                && leaf_name(when_false).is_some()
                && constant_value(when_true).is_none()
                && constant_value(when_false).is_none()
            {
                let false_register = self.general_register_of_leaf(when_false)?;
                self.emit_conditional(
                    condition,
                    when_true,
                    when_false,
                    false_register,
                    false,
                    *origin,
                )?;
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
}
