//! Constant/computed store-run fills.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_constant_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        // The store run is either the whole body, or the body of a single trailing `if (c) { … }`
        // with no else — the same batched constant materialization, wrapped in a conditional return
        // (`cmpwi;beqlr; <run>`). Everything below (detection, register plan) works on `statements`;
        // the conditional-return guard is emitted just before materializing, so a non-run body
        // returns Ok(false) without leaving orphaned instructions.
        let (statements, guard): (&[Statement], Option<&Expression>) = match function.statements.as_slice() {
            [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => (then_body.as_slice(), Some(condition)),
            other => (other, None),
        };
        let Some(plan) = self.constant_store_run_plan(statements) else {
            return Ok(false);
        };
        // Commit. Emit the conditional-return guard first (for the trailing-if form), then the run.
        if let Some(condition) = guard {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        }
        self.emit_constant_store_run(statements, plan)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The register plan for a run of two-or-more constant stores to scratch-safe targets, or
    /// `None` when the statements are not such a run (a non-store, a non-constant value, an unsafe
    /// target) or cannot be scheduled here (3+ distinct constants with a non-global or duplicate
    /// value, or no free register). Pure — used both to emit a run and to pre-check an if-else arm.
    pub(crate) fn constant_store_run_plan(&self, statements: &[Statement]) -> Option<ConstStoreRun> {
        if statements.len() < 2 {
            return None;
        }
        let mut constants = Vec::new();
        for statement in statements {
            let Statement::Store { target, value } = statement else { return None };
            if !self.is_scratch_safe_store_target(target) {
                return None;
            }
            constants.push(constant_value(value)? as i32);
        }
        if constants.iter().all(|constant| *constant == constants[0]) {
            return Some(ConstStoreRun::AllSame);
        }
        if constants.len() == 2 {
            // Two distinct constants: the first into a free register, the second into the scratch.
            let base_registers: Vec<u8> = statements.iter()
                .filter_map(|statement| match statement {
                    Statement::Store { target, .. } => self.store_base_register(target),
                    _ => None,
                })
                .collect();
            let first_register = (3u8..=12).find(|r| *r != GENERAL_SCRATCH && !base_registers.contains(r) && !self.reserved.contains(r))?;
            return Some(ConstStoreRun::Distinct(vec![(constants[0], first_register), (constants[1], GENERAL_SCRATCH)]));
        }
        // 3+ distinct constants to small-data globals: r(N+1) descending to r3 and the last into r0.
        // Member/dereference targets reschedule with their base register, and a duplicate constant
        // shares one register — both fall out of this plan.
        let all_globals = statements.iter().all(|statement| {
            matches!(statement, Statement::Store { target: Expression::Variable(_), .. })
        });
        let count = constants.len();
        let mut distinct = constants.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if !all_globals || distinct.len() != count || count + 1 > 12 {
            return None;
        }
        let assignments = constants.iter().enumerate().map(|(index, &constant)| {
            let register = if index + 1 < count { (count + 1 - index) as u8 } else { GENERAL_SCRATCH };
            (constant, register)
        }).collect();
        Some(ConstStoreRun::Distinct(assignments))
    }

    /// Emit a planned constant store run: materialize the values (all up front for `Distinct`, or
    /// once into the reused scratch for `AllSame`), then the stores in source order. No epilogue.
    pub(crate) fn emit_constant_store_run(&mut self, statements: &[Statement], plan: ConstStoreRun) -> Compilation<()> {
        match plan {
            ConstStoreRun::Distinct(assignments) => {
                for &(constant, register) in &assignments {
                    self.load_integer_constant(register, constant as i64);
                }
                self.prematerialized_constants = assignments;
                for statement in statements {
                    self.emit_statement(statement)?;
                }
                self.prematerialized_constants.clear();
            }
            ConstStoreRun::AllSame => {
                self.reuse_scratch_constant = true;
                self.scratch_constant = None;
                for statement in statements {
                    self.emit_statement(statement)?;
                }
                self.reuse_scratch_constant = false;
                self.scratch_constant = None;
            }
        }
        Ok(())
    }

    /// Whether an if-else arm is a run of two-plus REGISTER-VALUED stores (each value a param/local
    /// already in a register) — emitted sequentially, no value to materialize.
    pub(crate) fn store_run_arm_registers(&self, statements: &[Statement]) -> bool {
        statements.len() >= 2 && statements.iter().all(|statement| matches!(statement,
            Statement::Store { value: Expression::Variable(name), .. } if self.locations.contains_key(name.as_str())))
    }

    pub(crate) fn try_constant_store_if_else(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        // Each arm is handleable when it is a register-valued run or a constant run; pre-check both
        // (no emission) so a non-run arm leaves no orphaned branch.
        let then_plan = self.constant_store_run_plan(then_body);
        let else_plan = self.constant_store_run_plan(else_body);
        let then_registers = self.store_run_arm_registers(then_body);
        let else_registers = self.store_run_arm_registers(else_body);
        if !(then_plan.is_some() || then_registers) || !(else_plan.is_some() || else_registers) {
            return Ok(false);
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        match then_plan {
            Some(plan) => self.emit_constant_store_run(then_body, plan)?,
            None => for statement in then_body { self.emit_statement(statement)?; },
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        let else_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = else_label;
        }
        match else_plan {
            Some(plan) => self.emit_constant_store_run(else_body, plan)?,
            None => for statement in else_body { self.emit_statement(statement)?; },
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// Two computed-value stores to distinct SDA globals (`gi = a+1; gj = b*2;`). mwcc
    /// overlaps the two value computations: it evaluates both first — the earlier into a
    /// real GPR, the later into the scratch `r0` — then stores both (`addi r3,r3,1; slwi
    /// r0,r4,1; stw r3; stw r0`), rather than the unscheduled `compute; store; compute;
    /// store`. We reproduce it by evaluating the first value into a fresh virtual (the
    /// allocator gives it the in-place GPR and keeps it off `r0`, since it is live across
    /// the second computation) and the second into `r0`, then both stores. Leaf/constant
    /// values (no computation to overlap) and 3+ stores fall through to their own paths.
    pub(crate) fn try_computed_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // The two-store run is either the whole body, or the body of a single trailing `if (c) { … }`
        // with no else — the same latency-scheduled value overlap, wrapped in a conditional return.
        // Detection is emission-free, so the guard is emitted just before the value evaluations.
        let (statements, guard): (&[Statement], Option<&Expression>) = match function.statements.as_slice() {
            [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => (then_body.as_slice(), Some(condition)),
            other => (other, None),
        };
        if statements.len() != 2 {
            return Ok(false);
        }
        // Both statements must store to a distinct SDA global. Each value is a single-op
        // computation or a constant; a bare register leaf needs no overlap and goes through
        // try_mixed_store_fill / the normal path.
        let mut stores = Vec::new();
        for statement in statements {
            let Statement::Store { target, value } = statement else { return Ok(false) };
            let Expression::Variable(name) = target else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            // Integer globals only — this path evaluates values through the general
            // (integer) evaluator; a float global/value goes through the float path.
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            // A single-instruction op over register operands, or a constant (materialized
            // with `li`, ordered as a low-latency value) — both shapes this path can
            // schedule. A memory read, comparison idiom, modulo, or nested value reorders
            // or needs more, and a bare register leaf goes through try_mixed_store_fill.
            if !self.is_single_op_register_value(value) && constant_value(value).is_none() {
                return Ok(false);
            }
            stores.push((name.clone(), pointee, value.clone()));
        }
        // At least one value must be a genuine computation. Two constants are the constant
        // fill's domain (it dedups a repeated value to one `li`); this overlap path would
        // emit a separate `li` per store.
        if !self.is_single_op_register_value(&stores[0].2) && !self.is_single_op_register_value(&stores[1].2) {
            return Ok(false);
        }
        if stores[0].0 == stores[1].0 {
            // Same target: the first store is dead (mwcc emits only the second) — a
            // dead-store elimination this path does not model, so defer.
            return Ok(false);
        }
        // The first store's value lives in a virtual (the allocator gives it the in-place
        // GPR, off r0 since it is live across the other op), the second in the scratch r0.
        // mwcc issues the heavier op first and stores the quicker value first, so order the
        // two evaluations and the two stores by latency.
        let high = [self.value_latency_is_high(&stores[0].2), self.value_latency_is_high(&stores[1].2)];
        // Evaluate the heavier value first so the allocator can reuse a spent operand
        // register for the lighter one. Weight: high-latency op > single-cycle op >
        // constant — a constant is just an `li`, materialized last once the computation has
        // freed its operand register (`gi=5; gj=a+1` → `addi r0,r3,1; li r3,5`, the `5`
        // reusing a's register).
        let weight = |is_high: bool, is_constant: bool| -> u8 {
            if is_high { 2 } else if is_constant { 0 } else { 1 }
        };
        let weights = [
            weight(high[0], constant_value(&stores[0].2).is_some()),
            weight(high[1], constant_value(&stores[1].2).is_some()),
        ];
        // For the trailing-if form, the conditional return precedes the value overlap
        // (`cmpwi;beqlr; <two values>; <two stores>`).
        if let Some(condition) = guard {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        }
        let first_register = self.fresh_virtual_general();
        if weights[1] > weights[0] {
            // The second value is the heavier op: compute it (into r0) first.
            self.evaluate_general(&stores[1].2, GENERAL_SCRATCH)?;
            self.evaluate_general(&stores[0].2, first_register)?;
        } else {
            self.evaluate_general(&stores[0].2, first_register)?;
            self.evaluate_general(&stores[1].2, GENERAL_SCRATCH)?;
        }
        if high[0] && !high[1] {
            // The first value is the long op: store the quicker second value first.
            self.emit_sda_global_store_from(&stores[1].0, stores[1].1, GENERAL_SCRATCH)?;
            self.emit_sda_global_store_from(&stores[0].0, stores[0].1, first_register)?;
        } else {
            self.emit_sda_global_store_from(&stores[0].0, stores[0].1, first_register)?;
            self.emit_sda_global_store_from(&stores[1].0, stores[1].1, GENERAL_SCRATCH)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Two stores to distinct integer SDA globals where one value is a register-resident
    /// leaf parameter and the other a "filler" — a single-op computation (`gi=a+1; gj=b;`)
    /// or a constant (`gi=a; gj=5;`). mwcc produces the filler into the scratch, then stores
    /// the LEAF first (it is ready immediately) and the filler second — `addi r0,r3,1; stw
    /// r4,gj; stw r0,gi` or `li r0,5; stw r3,gi; stw r0,gj`. (Both-computed and computed+
    /// constant are the latency-ordered fill above; both-leaf is the normal path; both-
    /// constant is the constant fill.)
    pub(crate) fn try_mixed_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // Either the whole body, or a single trailing `if (c) { … }` (no else) — the same
        // leaf/filler pairing, wrapped in a conditional return emitted just before the filler.
        let (statements, guard): (&[Statement], Option<&Expression>) = match function.statements.as_slice() {
            [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => (then_body.as_slice(), Some(condition)),
            other => (other, None),
        };
        if statements.len() != 2 {
            return Ok(false);
        }
        let mut stores = Vec::new();
        for statement in statements {
            let Statement::Store { target: Expression::Variable(name), value } = statement else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            stores.push((name.clone(), pointee, value.clone()));
        }
        if stores[0].0 == stores[1].0 {
            return Ok(false);
        }
        // Exactly one value is a "filler" — a single-op computation or a constant — and the
        // other a register-resident leaf parameter (a global/memory leaf would need a load).
        // The filler is materialized into the scratch and the leaf stays in its register, so
        // both `gi=a+1; gj=b;` and `gi=a; gj=5;` reduce to: produce the filler, store the
        // leaf, store the filler.
        let filler = [
            self.is_single_op_register_value(&stores[0].2) || constant_value(&stores[0].2).is_some(),
            self.is_single_op_register_value(&stores[1].2) || constant_value(&stores[1].2).is_some(),
        ];
        let is_register_leaf = |value: &Expression| {
            matches!(value, Expression::Variable(name) if !self.globals.contains_key(name.as_str()))
        };
        let (filler, leaf) = if filler[0] && is_register_leaf(&stores[1].2) {
            (0usize, 1usize)
        } else if is_register_leaf(&stores[0].2) && filler[1] {
            (1usize, 0usize)
        } else {
            return Ok(false);
        };
        // The filler goes into the scratch; the leaf is already in its register, so store it
        // first, then the filler. For the trailing-if form the conditional return precedes them.
        let leaf_register = self.general_register_of_leaf(&stores[leaf].2)?;
        if let Some(condition) = guard {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        }
        self.evaluate_general(&stores[filler].2, GENERAL_SCRATCH)?;
        self.emit_sda_global_store_from(&stores[leaf].0, stores[leaf].1, leaf_register)?;
        self.emit_sda_global_store_from(&stores[filler].0, stores[filler].1, GENERAL_SCRATCH)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Three or more stores to distinct integer SDA globals where exactly one value is a
    /// constant and the rest are register-resident leaf parameters (`gi=a; gj=b; gk=5;`).
    /// mwcc hoists the constant's `li` into the scratch up front and stores in source order
    /// — except a constant store cannot occupy the `li`'s one-cycle latency slot, so if the
    /// constant is the FIRST store it swaps with the next (leaf) store:
    ///
    ///     gi=a; gj=b; gk=5  ->  li r0,5; stw r3,gi; stw r4,gj; stw r0,gk   (source order)
    ///     gi=5; gj=a; gk=b  ->  li r0,5; stw r3,gj; stw r0,gi; stw r4,gk   (leading const swaps)
    ///
    /// (Two stores are the mixed fill; all-constant is the constant fill; a non-leaf, non-
    /// constant value among the rest needs the general scheduler and defers.)
    pub(crate) fn try_leaf_constant_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.statements.len() < 3
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let mut stores = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target: Expression::Variable(name), value } = statement else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            stores.push((name.clone(), pointee, value.clone()));
        }
        // Distinct targets (a repeated target is a dead store this path does not model).
        for outer in 0..stores.len() {
            for inner in (outer + 1)..stores.len() {
                if stores[outer].0 == stores[inner].0 {
                    return Ok(false);
                }
            }
        }
        // Exactly one constant; every other value a register-resident leaf parameter.
        let mut constant_index = None;
        for (index, store) in stores.iter().enumerate() {
            if constant_value(&store.2).is_some() {
                if constant_index.is_some() {
                    return Ok(false);
                }
                constant_index = Some(index);
            } else if !matches!(&store.2, Expression::Variable(name) if !self.globals.contains_key(name.as_str())) {
                return Ok(false);
            }
        }
        let Some(constant_index) = constant_index else {
            return Ok(false);
        };
        let constant = constant_value(&stores[constant_index].2).unwrap();
        self.load_integer_constant(GENERAL_SCRATCH, constant as i64);
        // Source order, except a leading constant store swaps with the next store so it does
        // not sit in the `li`'s latency slot.
        let mut order: Vec<usize> = (0..stores.len()).collect();
        if constant_index == 0 {
            order.swap(0, 1);
        }
        for &index in &order {
            if index == constant_index {
                self.emit_sda_global_store_from(&stores[index].0, stores[index].1, GENERAL_SCRATCH)?;
            } else {
                let register = self.general_register_of_leaf(&stores[index].2)?;
                self.emit_sda_global_store_from(&stores[index].0, stores[index].1, register)?;
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Whether `value` is a single-instruction arithmetic op over register-resident
    /// operands (parameters and constants) — the shape the computed-store-fill schedules.
    /// Includes the single-cycle ops (add/sub/and/or/xor/shift/neg/not, power-of-two
    /// multiply) and the multi-cycle but single-instruction ops (register/immediate
    /// multiply, divide), whose latency the fill orders around. Excluded (need more than a
    /// register-shuffle): modulo and comparisons (multi-instruction idioms), a nested
    /// value (needs an intermediate), and a memory read (needs load hoisting).
    /// `int t = <single-op value>; *p = t; [*q = t; …] return t;` — a computed local
    /// KEPT in the result register (r3, because it is returned), stored to one or more
    /// pointers from that register, then returned: `addi r3,r3,1; stw r3,0(r4);
    /// [stw r3,0(r5);] blr`. This is the register-kept slice of value-tracking-with-
    /// stores — the general value_tracking pass INLINES a local's value, which would
    /// recompute t separately for the store and the return; mwcc keeps it in one
    /// register, so we compute it once into r3 and store from there. The store targets
    /// are bare pointer derefs of parameters (`*p = t`) or direct globals (`gg = t`) —
    /// whose address never touches r3, so t survives every store to the return. Single
    /// int local, single-op initializer.
    pub(crate) fn try_computed_local_stored_returned(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else { return Ok(false) };
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(initializer) = local.initializer.as_ref() else { return Ok(false) };
        if !self.is_single_op_register_value(initializer) {
            return Ok(false);
        }
        // The return is exactly the kept local.
        if !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &local.name) {
            return Ok(false);
        }
        // Every statement stores the local to memory through an address that never
        // touches r3 (so t survives to the return): a bare deref of a general parameter
        // (`*p = t`), or a direct global (`gg = t`, SDA/ADDR16-addressed off r0/r2/r13).
        if function.statements.is_empty() {
            return Ok(false);
        }
        for statement in &function.statements {
            let Statement::Store { target, value } = statement else { return Ok(false) };
            if !matches!(value, Expression::Variable(name) if name == &local.name) {
                return Ok(false);
            }
            match target {
                Expression::Dereference { pointer } => {
                    let Some(base) = leaf_name(pointer) else { return Ok(false) };
                    if self.locations.get(base).map(|location| location.class) != Some(ValueClass::General) {
                        return Ok(false);
                    }
                }
                Expression::Variable(name) if self.globals.contains_key(name.as_str()) => {}
                _ => return Ok(false),
            }
        }
        // -- emit: the value once into r3, then each store from r3, then return.
        let result = Eabi::general_result().number;
        self.evaluate_general(initializer, result)?;
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: result, signed, width: 32, pointee: None, stride: None },
        );
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn is_single_op_register_value(&self, value: &Expression) -> bool {
        let is_register_leaf = |operand: &Expression| match operand {
            // A NARROW (char/short) register is not a single-op leaf: it needs a
            // re-extension first (extsb/extsh/clrlwi), whose latency reorders the
            // scheduled overlap — those shapes go through the DAG emitter.
            Expression::Variable(name) => {
                !self.globals.contains_key(name.as_str())
                    && self.locations.get(name.as_str()).is_none_or(|location| location.width == 32)
            }
            Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
            _ => false,
        };
        match value {
            Expression::Binary { operator, left, right } => {
                is_register_leaf(left)
                    && is_register_leaf(right)
                    && matches!(
                        operator,
                        BinaryOperator::Add | BinaryOperator::Subtract
                            | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor
                            | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                            | BinaryOperator::Multiply | BinaryOperator::Divide
                    )
            }
            Expression::Unary { operator: UnaryOperator::Negate | UnaryOperator::BitNot, operand } => is_register_leaf(operand),
            _ => false,
        }
    }

    /// Whether a single-op value is multi-cycle (a register or non-power-of-two multiply,
    /// or a divide) rather than single-cycle. mwcc issues the long op early and stores the
    /// quick value first; the computed-store-fill orders the two values by this.
    pub(crate) fn value_latency_is_high(&self, value: &Expression) -> bool {
        let is_power_of_two = |operand: &Expression| {
            matches!(operand, Expression::IntegerLiteral(n) if *n > 0 && (*n & (*n - 1)) == 0)
        };
        match value {
            Expression::Binary { operator: BinaryOperator::Multiply, left, right } => {
                !(is_power_of_two(left) || is_power_of_two(right))
            }
            Expression::Binary { operator: BinaryOperator::Divide, .. } => true,
            _ => false,
        }
    }

    /// Whether a store to `target` writes only memory (and the value register),
    /// never the scratch — so a constant-fill run can keep its value live in the
    /// scratch across it. A leaf-based member/dereference/constant-index store
    /// qualifies; a global (absolute-addressing base) or variable index (scratch
    /// scaling) does not.
    pub(crate) fn is_scratch_safe_store_target(&self, target: &Expression) -> bool {
        match target {
            Expression::Member { base, .. } => matches!(base.as_ref(), Expression::Variable(_)),
            Expression::Dereference { pointer } => matches!(pointer.as_ref(), Expression::Variable(_)),
            Expression::Index { base, index } => {
                matches!(base.as_ref(), Expression::Variable(_)) && constant_value(index).is_some()
            }
            // A small-data (SDA21) integer global store folds the relocation into the
            // store itself (`stw r0, g@sda21`) — no base register, and it never writes the
            // scratch — so a constant fill can keep its value live across it. An absolute-
            // addressing global needs a base register, so it stays excluded.
            Expression::Variable(name) => {
                matches!(self.behavior.global_addressing, GlobalAddressing::SmallData)
                    && self.globals.get(name.as_str()).is_some_and(|global_type| !matches!(global_type, Type::Float | Type::Double))
            }
            _ => false,
        }
    }

    /// The register holding the base pointer of a scratch-safe store target.
    pub(crate) fn store_base_register(&self, target: &Expression) -> Option<u8> {
        let name = match target {
            Expression::Member { base, .. } | Expression::Index { base, .. } => leaf_name(base),
            Expression::Dereference { pointer } => leaf_name(pointer),
            _ => None,
        }?;
        self.lookup_general(name)
    }

}
