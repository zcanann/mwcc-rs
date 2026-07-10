//! Conditional-assign and select-diamond families.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A leaf `void` body that is purely constant stores: mwcc materializes a
    /// repeated store value once and reuses the register (`li r0,0; stw; stw; stw`
    /// for struct/array zeroing). A run of *differing* constants instead needs the
    /// instruction scheduler (distinct registers, interleaved) — defer rather than
    /// emit the unscheduled form. Returns `false` (use the normal path) for bodies
    /// outside this shape, e.g. stores of register-resident values, which already
    /// match.
    /// `T y; if (c) y = A; else y = B; return y;` — both arms assign the same local,
    /// which is then returned, so the body is the select `return (c) ? A : B`. mwcc
    /// compiles it identically to `if (c) return A; return B`. A call in the body
    /// (value live across a branch) is the keystone's and defers.
    pub(crate) fn try_conditional_assign(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        // An initializer is DEAD here — both arms reassign the local before it is read (verified
        // below) and the handler builds the select purely from the arm values — so allow it:
        // `int b = INIT; if (c) b = A; else b = B; return b;` is the same select as the no-init form,
        // which mwcc compiles identically. (No-else keeps deferring to the initialized handler.)
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        let returned = match &function.return_expression {
            Some(Expression::Variable(name)) => name,
            _ => return Ok(false),
        };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        // Each arm must be exactly `y = <value>` for the returned local `y`.
        let arm_value = |body: &[Statement]| match body {
            [Statement::Assign { name, value }] if name == &local.name => Some(value.clone()),
            _ => None,
        };
        let (Some(when_true), Some(when_false)) = (arm_value(then_body), arm_value(else_body)) else {
            return Ok(false);
        };
        // guard_select's early-return / in-place layout matches mwcc only when the fall-through
        // (else) arm is itself a leaf. With an initializer present, a LEAF then-arm and a COMPUTED
        // else-arm (`int y=a; if(c) y=b; else y=a+1;`) drive mwcc to a SCRATCH-select
        // (`<test>; <else into r0>; b<!c>; <then into r0>; mr result,r0`) that this path does not
        // reproduce — it would emit the conditional-return form and ship wrong bytes. Defer that
        // exact shape (the no-initializer variant already defers downstream).
        let arm_is_leaf = |expr: &Expression| leaf_name(expr).is_some() || constant_value(expr).is_some();
        if local.initializer.is_some() && arm_is_leaf(&when_true) && !arm_is_leaf(&when_false) {
            return Ok(false);
        }
        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        // `if (c) y = A; else y = B;` is the guard `if (c) y = A` with fall-through B
        // — mwcc normalizes a negated `if (!c)` the same way it does a guard return
        // (keep A as the in-place default, strip the `!`), so route through
        // guard_select rather than a bare `(c) ? A : B` select.
        let select = guard_select(condition, &when_true, &when_false);
        self.evaluate_tail(&select, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// `T y = INIT; if (c) y = NEW; return y;` (an `if` with no else) where INIT and NEW are
    /// constants. mwcc lowers this conditional ASSIGN as an early-return branch — distinct from the
    /// select/branchless idiom it uses for the equivalent guard `if(c) return NEW; return INIT;`:
    /// `<test c>; li result,INIT; b<!c>lr; li result,NEW; blr` (the false path returns the
    /// initializer already in the result; the true path falls through to the new value). Variable
    /// arms use a different move/staging form and are deferred here.
    /// A narrow-guarded arm containing an INNER `&1` record-test one-liner — the
    /// __va_arg `if (type==2) { size=8; if (g_reg & 1) { even=1; } }` shape at
    /// 2-local scale (measured):
    ///   clrlwi r0,t,24; li r3,i1; cmplwi r0,C; li r5,i2; b<!c> JOIN;
    ///   clrlwi. r0,g,31; li r3,n1; beq JOIN; li r5,n2; JOIN: add r3,r3,r5; blr
    /// Home facts: a takes the in-place r3 (the dying outer-condition register); b
    /// AVOIDS the scratch (the inner record test claims r0) and the live inner
    /// operand's register, taking the next volatile (r5). BOTH branches land on the
    /// single join, and the arm's const assign fills the record-test latency slot.
    pub(crate) fn try_narrow_guard_inner_bittest(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || function_makes_call(function) || !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [first, second] = function.locals.as_slice() else { return Ok(false) };
        let (Some(first_init), Some(second_init)) = (
            first.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()),
            second.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()),
        ) else {
            return Ok(false);
        };
        if first.array_length.is_some() || second.array_length.is_some() {
            return Ok(false);
        }
        // Body: ONE narrow guard whose arm is [first = const, if (g & 1) { second = const }].
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name: name1, value: value1 }, Statement::If { condition: inner, then_body: inner_body, else_body: inner_else }] =
            then_body.as_slice()
        else {
            return Ok(false);
        };
        if name1 != &first.name || !inner_else.is_empty() {
            return Ok(false);
        }
        let Some(first_new) = constant_value(value1).and_then(|constant| i16::try_from(constant).ok()) else {
            return Ok(false);
        };
        let [Statement::Assign { name: name2, value: value2 }] = inner_body.as_slice() else { return Ok(false) };
        if name2 != &second.name {
            return Ok(false);
        }
        let Some(second_new) = constant_value(value2).and_then(|constant| i16::try_from(constant).ok()) else {
            return Ok(false);
        };
        // Inner condition: `g & 1` on a full-width general parameter.
        let Expression::Binary { operator: BinaryOperator::BitAnd, left: inner_left, right: inner_right } = inner else {
            return Ok(false);
        };
        if constant_value(inner_right) != Some(1) {
            return Ok(false);
        }
        let Some(inner_register) = leaf_name(inner_left)
            .and_then(|name| self.locations.get(name))
            .filter(|location| location.class == ValueClass::General && location.width == 32)
            .map(|location| location.register)
        else {
            return Ok(false);
        };
        // Outer condition: an unsigned narrow leaf against a u16 constant.
        let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
        let Some(constant) = constant_value(right).and_then(|value| u16::try_from(value).ok()) else {
            return Ok(false);
        };
        let Some((register, width)) = leaf_name(left)
            .and_then(|name| self.locations.get(name))
            .filter(|location| location.class == ValueClass::General && !location.signed && location.width < 32)
            .map(|location| (location.register, location.width))
        else {
            return Ok(false);
        };
        let Some((options, condition_bit)) = false_branch_bo_bi(*operator) else {
            return Ok(false);
        };
        // -- emit (measured) -- b's home is a PLAIN virtual: the measured r5 EMERGES
        // from pinned interference alone (the record test pins r0, the live inner
        // operand pins its register, the first local's li pins r3 — the lowest free
        // is r5), validating policy #4 as derived, not hardcoded.
        let result = Eabi::general_result().number;
        let second_home = self.fresh_virtual_general();
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: register, clear: 32 - width });
        self.load_integer_constant(result, i64::from(first_init));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: constant });
        self.load_integer_constant(second_home, i64::from(second_init));
        let outer_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        // The inner `&1` record test (keep only bit 31, record form), the arm const in
        // its latency slot, then the skip of the one-liner — to the SAME join.
        self.output.instructions.push(Instruction::AndMaskRecord { a: GENERAL_SCRATCH, s: inner_register, begin: 31, end: 31 });
        self.load_integer_constant(result, i64::from(first_new));
        let inner_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 }); // beq
        self.load_integer_constant(second_home, i64::from(second_new));
        let join = self.output.instructions.len();
        for index in [outer_branch, inner_branch] {
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[index] {
                *target = join;
            }
        }
        self.output.instructions.push(Instruction::Add { d: result, a: result, b: second_home });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Two ifs advance mwcc's anonymous-@N counter by 2 each.
        self.output.anonymous_label_bump += 4;
        Ok(true)
    }

    /// A MEMBER-LOAD-init local + a const-init local under one narrow guard that
    /// reassigns only the second — the __va_arg mixed-init interleave slice
    /// (measured): the LOAD issues in the width-op -> compare latency slot, its
    /// destination RECLAIMING the dying condition register r3 (the in-place add's
    /// home); the const init lands in the freed scratch after the compare; a signed-
    /// char pointee's `extsb` is SPLIT from its `lbz` and schedules after that init:
    ///   clrlwi r0,t,24; lbz r3,0(p); cmplwi r0,C; li r0,i2; extsb r3,r3; b<!c> L;
    ///   li r0,n2; L: add r3,r3,r0; blr           (lwz and no extend for an int*)
    pub(crate) fn try_narrow_interleave_load_first(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || function_makes_call(function) || !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [first, second] = function.locals.as_slice() else { return Ok(false) };
        if first.array_length.is_some() || second.array_length.is_some() {
            return Ok(false);
        }
        // First local: `*p` of a general pointer PARAMETER — a signed-char (lbz +
        // split extsb) or word (lwz) pointee. Second: an i16 constant.
        let Some(Expression::Dereference { pointer }) = first.initializer.as_ref() else { return Ok(false) };
        let Some(pointer_name) = leaf_name(pointer) else { return Ok(false) };
        let Some(&crate::generator::Location { class: ValueClass::General, register: base, pointee: Some(pointee), .. }) =
            self.locations.get(pointer_name)
        else {
            return Ok(false);
        };
        let (signed_char, word) = (
            pointee == mwcc_syntax_trees::Pointee::Char,
            matches!(pointee, mwcc_syntax_trees::Pointee::Int | mwcc_syntax_trees::Pointee::UnsignedInt),
        );
        if !signed_char && !word {
            return Ok(false);
        }
        let Some(second_init) = second.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        // One narrow-guarded block reassigning ONLY the second local to a constant.
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = then_body.as_slice() else { return Ok(false) };
        if name != &second.name {
            return Ok(false);
        }
        let Some(second_new) = constant_value(value).and_then(|constant| i16::try_from(constant).ok()) else {
            return Ok(false);
        };
        // The return is `first + second`.
        if !matches!(function.return_expression.as_ref(), Some(Expression::Binary { operator: BinaryOperator::Add, left, right })
            if matches!(left.as_ref(), Expression::Variable(name) if name == &first.name)
                && matches!(right.as_ref(), Expression::Variable(name) if name == &second.name))
        {
            return Ok(false);
        }
        // The condition: an UNSIGNED narrow leaf against a u16 constant, whose
        // register the load's destination reclaims.
        let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
        let Some(constant) = constant_value(right).and_then(|value| u16::try_from(value).ok()) else {
            return Ok(false);
        };
        let Some((register, width)) = leaf_name(left)
            .and_then(|name| self.locations.get(name))
            .filter(|location| location.class == ValueClass::General && !location.signed && location.width < 32)
            .map(|location| (location.register, location.width))
        else {
            return Ok(false);
        };
        let Some((options, condition_bit)) = false_branch_bo_bi(*operator) else {
            return Ok(false);
        };
        // -- emit (measured) --
        let result = Eabi::general_result().number;
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: register, clear: 32 - width });
        self.output.instructions.push(if signed_char {
            Instruction::LoadByteZero { d: result, a: base, offset: 0 }
        } else {
            Instruction::LoadWord { d: result, a: base, offset: 0 }
        });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: constant });
        self.load_integer_constant(GENERAL_SCRATCH, i64::from(second_init));
        if signed_char {
            self.output.instructions.push(Instruction::ExtendSignByte { a: result, s: result });
        }
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.load_integer_constant(GENERAL_SCRATCH, i64::from(second_new));
        let join = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = join;
        }
        self.output.instructions.push(Instruction::Add { d: result, a: result, b: GENERAL_SCRATCH });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }

    /// TWO const-init locals mutated by a CHAIN of narrow-guarded subset blocks
    /// (the __va_arg fall-through form): `int a=8; int b=4; if(t==2){a=7;}
    /// if(t==3){b=9;} return a+b;`. The condition parameter stays LIVE across the
    /// later tests, so the homes shift to the volatiles past it (r4, r5), each
    /// subsequent test RE-NARROWS into the scratch, and the join adds the homes
    /// into the result (measured):
    ///   clrlwi r0,t,w; li r4,i1; cmplwi r0,C1; li r5,i2; b<!c1> L1; <arm1>;
    ///   L1: clrlwi r0,t,w; cmplwi r0,C2; b<!c2> L2; <arm2>; L2: add r3,r4,r5; blr
    /// Each arm reassigns any SUBSET of the locals to i16 constants (declaration
    /// order). Gated to one general parameter (the condition) — a second live
    /// param would shift the homes (allocator liveness).
    pub(crate) fn try_narrow_chained_blocks(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || function_makes_call(function) || !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [first, second] = function.locals.as_slice() else { return Ok(false) };
        let (Some(first_init), Some(second_init)) = (
            first.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()),
            second.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()),
        ) else {
            return Ok(false);
        };
        if first.array_length.is_some() || second.array_length.is_some() {
            return Ok(false);
        }
        // TWO-plus chained no-else blocks, every condition the SAME unsigned narrow
        // leaf against a u16 constant; every arm a run of subset reassigns to consts.
        if function.statements.len() < 2 {
            return Ok(false);
        }
        let inits = [first_init, second_init];
        let mut assigned_before = [false, false];
        let mut blocks: Vec<(&Expression, Vec<(usize, i16)>)> = Vec::new();
        for statement in &function.statements {
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() || then_body.is_empty() {
                return Ok(false);
            }
            let mut arm: Vec<(usize, i16)> = Vec::new();
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { return Ok(false) };
                let index = if name == &first.name { 0 } else if name == &second.name { 1 } else { return Ok(false) };
                // Declaration order within the arm, no duplicates.
                if arm.last().is_some_and(|&(last, _)| last >= index) {
                    return Ok(false);
                }
                let constant = match constant_value(value).and_then(|value| i16::try_from(value).ok()) {
                    Some(constant) => constant,
                    // A SELF-op against the still-known init constant FOLDS (measured:
                    // `int a=8; if(..){ a=a-1; }` -> `li r4,7`, exactly __va_arg's
                    // `maxsize--` -> `li r5,7`). Valid only while NO earlier block
                    // reassigned the local (the value would then be branch-dependent).
                    None => {
                        let folded = (|| {
                            if assigned_before[index] {
                                return None;
                            }
                            let Expression::Binary { operator, left, right } = value else { return None };
                            if !matches!(left, box_expression if matches!(box_expression.as_ref(), Expression::Variable(inner_name) if inner_name == name)) {
                                return None;
                            }
                            let operand = constant_value(right).and_then(|constant| i16::try_from(constant).ok())?;
                            let base = i64::from(inits[index]);
                            let value = match operator {
                                BinaryOperator::Add => base + i64::from(operand),
                                BinaryOperator::Subtract => base - i64::from(operand),
                                _ => return None,
                            };
                            i16::try_from(value).ok()
                        })();
                        match folded {
                            Some(folded) => folded,
                            None => return Ok(false),
                        }
                    }
                };
                arm.push((index, constant));
            }
            for &(index, _) in &arm {
                assigned_before[index] = true;
            }
            blocks.push((condition, arm));
        }
        // The return is `first + second`.
        if !matches!(function.return_expression.as_ref(), Some(Expression::Binary { operator: BinaryOperator::Add, left, right })
            if matches!(left.as_ref(), Expression::Variable(name) if name == &first.name)
                && matches!(right.as_ref(), Expression::Variable(name) if name == &second.name))
        {
            return Ok(false);
        }
        // Conditions: all on the SAME unsigned narrow single general parameter.
        if self.locations.values().filter(|location| location.class == ValueClass::General).count() != 1 {
            return Ok(false);
        }
        let mut condition_register_width: Option<(u8, u16)> = None;
        let mut tests: Vec<(u16, u8, u8)> = Vec::new(); // (constant, options, bit)
        for (condition, _) in &blocks {
            let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
            let Some(constant) = constant_value(right).and_then(|value| u16::try_from(value).ok()) else {
                return Ok(false);
            };
            let Some((register, width)) = leaf_name(left)
                .and_then(|name| self.locations.get(name))
                .filter(|location| location.class == ValueClass::General && !location.signed && location.width < 32)
                .map(|location| (location.register, location.width as u16))
            else {
                return Ok(false);
            };
            if *condition_register_width.get_or_insert((register, width)) != (register, width) {
                return Ok(false);
            }
            let Some((options, condition_bit)) = false_branch_bo_bi(*operator) else {
                return Ok(false);
            };
            tests.push((constant, options, condition_bit));
        }
        let (register, width) = condition_register_width.expect("at least two blocks");
        // -- emit (measured) -- homes r4, r5 (past the live condition parameter),
        // riding preferred VIRTUALS through the general allocation machinery.
        let result = Eabi::general_result().number;
        let homes = [
            self.fresh_virtual_general_preferring(result + 1),
            self.fresh_virtual_general_preferring(result + 2),
        ];
        let inits = [first_init, second_init];
        for (block_index, ((_, arm), &(constant, options, condition_bit))) in blocks.iter().zip(&tests).enumerate() {
            self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: register, clear: 32 - width as u8 });
            if block_index == 0 {
                self.load_integer_constant(homes[0], i64::from(inits[0]));
            }
            self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: constant });
            if block_index == 0 {
                self.load_integer_constant(homes[1], i64::from(inits[1]));
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            for &(local, constant) in arm {
                self.load_integer_constant(homes[local], i64::from(constant));
            }
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
            // Each if's join advances mwcc's anonymous-@N counter by 2.
            self.output.anonymous_label_bump += 2;
        }
        self.output.instructions.push(Instruction::Add { d: result, a: homes[0], b: homes[1] });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// TWO const-init locals both reassigned to constants in ONE narrow-guarded
    /// block, returned as their sum — the 2-local slice of the __va_arg init-
    /// interleave (measured): the width op leads, the FIRST local's init fills the
    /// latency gap in the sum's in-place register r3, the LOGICAL compare consumes
    /// the scratch, the SECOND local's init lands in the freed r0, the arm rewrites
    /// both, and the join adds them:
    ///   clrlwi r0,t,24; li r3,i1; cmplwi r0,C; li r0,i2; b<!c> L; li r3,n1;
    ///   li r0,n2; L: add r3,r3,r0; blr
    /// The homes mirror the direct `a+b` lowering (a in r3, b in r0); three-plus
    /// locals reassociate the sum and shift homes (r4 appears) — deferred.
    pub(crate) fn try_narrow_interleave_two_locals(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || function_makes_call(function) || !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [first, second] = function.locals.as_slice() else { return Ok(false) };
        let (Some(first_init), Some(second_init)) = (
            first.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()),
            second.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok()),
        ) else {
            return Ok(false);
        };
        if first.array_length.is_some() || second.array_length.is_some() {
            return Ok(false);
        }
        // The single narrow-guarded block reassigns BOTH locals to i16 constants, in
        // declaration order.
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name: name1, value: value1 }, Statement::Assign { name: name2, value: value2 }] = then_body.as_slice() else {
            return Ok(false);
        };
        if name1 != &first.name || name2 != &second.name {
            return Ok(false);
        }
        let Some(second_new) = constant_value(value2).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        // The FIRST arm value: an i16 constant, or a `*p` deref-load of a signed-char/
        // word pointer parameter (the __va_arg type==3 `g_reg = list->fpr` reassign).
        // The load emits into the local's home with the second const filling its
        // latency slot and a signed-char's `extsb` SPLIT after it (measured):
        //   bne JOIN; lbz r3,0(p); li r0,n2; extsb r3,r3; JOIN: add r3,r3,r0
        enum ArmFirst {
            Const(i16),
            Load { base: u8, signed_char: bool },
        }
        let arm_first = if let Some(constant) = constant_value(value1).and_then(|value| i16::try_from(value).ok()) {
            ArmFirst::Const(constant)
        } else if let Expression::Dereference { pointer } = value1 {
            let Some(&crate::generator::Location { class: ValueClass::General, register: load_base, pointee: Some(pointee), .. }) =
                leaf_name(pointer).and_then(|name| self.locations.get(name))
            else {
                return Ok(false);
            };
            let signed_char = pointee == mwcc_syntax_trees::Pointee::Char;
            if !signed_char && !matches!(pointee, mwcc_syntax_trees::Pointee::Int | mwcc_syntax_trees::Pointee::UnsignedInt) {
                return Ok(false);
            }
            ArmFirst::Load { base: load_base, signed_char }
        } else {
            return Ok(false);
        };
        // The return is `first + second` in declaration order.
        if !matches!(function.return_expression.as_ref(), Some(Expression::Binary { operator: BinaryOperator::Add, left, right })
            if matches!(left.as_ref(), Expression::Variable(name) if name == &first.name)
                && matches!(right.as_ref(), Expression::Variable(name) if name == &second.name))
        {
            return Ok(false);
        }
        match arm_first {
            ArmFirst::Const(first_new) => {
                self.emit_narrow_interleave(function, &[(first_init, first_new), (second_init, second_new)])
            }
            ArmFirst::Load { base, signed_char } => {
                // The load-reassign arm variant (measured; homes stay [r3, r0]).
                let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
                let Some(compare_constant) = constant_value(right).and_then(|value| u16::try_from(value).ok()) else {
                    return Ok(false);
                };
                let Some((register, width)) = leaf_name(left)
                    .and_then(|name| self.locations.get(name))
                    .filter(|location| location.class == ValueClass::General && !location.signed && location.width < 32)
                    .map(|location| (location.register, location.width))
                else {
                    return Ok(false);
                };
                let Some((options, condition_bit)) = false_branch_bo_bi(*operator) else {
                    return Ok(false);
                };
                let result = Eabi::general_result().number;
                self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: register, clear: 32 - width });
                self.load_integer_constant(result, i64::from(first_init));
                self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: compare_constant });
                self.load_integer_constant(GENERAL_SCRATCH, i64::from(second_init));
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.output.instructions.push(if signed_char {
                    Instruction::LoadByteZero { d: result, a: base, offset: 0 }
                } else {
                    Instruction::LoadWord { d: result, a: base, offset: 0 }
                });
                self.load_integer_constant(GENERAL_SCRATCH, i64::from(second_new));
                if signed_char {
                    self.output.instructions.push(Instruction::ExtendSignByte { a: result, s: result });
                }
                let join = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = join;
                }
                self.output.instructions.push(Instruction::Add { d: result, a: result, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                self.output.anonymous_label_bump += 2;
                Ok(true)
            }
        }
    }

    /// The THREE-local sibling of [`Self::try_narrow_interleave_two_locals`]: the sum
    /// `a+b+c` REASSOCIATES to `a+(b+c)`, shifting the consumer-tree homes — c takes
    /// the in-place r3, b the scratch r0, and a the first FREE volatile (r4 with one
    /// param): `clrlwi r0,t,w; li r4,i1; cmplwi r0,C; li r0,i2; li r3,i3; b<!c> L;
    /// li r4,n1; li r0,n2; li r3,n3; L: add r3,r0,r3; add r3,r4,r3; blr` (measured).
    pub(crate) fn try_narrow_interleave_three_locals(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || function_makes_call(function) || !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [first, second, third] = function.locals.as_slice() else { return Ok(false) };
        let inits: Option<Vec<i16>> = [first, second, third]
            .iter()
            .map(|local| {
                if local.array_length.is_some() {
                    return None;
                }
                local.initializer.as_ref().and_then(constant_value).and_then(|value| i16::try_from(value).ok())
            })
            .collect();
        let Some(inits) = inits else { return Ok(false) };
        let [Statement::If { condition: _, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name: name1, value: value1 }, Statement::Assign { name: name2, value: value2 }, Statement::Assign { name: name3, value: value3 }] =
            then_body.as_slice()
        else {
            return Ok(false);
        };
        if name1 != &first.name || name2 != &second.name || name3 != &third.name {
            return Ok(false);
        }
        let news: Option<Vec<i16>> = [value1, value2, value3]
            .iter()
            .map(|value| constant_value(value).and_then(|constant| i16::try_from(constant).ok()))
            .collect();
        let Some(news) = news else { return Ok(false) };
        // The return is `first + second + third` (left-assoc) in declaration order.
        if !matches!(function.return_expression.as_ref(), Some(Expression::Binary { operator: BinaryOperator::Add, left, right })
            if matches!(right.as_ref(), Expression::Variable(name) if name == &third.name)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left: inner_left, right: inner_right }
                    if matches!(inner_left.as_ref(), Expression::Variable(name) if name == &first.name)
                        && matches!(inner_right.as_ref(), Expression::Variable(name) if name == &second.name)))
        {
            return Ok(false);
        }
        self.emit_narrow_interleave(function, &[(inits[0], news[0]), (inits[1], news[1]), (inits[2], news[2])])
    }

    /// Shared emission for the 2-/3-local narrow init-interleave, `pairs` in
    /// declaration order. Homes are CONSUMER-TREE-driven (the direct sum lowering):
    /// two locals -> [r3, r0]; three (the reassociated `a+(b+c)`) -> [free, r0, r3].
    /// The first local's init fills the width-op -> compare latency gap; the rest
    /// follow the compare; the arm rewrites all; the join adds right-first.
    fn emit_narrow_interleave(&mut self, function: &Function, pairs: &[(i16, i16)]) -> Compilation<bool> {
        let [Statement::If { condition, .. }] = function.statements.as_slice() else {
            return Ok(false);
        };
        // The condition: an UNSIGNED narrow leaf against a u16 constant.
        let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
        let Some(constant) = constant_value(right).and_then(|value| u16::try_from(value).ok()) else {
            return Ok(false);
        };
        let Some((register, width)) = leaf_name(left)
            .and_then(|name| self.locations.get(name))
            .filter(|location| location.class == ValueClass::General && !location.signed && location.width < 32)
            .map(|location| (location.register, location.width))
        else {
            return Ok(false);
        };
        let Some((options, condition_bit)) = false_branch_bo_bi(*operator) else {
            return Ok(false);
        };
        // -- emit (measured order) -- Homes per the consumer tree: two locals ->
        // [r3, r0] (in-place `add r3,r3,r0`); three (reassociated `a+(b+c)`) ->
        // [first free volatile, r0, r3] (`add r3,r0,r3; add r3,rF,r3`).
        let result = Eabi::general_result().number;
        // The three-local first home is r4 — measured with the condition as the ONLY
        // parameter. A second parameter complicates it: mwcc reclaims a DEAD extra
        // param's register (an unused `u` in r4 still yields r4) but must skip a live
        // one — liveness the allocator models, not this handler. Gate to one param.
        if pairs.len() == 3
            && self
                .locations
                .values()
                .filter(|location| location.class == ValueClass::General)
                .count()
                != 1
        {
            return Ok(false);
        }
        // The homes ride preferred VIRTUALS through the general allocation machinery
        // (Phase D policy #1 end-to-end); LinearScan resolves each to its consumer-
        // tree register — including the scratch r0 preference, whose interval starts
        // at the post-compare `li`, after the width-op's physical r0 died.
        let homes: Vec<u8> = match pairs.len() {
            2 => vec![
                self.fresh_virtual_general_preferring(result),
                self.fresh_virtual_general_preferring(GENERAL_SCRATCH),
            ],
            3 => vec![
                self.fresh_virtual_general_preferring(result + 1),
                self.fresh_virtual_general_preferring(GENERAL_SCRATCH),
                self.fresh_virtual_general_preferring(result),
            ],
            _ => return Ok(false),
        };
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: register, clear: 32 - width });
        self.load_integer_constant(homes[0], i64::from(pairs[0].0));
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: constant });
        for (index, &(init, _)) in pairs.iter().enumerate().skip(1) {
            self.load_integer_constant(homes[index], i64::from(init));
        }
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        for (index, &(_, new)) in pairs.iter().enumerate() {
            self.load_integer_constant(homes[index], i64::from(new));
        }
        let join = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = join;
        }
        match pairs.len() {
            2 => self.output.instructions.push(Instruction::Add { d: result, a: homes[0], b: homes[1] }),
            _ => {
                // a+(b+c): b (r0) + c (r3) in place, then a (the free volatile).
                self.output.instructions.push(Instruction::Add { d: result, a: homes[1], b: homes[2] });
                self.output.instructions.push(Instruction::Add { d: result, a: homes[0], b: result });
            }
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The if's join advances mwcc's anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }

    pub(crate) fn try_conditional_assign_initialized(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        let Some(initializer) = &local.initializer else { return Ok(false) };
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = then_body.as_slice() else {
            return Ok(false);
        };
        if name != &local.name {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let result = Eabi::general_result().number;
        let init_const = constant_value(initializer);
        let new_const = constant_value(value);

        // Resolve the variable arms' registers BEFORE emitting the compare (so a deferral leaves no
        // orphaned instructions). Each variable arm must be a leaf already in a register. The MOVE
        // form stages the initializer in a register (the scratch for a constant init, else the init
        // variable's own register); that staged register must differ from the result — mwcc uses a
        // different layout when the init variable already sits in the result — so defer that case.
        let new_register = match new_const {
            Some(_) => None,
            None => match leaf_name(value).and_then(|name| self.lookup_general(name)) {
                register @ Some(_) => register,
                None => return Ok(false),
            },
        };
        let stage = if init_const.is_some() && new_const.is_some() {
            None // both constant -> branch form, no staging register
        } else {
            let stage = match init_const {
                Some(_) => GENERAL_SCRATCH,
                None => match leaf_name(initializer).and_then(|name| self.lookup_general(name)) {
                    Some(register) => register,
                    None => return Ok(false),
                },
            };
            if stage == result {
                return Ok(false);
            }
            Some(stage)
        };

        // Two measured DIFF gates (probed fire 644 — wrong bytes without them):
        // - A NARROW condition operand (`unsigned char t`): mwcc fills the width-op ->
        //   compare latency gap with the initializer (`clrlwi r0,t,24; li r3,init;
        //   cmplwi r0,2; bclr`), an interleave the test-first emission here does not
        //   model — defer.
        let condition_leaf = match condition {
            Expression::Binary { left, .. } => Some(left.as_ref()),
            other @ Expression::Variable(_) => Some(other),
            _ => None,
        };
        if condition_leaf.is_some_and(|leaf| self.is_narrow_leaf(leaf)) {
            // The MODELED slice of the interleave (measured — __va_arg's type-test
            // shape): an UNSIGNED narrow leaf compared against a small constant, with
            // both arms constant. mwcc widens into the scratch, fills the width-op ->
            // compare latency gap with the local's initializer (loaded into the
            // RESULT, since the local is returned bare), then the LOGICAL compare and
            // the conditional return: `clrlwi r0,t,24; li r3,init; cmplwi r0,C;
            // b<!c>lr; li r3,new; blr`.
            let interleave = (|| {
                let (init_value, new_value) = (init_const?, new_const?);
                let Expression::Binary { operator, left, right } = condition else { return None };
                let constant = constant_value(right).and_then(|value| u16::try_from(value).ok())?;
                let name = leaf_name(left)?;
                let location = self.locations.get(name)?;
                if location.class != ValueClass::General || location.signed || location.width >= 32 {
                    return None;
                }
                let (options, condition_bit) = false_branch_bo_bi(*operator)?;
                Some((location.register, location.width, constant, options, condition_bit, init_value, new_value))
            })();
            let Some((register, width, constant, options, condition_bit, init_value, new_value)) = interleave else {
                return Err(Diagnostic::error("a narrow condition over an initialized local needs the init-interleave schedule (roadmap)"));
            };
            self.output.instructions.push(Instruction::ClearLeftImmediate { a: GENERAL_SCRATCH, s: register, clear: 32 - width });
            // The local's home rides a VIRTUAL with a consumer-tree preference for the
            // result register — the first shape routed through the general allocation
            // machinery (Phase D policy #1 end-to-end); LinearScan resolves it to r3.
            let home = self.fresh_virtual_general_preferring(result);
            self.load_integer_constant(home, init_value);
            self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: constant });
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            self.load_integer_constant(home, new_value);
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        // - The MOVE form with a CONSTANT init (staging in the scratch) when the
        //   condition operand OCCUPIES the result register and DIES at the compare
        //   (the arm's new value does not read it): mwcc homes the local directly in
        //   r3 (`li r3,init; bclr; mr r3,new; blr` — no scratch staging, no merge) —
        //   defer. When the arm READS the condition operand (canary 890's `if (a)
        //   b = a;`) it stays live past the compare, and the scratch-staged form IS
        //   mwcc's; a variable init stages in its own register either way.
        if init_const.is_some()
            && stage.is_some()
            && condition_leaf
                .and_then(leaf_name)
                .and_then(|name| self.lookup_general(name))
                == Some(result)
            && new_register != Some(result)
        {
            return Err(Diagnostic::error("a conditional reassign whose condition dies in the result register homes the local there (roadmap)"));
        }

        // emit_condition_test returns the branch-if-FALSE options (a guard's forward-skip sense),
        // which is exactly the early-return / forward-skip-on-!c we want.
        let (options, condition_bit) = self.emit_condition_test(condition)?;

        // Both arms constant: the early-return BRANCH form — return the initializer in place when
        // the condition does not hold, then fall through to the new value.
        let Some(stage) = stage else {
            self.load_integer_constant(result, init_const.unwrap());
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            self.load_integer_constant(result, new_const.unwrap());
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        };

        // A variable arm: the MOVE/staging form.
        if let Some(init_value) = init_const {
            self.load_integer_constant(stage, init_value);
        }
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        match new_register {
            Some(register) => self.output.instructions.push(Instruction::move_register(stage, register)),
            None => self.load_integer_constant(stage, new_const.unwrap()),
        }
        let after = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = after;
        }
        self.output.instructions.push(Instruction::move_register(result, stage));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// `T y = v; if (c) y = NEW; return y;` (no else) where the initializer `v` is a variable
    /// ALREADY resident in the result register — the param-0 min/max/abs/clamp idiom. mwcc keeps
    /// the initializer in the result register (no move), tests the condition, and issues a
    /// conditional RETURN on the inverse (`b<!c>lr`) that returns the initializer in place; the
    /// taken path falls through to `<NEW into result>; blr`. Every observed NEW shape — `neg`,
    /// `mr` (a variable), `li` (a constant), `add` (a computed value) — is exactly what the general
    /// tail evaluator emits into the result register, so route NEW through it rather than
    /// re-deriving a per-shape layout. This fills the `stage == result` case the initialized
    /// handler above defers (init already in the result register). Only emits after the last
    /// deferral check, so a deferred NEW (an Err from the evaluator) fails the whole function
    /// rather than leaving orphaned instructions.
    pub(crate) fn try_conditional_overwrite_inplace(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        // Match the initialized handler's scope: the branch-with-conditional-return form is the
        // int lowering; other widths/types use different staging, so defer them.
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // The initializer must be a plain variable already living in the result register — then
        // materializing it costs no instruction and the condition test reads it in place. A
        // constant / elsewhere-resident / computed initializer is a different layout (left to the
        // initialized handler or beyond).
        let Some(Expression::Variable(init_name)) = &local.initializer else { return Ok(false) };
        let result = Eabi::general_result().number;
        if self.lookup_general(init_name) != Some(result) {
            return Ok(false);
        }
        // The whole body is `if (c) y = NEW;` (no else) returning y.
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = then_body.as_slice() else {
            return Ok(false);
        };
        if name != &local.name {
            return Ok(false);
        }
        // <test c> — emit_condition_test returns the branch-if-FALSE options (a guard's
        // forward-skip / early-return-on-!c sense), which is exactly what we want here.
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        // b<!c>lr — return the initializer, already in the result register, when c is false.
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        // The taken path computes NEW into the result register, then returns.
        self.evaluate_tail(value, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A PARAMETER conditionally reassigned (optionally after one global store), then
    /// returned: `if (c) { [g = leaf;] [v = NEW;] } return v;`. mwcc keeps v in its
    /// incoming register through the diamond; the skip branch targets the merge, and the
    /// merge is `mr r3,v` — or NOTHING when v already lives in r3, in which case the skip
    /// branch folds to `b<!c>lr` (the conditional-return fold). Captured shapes, GC/2.6:
    ///   `if (a<b) a=b; return a;`        -> cmpw; bgelr; mr r3,r4; blr
    ///   `if (a<b) b=b+1; return b;`      -> cmpw; bge M; addi r4,r4,1; M: mr r3,r4; blr
    ///   `if (a>0) { g=a; a=a-1; } ret a` -> cmpwi; blelr; stw r3; addi r3,r3,-1; blr
    ///   `if (a>0) { g=a; } return a;`    -> cmpwi; blelr; stw r3; blr
    /// LONGER then-bodies RESCHEDULE (a second store sinks below the addi — measured), so
    /// only the probed [Store], [Assign], [Store, Assign] forms are taken; more defers.
    pub(crate) fn try_conditional_reassign_return(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some(location) = self.locations.get(returned.as_str()) else { return Ok(false) };
        if location.class != ValueClass::General || location.width != 32 {
            return Ok(false);
        }
        let home = location.register;
        let result = Eabi::general_result().number;
        // No side effect in either arm of an if/ELSE: the SELECT layouts — checked
        // before the reassign plan, whose in-place gates are narrower than select's
        // computed-from-any-register arms.
        if !else_body.is_empty()
            && !then_body.iter().chain(else_body.iter()).any(|statement| matches!(statement, Statement::Store { .. }))
        {
            return self.try_select_diamond(condition, then_body, else_body, returned);
        }
        let Some(then_order) = self.conditional_reassign_plan(then_body, returned) else { return Ok(false) };

        if else_body.is_empty() {
            // SINGLE-SIDED: v keeps its incoming register; the merge is `mr r3,v`, empty
            // (and folded to a conditional return) when v already lives in r3.
            // -- commit (an Err past here defers the whole function; never Ok(false)) --
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let merge = if home == result { None } else { Some(self.fresh_label()) };
            match merge {
                Some(label) => self.emit_branch_conditional_to(options, condition_bit, label),
                None => self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit }),
            }
            self.emit_conditional_reassign_body(&then_order, home)?;
            if let Some(label) = merge {
                self.bind_label(label);
                self.output.instructions.push(Instruction::move_register(result, home));
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }

        let Some(else_order) = self.conditional_reassign_plan(else_body, returned) else { return Ok(false) };
        let then_ends_assign = matches!(then_body.last(), Some(Statement::Assign { .. }));
        let else_ends_assign = matches!(else_body.last(), Some(Statement::Assign { .. }));

        if then_ends_assign && else_ends_assign {
            // ARM-EXIT: both arms rewrite v last, so each arm computes the RETURN VALUE
            // directly into r3 and returns — no merge, no re-test (measured: `addi
            // r3,r4,1; blr` / an else of `b=a` with a in r3 emits NOTHING, its branch
            // folding to `b<c>lr`). Two statements per arm at most: a THREE-statement
            // arm takes the working-register diamond (through r0, an unconditional
            // branch to a shared `mr r3,r0` merge — measured on x6) — deferred.
            if then_body.len() > 2 || else_body.len() > 2 {
                return Ok(false);
            }
            let then_empty = self.reassign_arm_is_empty(&then_order, result);
            let else_empty = self.reassign_arm_is_empty(&else_order, result);
            if then_empty && else_empty {
                return Ok(false);
            }
            // -- commit --
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if else_empty {
                // The else returns v unchanged (already r3): branch-to-LR on !c.
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                self.emit_reassign_arm_into_result(&then_order, home, result)?;
            } else if then_empty {
                // The mirror: return unchanged on c, fall into the else arm.
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                self.emit_reassign_arm_into_result(&else_order, home, result)?;
            } else {
                let else_label = self.fresh_label();
                self.emit_branch_conditional_to(options, condition_bit, else_label);
                self.emit_reassign_arm_into_result(&then_order, home, result)?;
                self.emit_epilogue_and_return();
                self.bind_label(else_label);
                self.emit_reassign_arm_into_result(&else_order, home, result)?;
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }

        // RE-TEST SPLIT: two independent guards — the then-arm, then the same compare
        // RE-EMITTED with the branch sense inverted for the else-arm; the second guard
        // folds to a conditional return when the merge is empty (the single-sided rules).
        // -- commit --
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let skip_then = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, skip_then);
        self.emit_conditional_reassign_body(&then_order, home)?;
        self.bind_label(skip_then);
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let inverted = options ^ 8;
        let merge = if home == result { None } else { Some(self.fresh_label()) };
        match merge {
            Some(label) => self.emit_branch_conditional_to(inverted, condition_bit, label),
            None => self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: inverted, condition_bit }),
        }
        self.emit_conditional_reassign_body(&else_order, home)?;
        if let Some(label) = merge {
            self.bind_label(label);
            self.output.instructions.push(Instruction::move_register(result, home));
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// True when an arm emits no code: no stores, and its reassignment is a copy whose
    /// source already lives in the result register.
    pub(crate) fn reassign_arm_is_empty(&self, order: &[&Statement], result: u8) -> bool {
        order.iter().all(|statement| match statement {
            Statement::Assign { value: Expression::Variable(source), .. } => self.lookup_general(source) == Some(result),
            _ => false,
        })
    }

    /// Emit one arm-exit arm: stores, then the final reassignment computed DIRECTLY into
    /// the result register (`mr r3,w` elided when w is r3; `addi r3,v,±C`; `li r3,C`).
    pub(crate) fn emit_reassign_arm_into_result(&mut self, order: &[&Statement], home: u8, result: u8) -> Compilation<()> {
        for statement in order {
            match statement {
                Statement::Store { target, value } => self.emit_store(target, value)?,
                Statement::Assign { value, .. } => match value {
                    Expression::Variable(source) => {
                        let source = self.lookup_general(source).expect("gated: register-resident");
                        if source != result {
                            self.output.instructions.push(Instruction::move_register(result, source));
                        }
                    }
                    Expression::Binary { operator, right, .. } => {
                        let constant = constant_value(right).expect("gated: i16 constant") as i16;
                        let immediate = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                        self.output.instructions.push(Instruction::AddImmediate { d: result, a: home, immediate });
                    }
                    other => {
                        let constant = constant_value(other).expect("gated: i16 constant") as i16;
                        self.output.instructions.push(Instruction::load_immediate(result, constant));
                    }
                },
                _ => unreachable!("gated"),
            }
        }
        Ok(())
    }

    /// A pure-assign diamond — `if (c) v = X; else v = Y; return v;` with no side
    /// effects — takes mwcc's SELECT layouts (measured, ten boundary probes):
    ///
    /// A CONSTANT arm is SPECULATED into the phi register in the compare latency slot
    /// (both constant: the else), the branch skipping the other (conditional) arm; with
    /// no constant, a COPY else COALESCES — phi becomes the copy's source register and
    /// the else emits nothing; otherwise the else speculates. The phi is r3 itself when
    /// the conditional arm does not read r3 (merge elided, the branch folding to
    /// b<c>lr), else r0; a coalesced phi is wherever the else source lives. The merge,
    /// when present, is `mr r3,phi`.
    pub(crate) fn try_select_diamond(&mut self, condition: &Expression, then_body: &[Statement], else_body: &[Statement], returned: &str) -> Compilation<bool> {
        let Some(then_arm) = self.classify_select_arm(then_body, returned) else { return Ok(false) };
        let Some(else_arm) = self.classify_select_arm(else_body, returned) else { return Ok(false) };
        let result = Eabi::general_result().number;
        let then_const = matches!(then_arm, SelectArm::Constant(_));
        let else_const = matches!(else_arm, SelectArm::Constant(_));

        if !then_const && !else_const {
            if let SelectArm::Copy(phi) = else_arm {
                // COALESCE: the else vanishes; the then-arm computes into phi.
                if matches!(then_arm, SelectArm::Copy(source) if source == phi) {
                    return Ok(false); // a self-move then-arm is unprobed
                }
                // -- commit --
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                if phi == result {
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                    self.emit_select_arm(&then_arm, phi);
                } else {
                    let merge = self.fresh_label();
                    self.emit_branch_conditional_to(options, condition_bit, merge);
                    self.emit_select_arm(&then_arm, phi);
                    self.bind_label(merge);
                    self.output.instructions.push(Instruction::move_register(result, phi));
                }
                self.emit_epilogue_and_return();
                return Ok(true);
            }
        }

        // SPECULATE: the constant arm if exactly one (the else when both or neither).
        let (speculated, conditional, conditional_is_then) = if then_const && !else_const {
            (&then_arm, &else_arm, false)
        } else {
            (&else_arm, &then_arm, true)
        };
        let conditional_reads_result = match conditional {
            SelectArm::Copy(source) | SelectArm::Computed { source, .. } => *source == result,
            SelectArm::Constant(_) => false,
        };
        let phi = if conditional_reads_result { GENERAL_SCRATCH } else { result };
        // -- commit --
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        self.emit_select_arm(speculated, phi);
        let skip = if conditional_is_then { options } else { options ^ 8 };
        if phi == result {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: skip, condition_bit });
            self.emit_select_arm(conditional, phi);
        } else {
            let merge = self.fresh_label();
            self.emit_branch_conditional_to(skip, condition_bit, merge);
            self.emit_select_arm(conditional, phi);
            self.bind_label(merge);
            self.output.instructions.push(Instruction::move_register(result, phi));
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// One select arm as a value: a register copy, a register ± constant, or a constant.
    pub(crate) fn classify_select_arm(&self, body: &[Statement], returned: &str) -> Option<SelectArm> {
        let [Statement::Assign { name, value }] = body else { return None };
        if name.as_str() != returned {
            return None;
        }
        match value {
            Expression::Variable(source) => Some(SelectArm::Copy(self.lookup_general(source)?)),
            Expression::Binary { operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract), left, right } => {
                let Expression::Variable(source) = left.as_ref() else { return None };
                let source = self.lookup_general(source)?;
                let constant = i16::try_from(constant_value(right)?).ok()?;
                let immediate = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                Some(SelectArm::Computed { source, immediate })
            }
            other => Some(SelectArm::Constant(i16::try_from(constant_value(other)?).ok()?)),
        }
    }

    /// Materialize a select arm into the phi register.
    pub(crate) fn emit_select_arm(&mut self, arm: &SelectArm, phi: u8) {
        match arm {
            SelectArm::Constant(constant) => self.output.instructions.push(Instruction::load_immediate(phi, *constant)),
            SelectArm::Copy(source) => self.output.instructions.push(Instruction::move_register(phi, *source)),
            SelectArm::Computed { source, immediate } => {
                self.output.instructions.push(Instruction::AddImmediate { d: phi, a: *source, immediate: *immediate })
            }
        }
    }

    /// Gate and order one arm of the conditional-reassign form: up to THREE statements
    /// — scalar-global stores of register variables and AT MOST ONE in-place
    /// reassignment of `returned` (`mr` from a register variable, `addi` self-adjust,
    /// or `li` constant) — in source order after the STORE-PAIR BREAK (mwcc pulls a
    /// following reassignment between two adjacent stores; blocked when the jumped
    /// store reads the reassigned variable). A store AFTER a var-copy or constant
    /// reassignment value-forwards the source register instead (measured) — `None`.
    pub(crate) fn conditional_reassign_plan<'a>(&self, body: &'a [Statement], returned: &str) -> Option<Vec<&'a Statement>> {
        if body.is_empty() || body.len() > 3 {
            return None;
        }
        let mut assign_count = 0usize;
        let mut stores_blocked = false;
        for statement in body {
            match statement {
                Statement::Store { target, value } => {
                    if stores_blocked {
                        return None;
                    }
                    let Expression::Variable(global) = target else { return None };
                    if !matches!(self.globals.get(global.as_str()), Some(Type::Int | Type::UnsignedInt)) {
                        return None;
                    }
                    if self.global_array_sizes.contains_key(global.as_str()) {
                        return None;
                    }
                    let Expression::Variable(source) = value else { return None };
                    self.lookup_general(source)?;
                }
                Statement::Assign { name, value } => {
                    if name.as_str() != returned {
                        return None;
                    }
                    assign_count += 1;
                    if assign_count > 1 {
                        return None;
                    }
                    match value {
                        Expression::Variable(source) => {
                            self.lookup_general(source)?;
                            stores_blocked = true;
                        }
                        Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right } => {
                            let reads_self = matches!(left.as_ref(), Expression::Variable(source) if source.as_str() == returned);
                            if !reads_self || constant_value(right).and_then(|value| i16::try_from(value).ok()).is_none() {
                                return None;
                            }
                        }
                        other if constant_value(other).and_then(|value| i16::try_from(value).ok()).is_some() => {
                            stores_blocked = true;
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        let mut order: Vec<&Statement> = body.iter().collect();
        for index in 0..order.len().saturating_sub(2) {
            if !matches!((order[index], order[index + 1]), (Statement::Store { .. }, Statement::Store { .. })) {
                continue;
            }
            if matches!(order[index + 2], Statement::Assign { .. }) {
                let Statement::Store { value, .. } = order[index + 1] else { unreachable!() };
                let jumped_reads_v = matches!(value, Expression::Variable(source) if source.as_str() == returned);
                if !jumped_reads_v {
                    order.swap(index + 1, index + 2);
                }
            }
        }
        Some(order)
    }

    /// Emit one planned arm: stores through the store path, reassignments in place.
    pub(crate) fn emit_conditional_reassign_body(&mut self, order: &[&Statement], home: u8) -> Compilation<()> {
        for statement in order {
            match statement {
                Statement::Store { target, value } => self.emit_store(target, value)?,
                Statement::Assign { value, .. } => match value {
                    Expression::Variable(source) => {
                        let source = self.lookup_general(source).expect("gated: register-resident");
                        self.output.instructions.push(Instruction::move_register(home, source));
                    }
                    Expression::Binary { operator, right, .. } => {
                        let constant = constant_value(right).expect("gated: i16 constant") as i16;
                        let immediate = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                        self.output.instructions.push(Instruction::AddImmediate { d: home, a: home, immediate });
                    }
                    other => {
                        let constant = constant_value(other).expect("gated: i16 constant") as i16;
                        self.output.instructions.push(Instruction::load_immediate(home, constant));
                    }
                },
                _ => unreachable!("gated"),
            }
        }
        Ok(())
    }

}
