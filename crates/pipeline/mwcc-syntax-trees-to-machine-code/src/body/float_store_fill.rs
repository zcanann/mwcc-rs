//! Float/double and aggregate constant store-run fills.
//!
//! The float sibling of the integer store-run fills in `store_fill.rs`: leaf void bodies that
//! initialize `float`/`double` globals, struct members through a pointer base, and array
//! elements with literal constants. Each pre-loads the values into FPRs (and, for aggregates,
//! materializes the base) on mwcc's exact schedule, then stores — or declines so a more general
//! path can try.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A run of 2+ stores of FLOAT literals to small-data `float` globals (`gf=1.0f; gg=2.0f;`).
    /// mwcc pre-loads each constant into a DISTINCT FPR — f(count-1) down to f0 — then stores
    /// them all: `lfs f1,@a; lfs f0,@b; stfs f1,gf; stfs f0,gg`. A run of the SAME constant loads
    /// once into f0 and reuses it. The integer constant-store-fill excludes float globals
    /// (is_scratch_safe_store_target), so this is the float sibling. A MIXED run (some repeated,
    /// some distinct) or an absolute-addressing target defers.
    pub(crate) fn try_float_constant_store_fill(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !matches!(self.behavior.global_addressing, GlobalAddressing::SmallData)
        {
            return Ok(false);
        }
        let statements = function.statements.as_slice();
        if statements.len() < 2 {
            return Ok(false);
        }
        // Every statement stores a FloatLiteral to a `float` (or, homogeneously, `double`)
        // small-data global. A mixed float/double run, or any other target, defers.
        let run_is_double = matches!(
            statements.first(),
            Some(Statement::Store { target: Expression::Variable(name), .. })
                if matches!(self.globals.get(name.as_str()), Some(Type::Double))
        );
        let mut values = Vec::new();
        for statement in statements {
            let Statement::Store {
                target: Expression::Variable(name),
                value: Expression::FloatLiteral(value),
            } = statement
            else {
                return Ok(false);
            };
            let matches_type = match self.globals.get(name.as_str()) {
                Some(Type::Double) => run_is_double,
                Some(Type::Float) => !run_is_double,
                _ => false,
            };
            if !matches_type {
                return Ok(false);
            }
            values.push(*value);
        }
        let count = values.len();
        let keys: Vec<u64> = values.iter().map(|value| value.to_bits()).collect();
        let all_same = keys.iter().all(|value| *value == keys[0]);
        let distinct: std::collections::HashSet<u64> = keys.iter().copied().collect();
        if !all_same && (distinct.len() != count || count > 14) {
            return Ok(false);
        }
        if all_same {
            self.load_float_literal(FLOAT_SCRATCH, values[0], run_is_double);
            self.prematerialized_float_constants = vec![(keys[0], FLOAT_SCRATCH)];
        } else {
            let mut assignments = Vec::with_capacity(count);
            for (index, &value) in values.iter().enumerate() {
                let register = (count - 1 - index) as u8;
                self.load_float_literal(register, value, run_is_double);
                assignments.push((keys[index], register));
            }
            self.prematerialized_float_constants = assignments;
        }
        for statement in statements {
            self.emit_statement(statement)?;
        }
        self.prematerialized_float_constants.clear();
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A leaf void body of float-literal stores to consecutive members through ONE pointer base
    /// (`p->x = 1.0f; p->y = 2.0f; p->z = 3.0f;` — vector/matrix init). Unlike the global sibling,
    /// the base pointer occupies a register, so mwcc does not pre-load N distinct FPRs; instead it
    /// runs a fixed TWO-FPR software pipeline that stays two loads ahead of the stores:
    ///
    /// ```text
    ///   lfs f0,@x ; lfs f1,@y ; stfs f0 ; lfs f0,@z ; stfs f1 ; stfs f0   (reload reuses the freed FPR)
    /// ```
    ///
    /// The register for element `i` is `f((N-1-i) & 1)` (the last store is always `f0`), and each
    /// `load[i+2]` reuses the FPR that `store[i]` just freed. Verified identical at 1.3.2/2.0/2.6/2.7.
    /// Only the two CLEAN value profiles are modeled: an all-same run (one FPR, load once, store N),
    /// and an all-distinct run (the pipeline). A partial-duplicate run value-numbers differently
    /// (`1,2,1` keeps `1.0` live in one FPR and re-orders the assignment) and defers.
    pub(crate) fn try_float_member_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let statements = function.statements.as_slice();
        if statements.len() < 2 {
            return Ok(false);
        }
        // The first statement fixes the base pointer name and the run's width (float vs double).
        let (base_name, run_is_double) = match statements.first() {
            Some(Statement::Store {
                target:
                    Expression::Member {
                        base,
                        member_type,
                        index_stride: None,
                        ..
                    },
                value: Expression::FloatLiteral(_),
            }) => match (base.as_ref(), member_type) {
                (Expression::Variable(name), Type::Double) => (name.clone(), true),
                (Expression::Variable(name), Type::Float) => (name.clone(), false),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        let mut values = Vec::new();
        let mut offsets = Vec::new();
        for statement in statements {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value: Expression::FloatLiteral(value),
            } = statement
            else {
                return Ok(false);
            };
            match base.as_ref() {
                Expression::Variable(name) if *name == base_name => {}
                _ => return Ok(false),
            }
            let matches_type = match member_type {
                Type::Double => run_is_double,
                Type::Float => !run_is_double,
                _ => false,
            };
            if !matches_type {
                return Ok(false);
            }
            values.push(*value);
            let Ok(offset) = u16::try_from(*offset) else {
                return Ok(false);
            };
            offsets.push(offset);
        }
        let Some(base_register) = self.lookup_general(&base_name) else {
            return Ok(false);
        };
        let count = values.len();
        let keys: Vec<u64> = values.iter().map(|value| value.to_bits()).collect();
        let all_same = keys.iter().all(|key| *key == keys[0]);
        let distinct: std::collections::HashSet<u64> = keys.iter().copied().collect();
        let emit_store = |generator: &mut Self, register: u8, offset: u16| {
            let instruction = if run_is_double {
                Instruction::StoreFloatDouble {
                    s: register,
                    a: base_register,
                    offset: offset as i16,
                }
            } else {
                Instruction::StoreFloatSingle {
                    s: register,
                    a: base_register,
                    offset: offset as i16,
                }
            };
            generator.output.instructions.push(instruction);
        };
        if all_same {
            self.load_float_literal(FLOAT_SCRATCH, values[0], run_is_double);
            for &offset in &offsets {
                emit_store(self, FLOAT_SCRATCH, offset);
            }
        } else if distinct.len() == count {
            // Two-FPR software pipeline: register(i) = f((N-1-i) & 1), two loads ahead of the stores.
            let register = |index: usize| ((count - 1 - index) & 1) as u8;
            self.load_float_literal(register(0), values[0], run_is_double);
            self.load_float_literal(register(1), values[1], run_is_double);
            for index in 0..count {
                emit_store(self, register(index), offsets[index]);
                if index + 2 < count {
                    self.load_float_literal(register(index + 2), values[index + 2], run_is_double);
                }
            }
        } else {
            // PARTIAL-DUPLICATE (pooled) run — the identity-matrix shape (`1,0,0,…,1,…`).
            // A reused value stays live past its first store, so mwcc gives every
            // distinct constant its OWN FPR — no recycling, even when liveness would
            // allow it (measured: `1,1,2,3,2` holds 1.0 in f2 to the end): distinct
            // value k (first-seen order) sits in f(D-1-k), the LAST-seen in f0. The
            // first two distinct loads lead; load k (k >= 2) issues right after the
            // FIRST-USE store of distinct value k-2 (the same two-in-flight rhythm as
            // the all-distinct pipeline, keyed on first uses); the stores run in
            // source order. Verified 1.3.2: (1,0,1,0), (1,2,1), (1,1,0), (0,1,1),
            // (1,2,1,3), (1,1,2,3,2), and the 12-store identity fill.
            let pooled = distinct.len();
            // f0..f7 is comfortably inside the volatile FPR file; a wider pool is
            // unmeasured — and NOTHING downstream schedules this shape (the sequential
            // fallback emits wrong bytes, the leak this profile closes), so DEFER.
            if pooled > 8 {
                return Err(Diagnostic::error("a float member-store run pooling more than 8 distinct constants is not supported yet (roadmap)"));
            }
            // Number the distinct values in first-seen order; map each store to its number.
            let mut seen: Vec<(u64, f64)> = Vec::new();
            let mut item_of_store = Vec::with_capacity(count);
            let mut first_use = vec![usize::MAX; pooled];
            for (index, key) in keys.iter().enumerate() {
                let item = match seen.iter().position(|(existing, _)| existing == key) {
                    Some(item) => item,
                    None => {
                        seen.push((*key, values[index]));
                        seen.len() - 1
                    }
                };
                if first_use[item] == usize::MAX {
                    first_use[item] = index;
                }
                item_of_store.push(item);
            }
            let register = |item: usize| (pooled - 1 - item) as u8;
            self.load_float_literal(register(0), seen[0].1, run_is_double);
            if pooled >= 2 {
                self.load_float_literal(register(1), seen[1].1, run_is_double);
            }
            for (index, &item) in item_of_store.iter().enumerate() {
                emit_store(self, register(item), offsets[index]);
                // A first-use store releases the next pipelined distinct load.
                if first_use[item] == index && item + 2 < pooled {
                    self.load_float_literal(register(item + 2), seen[item + 2].1, run_is_double);
                }
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A leaf void body of exactly two member stores through ONE pointer base — one integer
    /// literal and one float/double literal, in either order (`p->i = 0; p->f = 1.0f;` — a
    /// `{int,float}` struct init). mwcc materializes BOTH values first (source order; the integer
    /// into the `r0` scratch, the float into `f0` — separate register files, so no contention),
    /// then emits BOTH stores in source order:
    ///
    /// ```text
    ///   li r0,0 ; lfs f0,@val ; stw r0,0(base) ; stfs f0,4(base)
    /// ```
    ///
    /// Only this two-element, one-of-each shape is modeled: it is version-invariant
    /// (1.3.2/2.0/2.6/2.7) and needs no register scheduling. Three-plus mixed runs interleave the
    /// float FPR pipeline with the stores in scheduler-dependent orders, and a SECOND integer value
    /// contends with the base for a GPR (`li r4; li r0; …`) — both keystone; left to the general path.
    pub(crate) fn try_mixed_member_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        enum Materialize {
            Integer(i64),
            Float(f64, bool),
        }
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let statements = function.statements.as_slice();
        if statements.len() != 2 {
            return Ok(false);
        }
        // Both must be member-literal stores through the same base, each value matching its member
        // class (float/double member <-> FloatLiteral, integer member <-> IntegerLiteral).
        let mut parsed: Vec<(String, u16, Pointee, Materialize)> = Vec::new();
        for statement in statements {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value,
            } = statement
            else {
                return Ok(false);
            };
            let Expression::Variable(name) = base.as_ref() else {
                return Ok(false);
            };
            let Some(pointee) = pointee_of_type(*member_type) else {
                return Ok(false);
            };
            let materialize = match pointee {
                Pointee::Float | Pointee::Double => {
                    let Expression::FloatLiteral(literal) = value else {
                        return Ok(false);
                    };
                    Materialize::Float(*literal, matches!(pointee, Pointee::Double))
                }
                _ => {
                    let Expression::IntegerLiteral(literal) = value else {
                        return Ok(false);
                    };
                    // Only a single-`li` integer (fits a signed 16-bit immediate). A wider value
                    // materializes with `lis;addi`, and mwcc slots the float load into that gap
                    // (`lis r4; lfs f0; addi r0,r4; …`) — a scheduler interleave, left to defer.
                    if i16::try_from(*literal).is_err() {
                        return Ok(false);
                    }
                    Materialize::Integer(*literal)
                }
            };
            let Ok(offset) = u16::try_from(*offset) else {
                return Ok(false);
            };
            parsed.push((name.clone(), offset, pointee, materialize));
        }
        // Same base pointer, and exactly one float-class member paired with one integer-class member.
        if parsed[0].0 != parsed[1].0 {
            return Ok(false);
        }
        let float_members = parsed
            .iter()
            .filter(|(_, _, _, m)| matches!(m, Materialize::Float(..)))
            .count();
        if float_members != 1 {
            return Ok(false);
        }
        let Some(base) = self.lookup_general(&parsed[0].0) else {
            return Ok(false);
        };
        // Materialize both values (source order), then both stores (source order).
        for (_, _, _, materialize) in &parsed {
            match materialize {
                Materialize::Float(literal, double) => {
                    self.load_float_literal(FLOAT_SCRATCH, *literal, *double)
                }
                Materialize::Integer(literal) => {
                    self.load_integer_constant(GENERAL_SCRATCH, *literal)
                }
            }
        }
        for (_, offset, pointee, materialize) in &parsed {
            let register = match materialize {
                Materialize::Float(..) => FLOAT_SCRATCH,
                Materialize::Integer(_) => GENERAL_SCRATCH,
            };
            self.output.instructions.push(displacement_store(
                *pointee,
                register,
                base,
                *offset as i16,
            )?);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Push a float/double store-with-update (`stfsu`/`stfdu`) — writes `s` at `0(base)` then
    /// sets `base` to that effective address (the `@l` relocation rides the displacement field).
    fn push_float_store_update(&mut self, s: u8, base: u8, double: bool) {
        self.output.instructions.push(if double {
            Instruction::StoreFloatDoubleWithUpdate {
                s,
                a: base,
                offset: 0,
            }
        } else {
            Instruction::StoreFloatSingleWithUpdate {
                s,
                a: base,
                offset: 0,
            }
        });
    }

    /// Push a plain float/double displacement store (`stfs`/`stfd`) of `s` at `offset(base)`.
    fn push_float_store_at(&mut self, s: u8, base: u8, offset: i16, double: bool) {
        self.output.instructions.push(if double {
            Instruction::StoreFloatDouble { s, a: base, offset }
        } else {
            Instruction::StoreFloatSingle { s, a: base, offset }
        });
    }

    /// A leaf void body that initializes consecutive elements of ONE file-scope array global from
    /// index 0 with float/double literals (`g[0]=1.0f; g[1]=2.0f; g[2]=3.0f;` — table/vector init).
    /// For a LARGE (ADDR16) array mwcc uses a shared-base schedule: it loads the values into distinct
    /// FPRs and materializes the base with `lis`, then the FIRST store is a `stfsu` (store-with-update)
    /// that both writes element 0 AND sets the base to `&g[0]`, so the remaining `stfs` ride element
    /// offsets off it:
    ///
    /// ```text
    ///   lfs f2,@a ; lis base,g@ha ; lfs f1,@b ; stfsu f2,g@l(base) ; lfs f0,@c ; stfs f1,4 ; stfs f0,8
    /// ```
    ///
    /// The loads are in source order (so the `.sdata2` pool interns 0,1,2,…); element `i` uses FPR
    /// `f(count-1-i)`; TWO loads precede the `stfsu`, the rest follow it, then the tail stores. Verified
    /// version-invariant (1.3.2/2.6/2.7), float and double, for the two clean value profiles — all-same
    /// (one FPR, one load) and all-distinct. A partial-duplicate run value-numbers by liveness (keystone),
    /// a run not starting at index 0 materializes with `addi` instead of `stfsu`, and a SMALL (SDA) array
    /// uses a `li` base — all three defer here.
    pub(crate) fn try_float_array_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !function.parameters.is_empty()
        {
            return Ok(false);
        }
        let statements = function.statements.as_slice();
        // Two-plus elements; capped at 14 so the distinct FPRs stay in the volatile bank (f0..f13).
        if statements.len() < 2 || statements.len() > 14 {
            return Ok(false);
        }
        // The first store fixes the array and requires the run to begin at element 0.
        let array_name = match statements.first() {
            Some(Statement::Store {
                target: Expression::Index { base, index },
                value: Expression::FloatLiteral(_),
            }) if matches!(index.as_ref(), Expression::IntegerLiteral(0)) => match base.as_ref() {
                Expression::Variable(name) => name.clone(),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        let Some(&total_size) = self.global_array_sizes.get(array_name.as_str()) else {
            return Ok(false);
        };
        // A SMALL (SDA) array (≤8 bytes = exactly two `float`s) uses the SDA base
        // setup, handled below; anything larger takes the ADDR16 shared-base path.
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        let double = match self.globals.get(array_name.as_str()) {
            Some(Type::Double) => true,
            Some(Type::Float) => false,
            _ => return Ok(false),
        };
        let element_size: i64 = if double { 8 } else { 4 };
        // Every statement stores a matching-width float/double literal to g[i], i ascending 0,1,2,…
        let mut values = Vec::new();
        for (expected_index, statement) in statements.iter().enumerate() {
            let Statement::Store {
                target: Expression::Index { base, index },
                value: Expression::FloatLiteral(value),
            } = statement
            else {
                return Ok(false);
            };
            match base.as_ref() {
                Expression::Variable(name) if *name == array_name => {}
                _ => return Ok(false),
            }
            if !matches!(index.as_ref(), Expression::IntegerLiteral(i) if *i == expected_index as i64)
            {
                return Ok(false);
            }
            values.push(*value);
        }
        let count = values.len();
        let keys: Vec<u64> = values.iter().map(|value| value.to_bits()).collect();
        let all_same = keys.iter().all(|key| *key == keys[0]);
        let distinct: std::collections::HashSet<u64> = keys.iter().copied().collect();
        // Only the two liveness-degenerate profiles are modeled; a partial-duplicate run defers.
        if !all_same && distinct.len() != count {
            return Ok(false);
        }
        let base = self.lowest_free_general()?;
        // SMALL (SDA) array: two `float`s. The base `li r3,g@sda21` lands SECOND
        // (after the first value load); the first store folds the SDA relocation
        // directly (`stfs f,g@sda21(r0)`), the second rides `4(r3)`. FPRs still
        // descend f(count-1)..f0; all-same reuses f0 and skips the slot fill.
        //   distinct:  lfs f1,@a ; li r3,g@sda ; lfs f0,@b ; stfs f1,g@sda ; stfs f0,4(r3)
        //   all-same:  lfs f0,@a ; li r3,g@sda ; stfs f0,g@sda ; stfs f0,4(r3)
        if small {
            let fold_store = |generator: &mut Self, source: u8| {
                generator.record_relocation(RelocationKind::EmbSda21, &array_name);
                generator.push_float_store_at(source, 0, 0, double);
            };
            if all_same {
                self.load_float_literal(FLOAT_SCRATCH, values[0], double);
                self.record_relocation(RelocationKind::EmbSda21, &array_name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base,
                    a: 0,
                    immediate: 0,
                });
                fold_store(self, FLOAT_SCRATCH);
                for i in 1..count {
                    self.push_float_store_at(
                        FLOAT_SCRATCH,
                        base,
                        (i as i64 * element_size) as i16,
                        double,
                    );
                }
            } else {
                let fpr = |i: usize| (count - 1 - i) as u8;
                self.load_float_literal(fpr(0), values[0], double);
                self.record_relocation(RelocationKind::EmbSda21, &array_name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base,
                    a: 0,
                    immediate: 0,
                });
                for i in 1..count {
                    self.load_float_literal(fpr(i), values[i], double);
                }
                fold_store(self, fpr(0));
                for i in 1..count {
                    self.push_float_store_at(
                        fpr(i),
                        base,
                        (i as i64 * element_size) as i16,
                        double,
                    );
                }
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        if all_same {
            self.load_float_literal(FLOAT_SCRATCH, values[0], double);
            self.emit_address_high(base, &array_name);
            self.record_relocation(RelocationKind::Addr16Lo, &array_name);
            self.push_float_store_update(FLOAT_SCRATCH, base, double);
            for i in 1..count {
                self.push_float_store_at(
                    FLOAT_SCRATCH,
                    base,
                    (i as i64 * element_size) as i16,
                    double,
                );
            }
        } else {
            let fpr = |i: usize| (count - 1 - i) as u8;
            self.load_float_literal(fpr(0), values[0], double);
            self.emit_address_high(base, &array_name);
            self.load_float_literal(fpr(1), values[1], double);
            self.record_relocation(RelocationKind::Addr16Lo, &array_name);
            self.push_float_store_update(fpr(0), base, double);
            for i in 2..count {
                self.load_float_literal(fpr(i), values[i], double);
            }
            for i in 1..count {
                self.push_float_store_at(fpr(i), base, (i as i64 * element_size) as i16, double);
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
