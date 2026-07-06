//! Local value tracking (copy propagation).
//!
//! mwcc does not keep mutable locals in registers across statements; it tracks
//! each local's current *value* (an expression) and substitutes it at the point
//! of use, then compiles the resulting expression. So `int y = x; y = y + 1;
//! return y;` compiles exactly like `return x + 1;`, and `int y = a + b; int z =
//! y * 2; return z;` like `return (a + b) * 2;`. We reproduce that by inlining
//! locals into the return expression and handing it to the normal codegen.

use std::collections::HashMap;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_target::Eabi;
use crate::analysis::{constant_value, expression_reads_memory, expression_reads_name, function_makes_call};
use crate::generator::*;

impl Generator {
    /// Compile the body by inlining value-tracked locals, when the body is in the
    /// shape this handles. Returns `false` (compile nothing) when the existing
    /// single-local / leaf paths should handle it instead, so those stay
    /// byte-identical. Returns `true` once it has emitted the whole body.
    pub(crate) fn try_value_tracking(&mut self, function: &Function) -> Compilation<bool> {
        // The clean in-place accumulator (`int t = a+b; t = t+c; return t;`) is one
        // mwcc keeps in the result register and mutates in place — the substitution
        // model below would reassociate it and disagree, so intercept it first.
        if self.try_inplace_accumulator(function)? {
            return Ok(true);
        }
        // Only take over the cases the straight-line path does not: a reassigned
        // local, or more than one local. A single never-reassigned local keeps the
        // existing handling.
        // Only take over the cases the straight-line path does not: a reassigned
        // local, or more than one local. A single never-reassigned local keeps the
        // existing handling (which computes it once in a register).
        let has_assignment = function.statements.iter().any(|statement| matches!(statement, Statement::Assign { .. }));
        // A single never-reassigned local normally stays with the straight-line path —
        // EXCEPT one whose initializer is a conditional (a branchless idiom like abs):
        // the straight-line path computes it into a register and then mis-reads it in a
        // surrounding expression (`int y = x<0?-x:x; return y + 1;`). Inlining it folds
        // the idiom into the use, matching the direct `(x<0?-x:x) + 1` form.
        let single_conditional_local = function.locals.len() == 1
            && matches!(
                function.locals.first().and_then(|local| local.initializer.as_ref()),
                Some(Expression::Conditional { .. })
            );
        // A single local that is a pure alias of another variable (`T* q = p;`) must
        // inline too: the straight-line path materializes the alias in a register and
        // then dereferences it, which for a pointer picks r0 and emits an invalid
        // `lwz rD,off(0)` (r0 in a base position means literal 0). Substituting the
        // alias away (`q->a` -> `p->a`) matches mwcc's plain `lwz rD,off(rP)`.
        // The alias must be the SAME width as its source (a pointer/int alias). A narrow
        // local aliasing a wider variable (`char c = a;`) is a narrowing, not a pure alias —
        // inlining it drops the truncation + sign-extension; let it fall to the single-local
        // path, which defers the not-already-extended narrow returns.
        let single_alias_local = function.locals.len() == 1
            && function.return_type != Type::Void
            && function.locals.first().is_some_and(|local| local.declared_type.width() >= 32)
            && matches!(
                function.locals.first().and_then(|local| local.initializer.as_ref()),
                Some(Expression::Variable(_))
            );
        // A single local initialized to a CONSTANT must inline too: the straight-line
        // path materializes the constant in the scratch and treats it as a leaf (`int
        // k=3; return x+k` -> `li r0,3; add r3,r3,r0`), but mwcc folds the constant into
        // the use (`return x+3` -> `addi r3,r3,3`). Inlining substitutes `k`'s value away,
        // matching the direct-literal form. A narrow const local is excluded (its width
        // coercion is handled by the narrow-local guards below).
        let single_constant_local = function.locals.len() == 1
            && function.locals.first().is_some_and(|local| local.declared_type.width() >= 32)
            && matches!(
                function.locals.first().and_then(|local| local.initializer.as_ref()),
                Some(Expression::IntegerLiteral(_))
            );
        // A single local read by a store (not just an assignment) still belongs here: the
        // statement loop below defers stores honestly. Declining would drop it to the normal
        // path, which cannot read a value-tracked local and emits garbage (it reads an
        // unallocated register) — a silent miscompile for `int x = a+1; gi = x; return x;`.
        let has_store = function.statements.iter().any(|statement| matches!(statement, Statement::Store { .. }));
        // A single local initialized from a MEMORY read (`int t = arr[i]; return t + 1;` — an
        // array element, dereference, or member) inlines into its use, matching the direct
        // `arr[i] + 1` lowering, but ONLY in a store-free leaf body: a store could alias the
        // read and a call could rewrite the memory, so those keep their order (non-leaf is
        // rejected below; a store is excluded here). The duplication guard below rejects a
        // twice-read load.
        let single_memory_local = function.locals.len() == 1
            && !has_store
            && function.locals.first().is_some_and(|local| local.declared_type.width() >= 32)
            && function.locals.first().and_then(|local| local.initializer.as_ref()).is_some_and(|initializer| {
                let register_names: std::collections::HashSet<&str> = function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str())
                    .chain(function.locals.iter().map(|local| local.name.as_str()))
                    .collect();
                expression_reads_memory(initializer, &register_names)
            });
        // A function with no locals but a reassigned PARAMETER (`int f(int a){ a += 5; return a; }`)
        // is value-tracked too: the param's register is mutated in place and the inlined value
        // feeds the return. Only `has_assignment` distinguishes this from a plain no-locals body
        // (a global store is a Store, never an Assign), so it is safe to take over here.
        if (function.locals.is_empty() && !has_assignment)
            || (function.locals.len() == 1
                && !has_assignment
                && !has_store
                && !single_conditional_local
                && !single_alias_local
                && !single_constant_local
                && !single_memory_local)
        {
            return Ok(false);
        }
        // Leaf functions only for now: a non-leaf needs the prologue/frame, which
        // the straight-line path sets up. Defer those (they error honestly there).
        if function_makes_call(function) {
            return Ok(false);
        }

        // A reassigned NARROW (char/short) PARAMETER narrows differently across versions: for
        // `char f(char a){ a += 1; return a; }`, mwcc 2.6 re-narrows on the return (`addi r0,r3,1;
        // extsb r3,r0`) but 1.3.2 mutates in place and returns raw (`addi r3,r3,1`). Inlining the
        // reassignment and letting the return narrow matches 2.6 but diffs 1.3.2, so defer. (A
        // narrow LOCAL is handled below; a narrow param that is only READ, not reassigned, is fine.)
        let narrow_params: std::collections::HashSet<&str> = function
            .parameters
            .iter()
            .filter(|parameter| parameter.parameter_type.width() < 32)
            .map(|parameter| parameter.name.as_str())
            .collect();
        if function
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::Assign { name, .. } if narrow_params.contains(name.as_str())))
        {
            return Err(Diagnostic::error("a reassigned narrow (char/short) parameter narrows differently across versions (roadmap)"));
        }

        // A narrow (char/short) local initialized from a WIDER value is a NARROWING
        // (`char c = a;` truncates an int to a byte). Inlining substitutes the wider value
        // raw, dropping the truncation AND the sign-extension — `char c = a; gi = c;` would
        // store the full int instead of `(int)(char)a`. Defer until the narrowing coercion is
        // modeled. (A same-width initializer, e.g. `char c = *char_ptr;`, is not a narrowing.)
        for local in &function.locals {
            if local.declared_type.width() < 32 {
                if let Some(initializer) = &local.initializer {
                    if let Ok((_, init_width, _)) = self.leaf_info(initializer) {
                        if init_width as u32 > local.declared_type.width() as u32 {
                            return Err(Diagnostic::error("a narrowing narrow local (char/short from a wider value) is not supported yet (roadmap)"));
                        }
                    }
                }
            }
        }

        // A local that REINTERPRETS signedness (`unsigned u = signed_x;`, `int s = unsigned_x;`)
        // carries the LOCAL's declared signedness, which only matters when the local is USED in a
        // SIGN-SENSITIVE op: `u >> 4` is a LOGICAL shift (`srwi`) for an unsigned `u` but ARITHMETIC
        // (`srawi`) for a signed one — likewise signed/unsigned divide and compare. Inlining the
        // initializer raw drops the local's signedness and emits the wrong shift/divide/compare (a
        // miscompile for negative values). A sign-INSENSITIVE use (`x | y`, `+`, `==`) is byte-exact
        // either way, so only defer when a reinterpreting local feeds a sign-sensitive op.
        for local in &function.locals {
            let Some(initializer) = &local.initializer else { continue };
            let Ok(initializer_signed) = self.signedness_of(initializer) else { continue };
            if initializer_signed == self.signed_of(local.declared_type) {
                continue;
            }
            let name: std::collections::HashSet<&str> = std::iter::once(local.name.as_str()).collect();
            let feeds_sign_sensitive_op = function.return_expression.as_ref().is_some_and(|ret| used_in_sign_sensitive_op(ret, &name))
                || function.statements.iter().any(|statement| match statement {
                    Statement::Store { value, .. } | Statement::Assign { value, .. } => used_in_sign_sensitive_op(value, &name),
                    _ => false,
                });
            if feeds_sign_sensitive_op {
                return Err(Diagnostic::error("a local that reinterprets signedness and feeds a sign-sensitive op is not value-tracked (roadmap)"));
            }
        }

        // Constraints — anything outside the pure-local-arithmetic shape defers.
        // Guards are allowed only when ORDER-INDEPENDENT: each guard reads names the
        // statements never assign (and no tracked local), so it sees the same pristine
        // registers whether it runs before or after the (virtual) reassignments — mwcc
        // compiles `b=b+1; if(a) return 1; return b;` and `if(a) return 1; b=b+1;
        // return b;` to identical bytes (`cmpwi; li r3,1; bnelr; addi r3,r4,1`). The
        // guard sequence then emits ahead of the inlined return below. A guard reading
        // an assigned name or a local, a call anywhere, or a void function defers.
        if !function.guards.is_empty() {
            let written: Vec<&str> = function
                .statements
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Assign { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .chain(function.locals.iter().map(|local| local.name.as_str()))
                .collect();
            let reads_written = |expression: &Expression| written.iter().any(|name| expression_reads_name(expression, name));
            // A guard value must be a constant or a plain variable (the fold / temp-fold
            // forms below); a guard reading an assigned name or a local joins through r0
            // (not modeled), and calls or a void function defer as before.
            let supportable = function.return_type != Type::Void
                && !function_makes_call(function)
                && function
                    .guards
                    .iter()
                    .all(|guard| constant_value(&guard.value).is_some() || matches!(&guard.value, Expression::Variable(_)))
                && !function.guards.iter().any(|guard| reads_written(&guard.condition) || reads_written(&guard.value));
            if !supportable {
                return Err(Diagnostic::error("value tracking combined with guards is not supported yet (roadmap)"));
            }
        }
        if function.return_type == Type::Void {
            // A void function whose body is only local reassignments has no observable
            // effect — every local is dead (assigned but never stored, passed, or
            // returned), so mwcc dead-code-eliminates the whole body and emits just the
            // return. A store/call would be observable and is handled (or deferred) below.
            if function.statements.iter().all(|statement| matches!(statement, Statement::Assign { .. })) {
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            return Err(Diagnostic::error("value tracking for a void function is not supported yet (roadmap)"));
        }

        // Build each local's current value, in order: a declaration initializes it,
        // a later assignment replaces it. Both substitute the values known so far.
        // Inlining duplicates a local's value at each use; that only matches mwcc
        // when no non-trivial computation is duplicated (mwcc keeps such a value in
        // one register — common-subexpression elimination we do not model). Defer a
        // use that would duplicate a non-leaf value.
        let mut values: HashMap<String, Expression> = HashMap::new();
        for local in &function.locals {
            // An uninitialized local has no value until it is assigned below.
            if let Some(initializer) = &local.initializer {
                guard_no_duplication(initializer, &values)?;
                let value = substitute(initializer, &values);
                values.insert(local.name.clone(), value);
            }
        }
        for statement in &function.statements {
            match statement {
                Statement::Assign { name, value } => {
                    guard_no_duplication(value, &values)?;
                    let value = substitute(value, &values);
                    values.insert(name.clone(), value);
                }
                _ => return Err(Diagnostic::error("value tracking with stores or calls is not supported yet (roadmap)")),
            }
        }

        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
        guard_no_duplication(return_expression, &values)?;
        let inlined = substitute(return_expression, &values);
        // Inlining a computed local into an additive chain (`t = a + b; … t + c` ->
        // `(a+b)+c`) makes us lower it like a *direct* multi-term sum, which mwcc
        // reassociates (`mr r0,r3; add r3,r4,r5; add r3,r0,r3`). But mwcc keeps the
        // assigned local in a register and mutates it in place (`add r3,r3,r4; add
        // r3,r3,r5`), so the two disagree. Defer rather than emit the reassociated
        // form; the register allocator will materialize the local and make it exact.
        // The in-place-mutation disagreement only arises when a COMPUTED local (kept in a
        // register by mwcc) is folded into the chain. When every tracked value is a
        // constant, the inlined chain (`int k=3; int m=4; return x+k+m` -> `x+3+4`) is
        // exactly the direct multi-term-with-constants form our codegen already folds
        // (`addi r3,r3,7`), and mwcc folds it identically — so it is safe to proceed.
        // (A single constant STEP like `t = t + 5` also reassociates in mwcc, but our
        // direct codegen matches only for `+`, not `-` — `(a+b)-5` reassociates in mwcc
        // yet lowers in place here — so the whole additive chain stays deferred.)
        let all_values_constant = values.values().all(|value| matches!(value, Expression::IntegerLiteral(_)));
        if has_additive_chain(&inlined) && !all_values_constant {
            return Err(Diagnostic::error("a value-tracked local folded into a multi-term sum needs the register allocator to match mwcc's in-place mutation (roadmap)"));
        }
        let result = match function.return_type {
            Type::Float => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        // Guards emit ahead of the inlined return in one of two verified forms.
        //
        // DIRECT FOLD (`cmpwi; li r3,V; bnelr; <tail into r3>`): constant guard values,
        // a tail that neither reads the result register's parameter (the `li` clobbers
        // it) nor reads two-plus distinct parameters (mwcc schedules such a tail into
        // the local's home register instead).
        //
        // TEMP-FOLD (a single guard over a simple two-leaf tail): the tail computes into
        // its home r0 first — `cmpwi; add r0,r4,r5; li r3,V; bnelr; mr r3,r0` for a
        // constant guard value (branch on the guard TAKEN), or `cmpwi; add r0,r4,r5;
        // mr r3,r0; beqlr; mr r3,v` for a register value (branch on the guard NOT
        // taken). Ordered sources never reach this path — the hoist keeps their
        // order-dependent shapes as If statements for the branch-form handler — so this
        // is exactly the FLAT form mwcc emits for these bodies.
        if function.guards.is_empty() {
            self.evaluate_tail(&inlined, function.return_type, result)?;
            self.emit_epilogue_and_return();
        } else {
            let tail_reads_result_register = self.locations.iter().any(|(name, location)| {
                location.register == result
                    && location.class == ValueClass::General
                    && expression_reads_name(&inlined, name)
            });
            let all_constant = function.guards.iter().all(|guard| constant_value(&guard.value).is_some());
            let distinct_parameter_reads = function
                .parameters
                .iter()
                .filter(|parameter| expression_reads_name(&inlined, &parameter.name))
                .count();
            let simple_binary_tail = matches!(&inlined, Expression::Binary { left, right, .. }
                if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                    && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
            if all_constant && !tail_reads_result_register && distinct_parameter_reads <= 1 {
                self.emit_guard_sequence(&function.guards, &inlined, function.return_type, result)?;
            } else if function.guards.len() == 1
                && !all_constant
                && !tail_reads_result_register
                && distinct_parameter_reads <= 1
                && matches!(function.return_type, Type::Int | Type::UnsignedInt)
            {
                // INVERTED FOLD — a register-valued guard over a one-parameter tail:
                // the tail computes directly into the result register, the conditional
                // return fires when the guard is NOT taken, and the guard value follows
                // (`cmpwi; addi r3,r4,1; beqlr; mr r3,c`). Verified identical in both
                // source orders.
                let guard = &function.guards[0];
                let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
                self.evaluate_tail(&inlined, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                let Expression::Variable(name) = &guard.value else {
                    return Err(Diagnostic::error("a computed guard value is not supported yet (roadmap)"));
                };
                let Some(register) = self.lookup_general(name) else {
                    return Err(Diagnostic::error("a non-general guard value is not supported yet (roadmap)"));
                };
                self.output.instructions.push(Instruction::Or { a: result, s: register, b: register });
                self.emit_epilogue_and_return();
            } else if function.guards.len() == 1
                && matches!(function.return_type, Type::Int | Type::UnsignedInt)
                && simple_binary_tail
            {
                let guard = &function.guards[0];
                let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
                self.evaluate_general(&inlined, GENERAL_SCRATCH)?;
                if let Some(constant) = constant_value(&guard.value) {
                    let immediate = i16::try_from(constant)
                        .map_err(|_| Diagnostic::error("a guard constant outside the li range is not supported yet (roadmap)"))?;
                    self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate });
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                } else {
                    let Expression::Variable(name) = &guard.value else {
                        return Err(Diagnostic::error("a computed guard value is not supported yet (roadmap)"));
                    };
                    let Some(register) = self.lookup_general(name) else {
                        return Err(Diagnostic::error("a non-general guard value is not supported yet (roadmap)"));
                    };
                    self.output.instructions.push(Instruction::Or { a: result, s: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                    self.output.instructions.push(Instruction::Or { a: result, s: register, b: register });
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
                self.output.instructions.push(Instruction::Or { a: result, s: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                self.emit_epilogue_and_return();
            } else {
                return Err(Diagnostic::error(
                    "a flat early-return form outside the fold/temp-fold shapes is not supported yet (roadmap)",
                ));
            }
        }
        Ok(true)
    }

    /// The clean in-place accumulator shape — `int t = p0 OP p1; t = t OP p2; …;
    /// return t;` — where every STEP operand is the NEXT parameter in register order
    /// and the accumulator is always the LEFT operand. mwcc keeps `t` in the result
    /// register and mutates it in place (`add r3,r3,r4; add r3,r3,r5` for `+`, `subf
    /// r3,rN,r3` for `-`, `mullw r3,r3,rN` for `*`): each source register dies at its
    /// single use, so r3 stays free for `t` throughout and the left-operand anchor
    /// never moves. The substitution model would instead reassociate the folded chain
    /// (`(a+b)+c` -> `mr r0,r3; add r3,r4,r5; add r3,r0,r3`), which disagrees, so this
    /// handles the shape directly. The INIT may take a constant right operand (`t =
    /// p0 OP c` -> `addi r3,r3,c` / `mulli r3,r3,c`, in the signed-16-bit range).
    ///
    /// Any deviation — an out-of-order operand, the accumulator on the right, a
    /// reused parameter, a constant STEP operand (which reassociates: `t=t+5` ->
    /// `a+(b+5)`), an out-of-range init constant, a non-int type — instead lands `t`
    /// in the scratch or a callee-saved register, so it returns `false` to defer to
    /// the substitution path (which errors honestly) rather than risk wrong bytes.
    /// This is the first byte-exact slice of the in-register local mutation the
    /// general allocator will eventually own (`docs/register-allocator.md`, step 5).
    /// Verified byte-identical on GC/1.3.2 and GC/2.6.
    fn try_inplace_accumulator(&mut self, function: &Function) -> Compilation<bool> {
        // The result is exactly the accumulator local, returned by name, at int type.
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // No guards; every statement below must reassign the accumulator, so a guard,
        // store, call, or any other statement means this is not the pure shape.
        if !function.guards.is_empty() {
            return Ok(false);
        }
        // Exactly one local — the accumulator — an int initialized from `p0 OP p1`.
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if &local.name != returned || !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // Every parameter must be a plain 32-bit integer, so parameter index k is the
        // GPR r3+k — a float/double or narrow parameter would break that liveness.
        if function.parameters.iter().any(|parameter| !matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt)) {
            return Ok(false);
        }
        let parameter_name = |index: usize| function.parameters.get(index).map(|parameter| parameter.name.as_str());
        let step_operator = |operator: &BinaryOperator| match operator {
            BinaryOperator::Add => Some(AccumulatorOp::Add),
            BinaryOperator::Subtract => Some(AccumulatorOp::Subtract),
            BinaryOperator::Multiply => Some(AccumulatorOp::Multiply),
            _ => None,
        };
        // The initializer defines the accumulator: `p0 OP <p1 | constant>`. The left
        // operand is always the first parameter (its r3 home becomes t's). A constant
        // folds in place ONLY in the init (`addi r3,r3,5` / `mulli r3,r3,5`); a constant
        // in a later STEP instead reassociates the chain (`t=t+5` -> `a+(b+5)`), which
        // the substitution path owns, so the step loop below admits register operands only.
        let result = Eabi::general_result().number;
        let Some(Expression::Binary { operator: init_operator, left: init_left, right: init_right }) = local.initializer.as_ref() else {
            return Ok(false);
        };
        let (init_left, init_right) = (init_left.as_ref(), init_right.as_ref());
        let Expression::Variable(init_left_name) = init_left else {
            return Ok(false);
        };
        if Some(init_left_name.as_str()) != parameter_name(0) {
            return Ok(false);
        }
        let (Some(init_operator), Some(left_register)) = (step_operator(init_operator), self.lookup_general(init_left_name)) else {
            return Ok(false);
        };
        // `first_step_parameter` is the parameter index the step chain starts at: a
        // register init consumes p0,p1 (steps start at p2); a constant init consumes
        // only p0 (steps start at p1).
        let (init_instruction, first_step_parameter) = match init_right {
            Expression::Variable(init_right_name) if Some(init_right_name.as_str()) == parameter_name(1) => {
                let Some(right_register) = self.lookup_general(init_right_name) else {
                    return Ok(false);
                };
                (accumulate(init_operator, result, left_register, right_register), 2)
            }
            _ => {
                let Some(constant) = constant_value(init_right) else {
                    return Ok(false);
                };
                // Out of the signed-16-bit immediate range mwcc materializes the constant
                // (`addis`/`lis`), a form left to defer.
                let Some(instruction) = accumulate_immediate(init_operator, result, left_register, constant) else {
                    return Ok(false);
                };
                (instruction, 1)
            }
        };
        // Each assignment is `t = t OP p_k`, consuming the next parameter in order.
        let mut steps = Vec::new();
        for (index, statement) in function.statements.iter().enumerate() {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            if name != returned {
                return Ok(false);
            }
            let Expression::Binary { operator, left, right } = value else {
                return Ok(false);
            };
            let (Expression::Variable(left_name), Expression::Variable(right_name)) = (left.as_ref(), right.as_ref()) else {
                return Ok(false);
            };
            if left_name != returned || Some(right_name.as_str()) != parameter_name(first_step_parameter + index) {
                return Ok(false);
            }
            let Some(operator) = step_operator(operator) else {
                return Ok(false);
            };
            let Some(register) = self.lookup_general(right_name) else {
                return Ok(false);
            };
            steps.push((operator, register));
        }

        // Emit: the accumulator lives in the result register for the whole chain.
        self.output.instructions.push(init_instruction);
        for (operator, register) in steps {
            self.output.instructions.push(accumulate(operator, result, result, register));
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

/// An in-place accumulator step's operator — the byte-exact subset, each a single
/// register-to-register mutation of the accumulator.
#[derive(Clone, Copy)]
enum AccumulatorOp {
    Add,
    Subtract,
    Multiply,
}

/// Emit `dst = left OP right` for the in-place accumulator, matching mwcc's operand
/// placement: `add`/`mullw` take `(dst, left, right)`; subtraction is `subf dst,
/// right, left` because `subf` computes `b - a` (the subtrahend fills the `a` field).
fn accumulate(operator: AccumulatorOp, dst: u8, left: u8, right: u8) -> Instruction {
    match operator {
        AccumulatorOp::Add => Instruction::Add { d: dst, a: left, b: right },
        AccumulatorOp::Subtract => Instruction::SubtractFrom { d: dst, a: right, b: left },
        AccumulatorOp::Multiply => Instruction::MultiplyLow { d: dst, a: left, b: right },
    }
}

/// Emit `dst = left OP const` for a constant-bearing accumulator init, matching
/// mwcc's immediate forms: `addi dst,left,c` for `+`, `addi dst,left,-c` for `-`
/// (subtraction is `+ (-c)`), and `mulli dst,left,c` for `*`. Returns `None` when the
/// constant does not fit the signed 16-bit immediate field — mwcc materializes it
/// with `addis`/`lis` there, a form left to defer.
fn accumulate_immediate(operator: AccumulatorOp, dst: u8, left: u8, constant: i64) -> Option<Instruction> {
    Some(match operator {
        AccumulatorOp::Add => Instruction::AddImmediate { d: dst, a: left, immediate: i16::try_from(constant).ok()? },
        AccumulatorOp::Subtract => Instruction::AddImmediate { d: dst, a: left, immediate: i16::try_from(constant.checked_neg()?).ok()? },
        AccumulatorOp::Multiply => Instruction::MultiplyImmediate { d: dst, a: left, immediate: i16::try_from(constant).ok()? },
    })
}

/// Error if substituting `values` into `expression` would duplicate a non-leaf
/// computation (a local whose value is not a leaf appearing more than once).
/// Whether `expression` uses any of `names` as an operand of a SIGN-SENSITIVE operation — a right
/// shift, a divide/modulo, or a relational comparison — where the operand's signedness selects the
/// instruction (`srwi`/`srawi`, `divwu`/`divw`, `cmplw`/`cmpw`).
fn used_in_sign_sensitive_op(expression: &Expression, names: &std::collections::HashSet<&str>) -> bool {
    match expression {
        Expression::CallThrough { .. } => true, // conservative: an indirect call blocks folds
        Expression::AggregateLiteral(_) => false,
        Expression::PostStep { .. } => true, // conservative: block folds through a postfix step
        Expression::Binary { operator, left, right } => {
            let sign_sensitive = matches!(
                operator,
                BinaryOperator::ShiftRight | BinaryOperator::Divide | BinaryOperator::Modulo
                    | BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
            );
            (sign_sensitive && (crate::analysis::reads_register(left, names) || crate::analysis::reads_register(right, names)))
                || used_in_sign_sensitive_op(left, names)
                || used_in_sign_sensitive_op(right, names)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } | Expression::AddressOf { operand } => used_in_sign_sensitive_op(operand, names),
        Expression::Dereference { pointer } => used_in_sign_sensitive_op(pointer, names),
        Expression::Conditional { condition, when_true, when_false } => {
            used_in_sign_sensitive_op(condition, names) || used_in_sign_sensitive_op(when_true, names) || used_in_sign_sensitive_op(when_false, names)
        }
        Expression::Index { base, index } => used_in_sign_sensitive_op(base, names) || used_in_sign_sensitive_op(index, names),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => used_in_sign_sensitive_op(base, names),
        Expression::Call { arguments, .. } => arguments.iter().any(|argument| used_in_sign_sensitive_op(argument, names)),
        Expression::Assign { target, value } => used_in_sign_sensitive_op(target, names) || used_in_sign_sensitive_op(value, names),
        Expression::Comma { left, right } => used_in_sign_sensitive_op(left, names) || used_in_sign_sensitive_op(right, names),
        Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => false,
    }
}

fn guard_no_duplication(expression: &Expression, values: &HashMap<String, Expression>) -> Compilation<()> {
    for (name, value) in values {
        if !is_leaf_value(value) && count_references(name, expression) > 1 {
            return Err(Diagnostic::error("value tracking would duplicate a computation (needs CSE, roadmap)"));
        }
    }
    Ok(())
}

/// Whether `expression` contains an additive node (`+`/`-`) one of whose operands
/// is itself additive — a multi-term sum/difference. Our direct-expression codegen
/// reassociates such a chain to match mwcc, but a value-tracked local folded into
/// the chain is one mwcc instead materializes and mutates in place, so the two
/// forms disagree.
fn has_additive_chain(expression: &Expression) -> bool {
    fn additive(expression: &Expression) -> bool {
        matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, .. })
    }
    match expression {
        Expression::CallThrough { .. } => true, // conservative
        Expression::AggregateLiteral(_) => false,
        Expression::PostStep { .. } => true, // conservative
        Expression::Binary { operator, left, right } => {
            (matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) && (additive(left) || additive(right)))
                || has_additive_chain(left)
                || has_additive_chain(right)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } | Expression::AddressOf { operand } => has_additive_chain(operand),
        Expression::Conditional { condition, when_true, when_false } => {
            has_additive_chain(condition) || has_additive_chain(when_true) || has_additive_chain(when_false)
        }
        Expression::Dereference { pointer } => has_additive_chain(pointer),
        Expression::Index { base, index } => has_additive_chain(base) || has_additive_chain(index),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => has_additive_chain(base),
        Expression::Assign { target, value } => has_additive_chain(target) || has_additive_chain(value),
        Expression::Comma { left, right } => has_additive_chain(left) || has_additive_chain(right),
        Expression::Call { arguments, .. } => arguments.iter().any(has_additive_chain),
        Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => false,
    }
}

/// Whether an expression is a leaf (free to duplicate): a variable or literal.
fn is_leaf_value(expression: &Expression) -> bool {
    matches!(expression, Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_))
}

/// Count references to the variable `name` within `expression`.
fn count_references(name: &str, expression: &Expression) -> usize {
    match expression {
        Expression::CallThrough { target, arguments } => {
            count_references(name, target) + arguments.iter().map(|argument| count_references(name, argument)).sum::<usize>()
        }
        Expression::AggregateLiteral(_) => 0,
        Expression::PostStep { target, .. } => 2 * count_references(name, target),
        Expression::Variable(variable) => usize::from(variable == name),
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => 0,
        Expression::Binary { left, right, .. } => count_references(name, left) + count_references(name, right),
        Expression::Unary { operand, .. } => count_references(name, operand),
        Expression::Conditional { condition, when_true, when_false } => {
            count_references(name, condition) + count_references(name, when_true) + count_references(name, when_false)
        }
        Expression::Cast { operand, .. } => count_references(name, operand),
        Expression::Dereference { pointer } => count_references(name, pointer),
        Expression::Index { base, index } => count_references(name, base) + count_references(name, index),
        Expression::Member { base, .. } => count_references(name, base),
        Expression::MemberAddress { base, .. } => count_references(name, base),
        Expression::AddressOf { operand } => count_references(name, operand),
        Expression::Assign { target, value } => count_references(name, target) + count_references(name, value),
        Expression::Comma { left, right } => count_references(name, left) + count_references(name, right),
        Expression::Call { arguments, .. } => arguments.iter().map(|argument| count_references(name, argument)).sum(),
    }
}

/// Replace every value-tracked local in `expression` with its current value,
/// recursively. Names not in `values` (parameters, globals) are left untouched.
pub(crate) fn substitute(expression: &Expression, values: &HashMap<String, Expression>) -> Expression {
    match expression {
        // Never substitute through an indirect call (its target is a live load).
        other @ Expression::CallThrough { .. } => other.clone(),
        other @ Expression::AggregateLiteral(_) => other.clone(),
        // A postfix step mutates its target — never substitute through it.
        Expression::PostStep { .. } => expression.clone(),
        Expression::Variable(name) => values.get(name).cloned().unwrap_or_else(|| expression.clone()),
        Expression::Binary { operator, left, right } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute(left, values)),
            right: Box::new(substitute(right, values)),
        },
        Expression::Unary { operator, operand } => {
            Expression::Unary { operator: *operator, operand: Box::new(substitute(operand, values)) }
        }
        Expression::Conditional { condition, when_true, when_false } => Expression::Conditional {
            condition: Box::new(substitute(condition, values)),
            when_true: Box::new(substitute(when_true, values)),
            when_false: Box::new(substitute(when_false, values)),
        },
        Expression::Cast { target_type, operand } => {
            Expression::Cast { target_type: *target_type, operand: Box::new(substitute(operand, values)) }
        }
        Expression::Dereference { pointer } => Expression::Dereference { pointer: Box::new(substitute(pointer, values)) },
        Expression::AddressOf { operand } => Expression::AddressOf { operand: Box::new(substitute(operand, values)) },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(substitute(base, values)),
            index: Box::new(substitute(index, values)),
        },
        Expression::Member { base, offset, member_type, index_stride } => Expression::Member {
            base: Box::new(substitute(base, values)),
            offset: *offset,
            member_type: *member_type,
            index_stride: *index_stride,
        },
        Expression::MemberAddress { base, offset, element } => Expression::MemberAddress {
            base: Box::new(substitute(base, values)),
            offset: *offset,
            element: *element,
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments.iter().map(|argument| substitute(argument, values)).collect(),
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(substitute(target, values)),
            value: Box::new(substitute(value, values)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(substitute(left, values)),
            right: Box::new(substitute(right, values)),
        },
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => expression.clone(),
    }
}
