//! Callee-saved call/park/combine shapes: results and params parked in r31/r30 across successive calls.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A void function whose body is two or more calls that each pass the SAME argument
    /// list — all the parameters, in order — `f(a,b){ g(a,b); h(a,b); }` (the single-
    /// parameter `f(x){ g(x); h(x); }` is the common case). Each parameter is live across
    /// the calls, so mwcc saves them in callee-saved registers up front (r31 to the last
    /// parameter, descending), interleaving each save with its move; the first call uses
    /// the incoming argument registers directly (no moves), and each later call restores
    /// them. One of the most common real shapes (a state handed to several functions).
    pub(crate) fn try_callee_saved_call_args(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.parameters.is_empty() || matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        // A non-void function returns one of the parameters (or a register-only expression
        // over them) after the calls — the return is the post-call use that keeps them
        // live; a void function needs two or more calls for that liveness. A call in the
        // return is a different shape (call-result), so it defers.
        let returns_value = function.return_type != Type::Void;
        if returns_value {
            match &function.return_expression {
                Some(expression) if !expression_has_call(expression) => {}
                _ => return Ok(false),
            }
        } else if function.statements.len() < 2 {
            return Ok(false);
        }
        // Every statement must be a call whose arguments are exactly the parameters in
        // order, so the first call needs no moves and the live set is all the parameters.
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else { return Ok(false) };
            if arguments.len() != function.parameters.len() {
                return Ok(false);
            }
            for (argument, parameter) in arguments.iter().zip(&function.parameters) {
                if !matches!(argument, Expression::Variable(name) if name == &parameter.name) {
                    return Ok(false);
                }
            }
        }
        // Each parameter's incoming register; all must be general-class.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        let count = incoming.len();
        let frame_size = ((8 + 4 * count as i32 + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first; the interleaved save+move
        // prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().map(|(_, register)| *register).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The first call finds the parameters still in their incoming registers (no
        // moves); afterward they live only in their callee-saved registers.
        self.emit_statement(&function.statements[0])?;
        for (rank, (name, _)) in incoming.iter().rev().enumerate() {
            let register = homes[rank];
            if let Some(location) = self.locations.get_mut(name) {
                location.register = register;
            }
        }
        for statement in &function.statements[1..] {
            self.emit_statement(statement)?;
        }
        // A non-void return reads the parameters from their callee-saved registers; the
        // epilogue scheduler hoists the LR reload ahead of this move, matching mwcc.
        if returns_value {
            let result = Eabi::general_result().number;
            let return_expression = function.return_expression.as_ref().unwrap();
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f(...) + x;` — a single general parameter `x` kept live across a call that sits INSIDE
    /// the return expression, then combined with the call's result. mwcc saves `x` in r31 before the
    /// call (`mr r31,r3`), runs the call (whose argument, when it is `x`, is already in the incoming
    /// register, so no move precedes it), reloads LR, then combines from the callee-saved register
    /// (`add r3,r31,r3` — the saved value first). The call is argument-free or forwards exactly the
    /// parameter; a computed/constant argument schedules its materialization differently and defers.
    /// Handles the low-latency ops `+ | & ^` (commutative — `OP r3,r31,r3` on either source side) and
    /// `-` (its `subf` operands chosen by the call's side); heavier ops (e.g. `*`) and multi-parameter
    /// shapes are follow-ups.
    pub(crate) fn try_callee_saved_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 1 {
            return Ok(false);
        }
        let param = &function.parameters[0];
        let (class, param_register) = match self.locations.get(&param.name) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Low-latency ops mwcc issues as a single register op combining the saved parameter (r31)
        // and the call result (r3). The commutative ops (`+ | & ^`) use `OP r3,r31,r3` on either
        // source side; the non-commutative `-` picks its `subf` operands by which side the call is on.
        // `*` combines to a single `mullw r3,r31,r3`; mwcc issues it BEFORE the LR reload (overlapping
        // the multiply latency with the load), which the LR-reload hoist now models.
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let is_param = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == &param.name);
        let (call_name, call_arguments, call_on_left) = match (left.as_ref(), right.as_ref()) {
            (Expression::Call { name, arguments }, other) if is_param(other) => (name, arguments, true),
            (other, Expression::Call { name, arguments }) if is_param(other) => (name, arguments, false),
            _ => return Ok(false),
        };
        // The call takes no arguments or forwards exactly the parameter (already in its incoming
        // register); anything else materializes an argument on a different schedule.
        if !(call_arguments.is_empty() || (call_arguments.len() == 1 && is_param(&call_arguments[0]))) {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the saved parameter.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the saved parameter's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // Save the live parameter before the call clobbers its incoming register.
        self.output.instructions.push(Instruction::Or { a: saved, s: param_register, b: param_register });
        self.emit_call(call_name, call_arguments, None, false)?;
        // Combine the saved parameter with the call result (r3) — the saved value first.
        let result = Eabi::general_result().number;
        self.output.instructions.push(match operator {
            BinaryOperator::Add => Instruction::Add { d: result, a: saved, b: result },
            BinaryOperator::BitOr => Instruction::Or { a: result, s: saved, b: result },
            BinaryOperator::BitAnd => Instruction::And { a: result, s: saved, b: result },
            BinaryOperator::BitXor => Instruction::Xor { a: result, s: saved, b: result },
            // `subf d,a,b` computes `b - a`. `f()-x` (call left) is result-param -> `subf r3,r31,r3`;
            // `x-f()` (call right) is param-result -> `subf r3,r3,r31`.
            BinaryOperator::Subtract if call_on_left => Instruction::SubtractFrom { d: result, a: saved, b: result },
            BinaryOperator::Subtract => Instruction::SubtractFrom { d: result, a: result, b: saved },
            BinaryOperator::Multiply => Instruction::MultiplyLow { d: result, a: saved, b: result },
            _ => unreachable!("operator restricted above"),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `p(x); q(y);` — two single-argument calls passing two distinct parameters in order (a `void`
    /// body). The second parameter is live across the first call, so mwcc saves it in r31 up front
    /// (`mr r31,r4`), runs the first call (the first parameter is still in its incoming register), then
    /// moves the saved second parameter into place for the second call (`mr r3,r31; bl`). The epilogue
    /// reloads LR (hoisted right after the last call) then restores r31. Exactly two parameters/two
    /// calls for now — longer sequences assign further callee-saved registers and are a follow-up.
    pub(crate) fn try_callee_saved_call_sequence(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void || function.return_expression.is_some() {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call { name: name0, arguments: args0 }), Statement::Expression(Expression::Call { name: name1, arguments: args1 })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if args0.len() != 1 || !is_param(&args0[0], 0) || args1.len() != 1 || !is_param(&args1[0], 1) {
            return Ok(false);
        }
        // Both parameters must be general-class (a float parameter is passed/saved differently).
        let mut param_registers = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => param_registers.push(location.register),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and the saved parameter.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the saved parameter's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // Save the second parameter (live across the first call), and record it there so
        // the second call materializes its argument from the saved home (`mr r3,r31`).
        self.output.instructions.push(Instruction::Or { a: saved, s: param_registers[1], b: param_registers[1] });
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = saved;
        }
        // First call: the first parameter is still in its incoming register (no move).
        self.emit_call(name0, args0, None, false)?;
        // Second call: the second parameter now lives in r31.
        self.emit_call(name1, args1, None, false)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f() - g();` — two argument-free calls whose results are subtracted in the return. mwcc
    /// runs the first call, saves its result in r31 (`mr r31,r3`, live across the second call), runs
    /// the second call (its result in r3), reloads LR, then `subf r3,r3,r31` (= r31 - r3 = f() - g()).
    /// Only `-` for now: a COMMUTATIVE op evaluates its operands right-first in mwcc, reordering the
    /// symbol table, which the left-first `symbol_order` does not reproduce (a `referenced_names`
    /// change — deferred). Argument-bearing calls are a follow-up.
    pub(crate) fn try_callee_saved_two_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // Only `-` for now. A commutative `+`/`|`/… evaluates its operands RIGHT-first in mwcc (so the
        // symbol/relocation order is right-then-left), which our left-first `symbol_order` does not
        // reproduce — that needs a `referenced_names` change and defers. `-` is natural left-then-right.
        let Some(Expression::Binary { operator: BinaryOperator::Subtract, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Both operands are argument-free calls (an argument would interleave its materialization with
        // the saves on a schedule not modeled here).
        let (Expression::Call { name: first_name, arguments: first_arguments }, Expression::Call { name: second_name, arguments: second_arguments }) = (left.as_ref(), right.as_ref()) else {
            return Ok(false);
        };
        if !first_arguments.is_empty() || !second_arguments.is_empty() {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the saved result.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the first result's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // First call; its result is saved across the second call.
        self.emit_call(first_name, first_arguments, None, false)?;
        self.output.instructions.push(Instruction::Or { a: saved, s: 3, b: 3 });
        // Second call; its result lands in r3.
        self.emit_call(second_name, second_arguments, None, false)?;
        // `subf d,a,b` = `b - a`; first result saved, second in r3, so `subf r3,r3,<saved>` =
        // first - second (`f() - g()`).
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: saved });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `g(x); return x OP y;` — TWO parameters both live across a single call (the first is passed to
    /// it, the second is only used in the return), combined by a low-latency op (`+ | & ^`, or `-`
    /// whose `subf` order `evaluate_tail` reproduces). mwcc
    /// preserves BOTH in callee-saved registers — the last parameter in r31, the first in r30 —
    /// saving them interleaved up front (`stw r31; mr r31,y; stw r30; mr r30,x`); the return combines
    /// from the saved registers (`add r3,r30,r31`). The call may pass EITHER parameter: the first stays
    /// in its incoming register (no move); the second is materialized from its saved r31 (`mr r3,r31`).
    pub(crate) fn try_callee_saved_param_pair_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call { name, arguments })] = function.statements.as_slice() else {
            return Ok(false);
        };
        // The call passes exactly one of the two parameters (the first stays in its incoming register;
        // the second is materialized from its callee-saved register — see the save/location logic).
        if arguments.len() != 1 || !matches!(&arguments[0], Expression::Variable(argument) if argument == &function.parameters[0].name || argument == &function.parameters[1].name) {
            return Ok(false);
        }
        // The return is `p OP q` reading both parameters, combined by a low-latency op whose operand
        // order `evaluate_tail` reproduces (source order for the commutative ops; the correct `subf`
        // for `-`). `*` is excluded here: with TWO saved GPRs mwcc interleaves the LR reload between
        // the register restores (`mullw; lwz r31; lwz r0; lwz r30`), a register-death epilogue schedule
        // this path does not model — the single-saved-GPR combine handles multiply.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if !((is_param(left, 0) && is_param(right, 1)) || (is_param(left, 1) && is_param(right, 0))) {
            return Ok(false);
        }
        // Both parameters general-class; keep them in incoming (parameter) order for the save loop.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and r31 + r30.
        let frame_size = 16i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first (id order -> r31, r30); the
        // interleaved save+move prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..incoming.len()).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().map(|(_, register)| *register).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The second parameter is now read from its callee-saved home (its incoming register
        // is dead), so a call passing it materializes `mr r3,r31`. The first parameter stays in its
        // incoming register for the call (no move) and moves to its home only afterward.
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = homes[0];
        }
        self.emit_call(name, arguments, None, false)?;
        if let Some(location) = self.locations.get_mut(&function.parameters[0].name) {
            location.register = homes[1];
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `g(a); h(b); return a OP b;` — TWO calls, each passing one of the two parameters, both live to
    /// a combining return. This fuses `try_callee_saved_call_sequence` (two calls, each passing its
    /// parameter) with `try_callee_saved_param_pair_combine` (two parameters combined in the return):
    /// mwcc saves BOTH up front interleaved (`stw r31; mr r31,b; stw r30; mr r30,a`); the FIRST call
    /// reads its parameter from the still-live incoming register (no move); the SECOND materializes its
    /// parameter from r31 (`mr r3,r31`); the return combines from the saved registers (`add r3,r30,
    /// r31`). `*` is excluded (its latency reschedules the two-GPR epilogue restores, per the
    /// param-pair combine); the calls may target the same or different functions.
    pub(crate) fn try_callee_saved_call_sequence_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        // Two call statements, each passing exactly the correspondingly-indexed parameter.
        let [Statement::Expression(Expression::Call { name: name0, arguments: args0 }), Statement::Expression(Expression::Call { name: name1, arguments: args1 })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if args0.len() != 1 || !is_param(&args0[0], 0) || args1.len() != 1 || !is_param(&args1[0], 1) {
            return Ok(false);
        }
        // The return combines both parameters with one low-latency op (either operand order;
        // evaluate_tail reproduces it). `*` is excluded — see the doc comment.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        if !((is_param(left, 0) && is_param(right, 1)) || (is_param(left, 1) && is_param(right, 0))) {
            return Ok(false);
        }
        // Both parameters general-class; keep incoming (parameter) order for the save loop.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push(location.register),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and r31 + r30, interleaved with the moves.
        let frame_size = 16i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first (id order -> r31, r30).
        let homes: Vec<u8> = (0..2).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().copied().collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The second parameter now lives in its callee-saved home (r31); the second call materializes
        // it (`mr r3,r31`). The first parameter stays in its incoming register for the first call and
        // moves to its home (r30) only afterward (its incoming register dies at the first call).
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = homes[0];
        }
        self.emit_call(name0, args0, None, false)?;
        if let Some(location) = self.locations.get_mut(&function.parameters[0].name) {
            location.register = homes[1];
        }
        self.emit_call(name1, args1, None, false)?;
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `h(g(), p)` — an outer call whose FIRST argument is a nested (argument-free) call and whose
    /// SECOND argument is the single parameter, which must survive the nested call. mwcc saves the
    /// parameter in r31 (`mr r31,p`), runs the nested call (its result lands in r3 = the outer call's
    /// first argument), materializes the saved parameter into the second argument register
    /// (`mr r4,r31`), then calls the outer function; the outer call's result (if any) is left in r3.
    /// This is MSL alloc.c's `free`: `__pool_free(get_malloc_pool(), ptr)`.
    pub(crate) fn try_callee_saved_nested_call_arg(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.parameters.len() != 1 || matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        // The whole body is a single outer call — a void expression statement, or the return value.
        let outer = match (function.statements.as_slice(), function.return_expression.as_ref()) {
            ([Statement::Expression(call)], None) if function.return_type == Type::Void => call,
            ([], Some(call)) => call,
            _ => return Ok(false),
        };
        let Expression::Call { name: outer_name, arguments: outer_arguments } = outer else {
            return Ok(false);
        };
        // Exactly two arguments: a nested argument-free call, then the (sole) parameter.
        let [Expression::Call { arguments: nested_arguments, .. }, Expression::Variable(passed)] = outer_arguments.as_slice() else {
            return Ok(false);
        };
        if !nested_arguments.is_empty() || passed != &function.parameters[0].name {
            return Ok(false);
        }
        // The parameter must be general-class (a float parameter is saved differently).
        let incoming = match self.locations.get(passed) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };
        // Prologue: a 16-byte frame saving the link register and the parameter (live across the call).
        self.non_leaf = true;
        self.frame_size = 16;
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        self.output.instructions.push(Instruction::Or { a: saved, s: incoming, b: incoming });
        if let Some(location) = self.locations.get_mut(passed) {
            location.register = saved;
        }
        // The outer call: the nested argument-free call runs (result -> r3 = first argument), the saved
        // parameter is materialized into the second argument register (`mr r4,r31`), then the call.
        self.emit_call(outer_name, outer_arguments, None, false)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A single local initialized by a call whose argument forwards the (single)
    /// parameter, returned combined with that parameter:
    /// `int x = g(a); return x + a;`. The parameter crosses the call: mwcc saves
    /// it in r31 (interleaved save+move prologue), the call reads the STILL-LIVE
    /// incoming register (no argument move), and the combine reads the result
    /// from r3 and the parameter from r31 (measured: the fire-498 probe).
    pub(crate) fn try_callee_saved_result_param_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 1 || function.locals.len() != 1 {
            return Ok(false);
        }
        let parameter = &function.parameters[0];
        let local = &function.locals[0];
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // The initializer is a call forwarding the parameter in its natural
        // register position (or no arguments) — no argument moves to schedule.
        let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
            return Ok(false);
        };
        match arguments.as_slice() {
            [] => {}
            [Expression::Variable(argument)] if argument == &parameter.name => {}
            _ => return Ok(false),
        }
        // The return combines the local and the parameter with one low-latency op
        // (either operand order; evaluate_tail reproduces it). Multiply is excluded:
        // its latency reschedules the epilogue restores.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let reads = |expression: &Expression, name: &str| matches!(expression, Expression::Variable(variable) if variable == name);
        if !((reads(left, &local.name) && reads(right, &parameter.name))
            || (reads(left, &parameter.name) && reads(right, &local.name)))
        {
            return Ok(false);
        }
        let incoming = match self.locations.get(&parameter.name) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };
        // Prologue: a 16-byte frame saving the link register and r31, the
        // parameter's save+move interleaved (stw r31; mr r31,r3).
        let frame_size = 16i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        let homes: Vec<u8> = vec![self.fresh_virtual_general()];
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        self.output.instructions.extend(plan.prologue_interleaved(&[incoming]));
        // The call reads the parameter from its STILL-LIVE incoming register (no
        // move); only afterwards does the parameter read from its saved home.
        self.emit_call(name, arguments, None, false)?;
        if let Some(location) = self.locations.get_mut(&parameter.name) {
            location.register = homes[0];
        }
        // The local IS the call result: it lives (and dies) in r3.
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: 3, signed, width: 32, pointee: None, stride: None },
        );
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A call-result local added to TWO call-crossing saved values (measured
    /// fire 498/499 probes): `int x = g(a); return x + a + b;` (both parameters
    /// cross) and `int x = g(a); int y = g(x); return y + a + x;` (the first
    /// result crosses the second call). mwcc REASSOCIATES `(res + s1) + s2` into
    /// `res + (s1 + s2)`: the fresh result parks in r0 (`mr r0,r3`), the two
    /// callee-saved homes combine first (`add r3,r30,r31` — creation order), and
    /// the parked value adds last. Adds only — the reassociation is unmeasured
    /// for the other operators.
    pub(crate) fn try_callee_saved_result_park_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // The return is ((result + saved1) + saved2), left-associated adds.
        let Some(Expression::Binary { operator: BinaryOperator::Add, left: outer_left, right: outer_right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Add, left: inner_left, right: inner_right } = outer_left.as_ref() else {
            return Ok(false);
        };
        let (Expression::Variable(result_name), Expression::Variable(saved1), Expression::Variable(saved2)) =
            (inner_left.as_ref(), inner_right.as_ref(), outer_right.as_ref())
        else {
            return Ok(false);
        };
        let all_int = |declared: Type| matches!(declared, Type::Int | Type::UnsignedInt);
        match (function.parameters.len(), function.locals.len()) {
            // `int x = g(a); return x + a + b;` — saved1 = a (param 0 -> r30),
            // saved2 = b (param 1 -> r31), the result local dies in r3.
            (2, 1) => {
                let local = &function.locals[0];
                if !all_int(local.declared_type)
                    || result_name != &local.name
                    || saved1 != &function.parameters[0].name
                    || saved2 != &function.parameters[1].name
                {
                    return Ok(false);
                }
                let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                    return Ok(false);
                };
                match arguments.as_slice() {
                    [] => {}
                    [Expression::Variable(argument)] if argument == &function.parameters[0].name => {}
                    _ => return Ok(false),
                }
                let mut incoming = Vec::new();
                for parameter in &function.parameters {
                    match self.locations.get(&parameter.name) {
                        Some(location) if location.class == ValueClass::General => incoming.push(location.register),
                        _ => return Ok(false),
                    }
                }
                self.non_leaf = true;
                self.frame_size = 16;
                // Reverse creation order: the LAST-created saved value takes r31.
                let homes: Vec<u8> = (0..2).map(|_| self.fresh_virtual_general()).collect();
                self.callee_saved = homes.clone();
                let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
                debug_assert_eq!(plan.frame_size, 16);
                // Interleaved save+fill pairs, r31 (param 1) first.
                let incoming_ordered: Vec<u8> = incoming.iter().rev().copied().collect();
                self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
                // The call reads the STILL-LIVE incoming register (no move).
                self.emit_call(name, arguments, None, false)?;
                self.emit_park_and_combine(homes[1], homes[0]);
                self.emit_epilogue_and_return();
                Ok(true)
            }
            // `int x = g(a); int y = g(x); return y + a + x;` — saved1 = a
            // (created first -> r30), saved2 = x (-> r31), y dies in r3.
            (1, 2) => {
                let (first, second) = (&function.locals[0], &function.locals[1]);
                if !all_int(first.declared_type)
                    || !all_int(second.declared_type)
                    || result_name != &second.name
                    || saved1 != &function.parameters[0].name
                    || saved2 != &first.name
                {
                    return Ok(false);
                }
                let Some(Expression::Call { name: call1, arguments: arguments1 }) = first.initializer.as_ref() else {
                    return Ok(false);
                };
                let Some(Expression::Call { name: call2, arguments: arguments2 }) = second.initializer.as_ref() else {
                    return Ok(false);
                };
                // Call 1 forwards the parameter (or nothing); call 2 passes the
                // FRESH first result — both read a still-live r3, no moves.
                match arguments1.as_slice() {
                    [] => {}
                    [Expression::Variable(argument)] if argument == &function.parameters[0].name => {}
                    _ => return Ok(false),
                }
                if !matches!(arguments2.as_slice(), [Expression::Variable(argument)] if argument == &first.name) {
                    return Ok(false);
                }
                let incoming = match self.locations.get(&function.parameters[0].name) {
                    Some(location) if location.class == ValueClass::General => location.register,
                    _ => return Ok(false),
                };
                self.non_leaf = true;
                self.frame_size = 16;
                let homes: Vec<u8> = (0..2).map(|_| self.fresh_virtual_general()).collect();
                self.callee_saved = homes.clone();
                let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
                debug_assert_eq!(plan.frame_size, 16);
                // Batched saves; the parameter's fill follows (its def is the
                // prologue), the first result's fill lands after its call.
                self.output.instructions.extend(plan.prologue());
                self.output.instructions.push(Instruction::Or { a: homes[1], s: incoming, b: incoming });
                self.emit_call(call1, arguments1, None, false)?;
                self.output.instructions.push(Instruction::Or { a: homes[0], s: 3, b: 3 });
                // The second call reads the first result from the STILL-LIVE r3
                // (its home copy exists but no argument move is emitted).
                let signed = !matches!(first.declared_type, Type::UnsignedInt);
                self.locations.insert(
                    first.name.clone(),
                    Location { class: ValueClass::General, register: 3, signed, width: 32, pointee: None, stride: None },
                );
                self.emit_call(call2, arguments2, None, false)?;
                self.emit_park_and_combine(homes[1], homes[0]);
                self.emit_epilogue_and_return();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// A local COMPUTED from the parameters, consumed by a call, and combined
    /// with that call's result: `int x = a*5+2; int y = g(x); return y + x;`
    /// (the fire-498 probe). The computation lands directly in the callee-saved
    /// home (intermediates in place, the scheduler hoists them into the
    /// prologue's latency gaps), the argument move `mr r3,r31` IS emitted (the
    /// home is not the argument register), and the combine reads r3 and r31.
    pub(crate) fn try_callee_saved_computed_then_call(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [computed, result] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(computed.declared_type, Type::Int | Type::UnsignedInt)
            || !matches!(result.declared_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        // The first local computes WITHOUT calling (its expression evaluates into
        // the home); the second is a call passing exactly the first.
        let Some(initializer) = computed.initializer.as_ref() else {
            return Ok(false);
        };
        if crate::analysis::expression_has_call(initializer) {
            return Ok(false);
        }
        let Some(Expression::Call { name, arguments }) = result.initializer.as_ref() else {
            return Ok(false);
        };
        if !matches!(arguments.as_slice(), [Expression::Variable(argument)] if argument == &computed.name) {
            return Ok(false);
        }
        // The return combines the result and the computed local with one
        // low-latency op, either order (evaluate_tail reproduces it).
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let reads = |expression: &Expression, name: &str| matches!(expression, Expression::Variable(variable) if variable == name);
        if !((reads(left, &result.name) && reads(right, &computed.name))
            || (reads(left, &computed.name) && reads(right, &result.name)))
        {
            return Ok(false);
        }
        self.non_leaf = true;
        self.frame_size = 16;
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The computation's INTERMEDIATES stay in place on the dying source
        // register; only the ROOT op lands in the callee-saved home (measured:
        // `mulli r3,r3,5; addi r31,r3,2` — the in-place mulli carries no
        // anti-dependence against the r31 save, so the scheduler may hoist it
        // into the prologue's latency gap). Only the measured root shape —
        // `<single-param subexpression> + <i16 literal>` — is emitted; other
        // roots decline to the honest defer.
        let (sub, literal) = match initializer {
            Expression::Binary { operator: BinaryOperator::Add, left, right } => match right.as_ref() {
                Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok() => (left.as_ref(), *value as i16),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        // The subexpression must be the measured ONE-instruction in-place shape
        // (`param * literal` -> mulli): the scheduler slot it fills — between
        // mflr and the LR store — is only measured for a single instruction.
        let (in_place_source, factor) = match sub {
            Expression::Binary { operator: BinaryOperator::Multiply, left, right } => match (left.as_ref(), right.as_ref()) {
                (Expression::Variable(source), Expression::IntegerLiteral(value)) if i16::try_from(*value).is_ok() => (source, *value as i16),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| &parameter.name == in_place_source) {
            return Ok(false);
        }
        let in_place = match self.locations.get(in_place_source.as_str()) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };
        // The prologue with the in-place multiply spliced into the mflr latency
        // gap (measured: stwu; mflr; MULLI; stw r0; stw r31), then the root op
        // landing in the home.
        let prologue = mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue();
        self.output.instructions.extend(prologue[..2].iter().cloned());
        self.output.instructions.push(Instruction::MultiplyImmediate { d: in_place, a: in_place, immediate: factor });
        self.output.instructions.extend(prologue[2..].iter().cloned());
        self.output.instructions.push(Instruction::AddImmediate { d: saved, a: in_place, immediate: literal });
        let signed = !matches!(computed.declared_type, Type::UnsignedInt);
        self.locations.insert(
            computed.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        self.emit_call(name, arguments, None, false)?;
        let result_signed = !matches!(result.declared_type, Type::UnsignedInt);
        self.locations.insert(
            result.name.clone(),
            Location { class: ValueClass::General, register: 3, signed: result_signed, width: 32, pointee: None, stride: None },
        );
        let destination = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, destination)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The measured reassociation tail: park the fresh call result in r0,
    /// combine the two callee-saved homes (creation order: lower home first),
    /// then add the parked value into the return register.
    pub(crate) fn emit_park_and_combine(&mut self, home_low: u8, home_high: u8) {
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 3, a: home_low, b: home_high });
        self.output.instructions.push(Instruction::Add { d: 3, a: 0, b: 3 });
    }

    /// One or two locals that are CALL RESULTS, live across later calls, then returned:
    /// `int z = g(); h(); return z;` or `int a = g1(); int b = g2(); h(); return a+b;`.
    /// mwcc preserves them in r31 (and r30) across the later calls — each producing call
    /// is followed by a move into its callee-saved register, all saved up front. The
    /// single-local return may post-process z (`z + 1`); the two-local return must be a
    /// single low-latency op of both (`a + b`), as in [`Self::try_callee_saved`].
    /// (Parameters live across calls go through that path.) Narrowly shaped.
    pub(crate) fn try_callee_saved_call_result(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // One or two general-int locals, each initialized by an argument-free call.
        let count = function.locals.len();
        if count == 0 || count > 2 {
            return Ok(false);
        }
        let mut init_calls: Vec<(String, Vec<Expression>)> = Vec::new();
        for local in &function.locals {
            if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
                return Ok(false);
            }
            let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                return Ok(false);
            };
            init_calls.push((name.clone(), arguments.clone()));
        }
        // A producing call's arguments are allowed only in the single-local case, and
        // only when they forward parameters in their natural register positions (arg i
        // is parameter i, all parameters general) — then the parameters are already in
        // place, no moves are emitted, and the sequence matches the argument-free shape.
        // A constant/reordered argument would schedule its materialization differently,
        // and a second producing call's parameter would be call-clobbered; both defer.
        let params_all_general = !function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double));
        for (index, (_, arguments)) in init_calls.iter().enumerate() {
            if arguments.is_empty() {
                continue;
            }
            let forwards_parameters = count == 1
                && index == 0
                && params_all_general
                && arguments.len() <= function.parameters.len()
                && arguments
                    .iter()
                    .enumerate()
                    .all(|(position, argument)| matches!(argument, Expression::Variable(name) if name == &function.parameters[position].name));
            if !forwards_parameters {
                return Ok(false);
            }
        }
        // The return reads no parameter (it would be a call-clobbered register) and no
        // global (its load reschedules against the epilogue). A single local may be
        // post-processed (`z + 1`); two locals must combine in one low-latency op
        // (`a + b`), the only shape whose restores aren't rescheduled.
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if function.parameters.iter().any(|parameter| expression_reads_name(return_expr, &parameter.name)) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(return_expr, name)) {
            return Ok(false);
        }
        if count == 1 {
            if !expression_reads_name(return_expr, &function.locals[0].name) {
                return Ok(false);
            }
        } else {
            let single_op = matches!(return_expr, Expression::Binary { operator, left, right }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                    && matches!(left.as_ref(), Expression::Variable(_))
                    && matches!(right.as_ref(), Expression::Variable(_)));
            if !single_op || !function.locals.iter().all(|local| expression_reads_name(return_expr, &local.name)) {
                return Ok(false);
            }
        }
        // The body is one or more straight-line argument-free calls (so the locals are
        // genuinely live across a call).
        if function.statements.is_empty() {
            return Ok(false);
        }
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if !arguments.is_empty() {
                return Ok(false);
            }
        }

        // Prologue: a frame saving the link register and the callee-saved registers
        // (r31, then r30), all up front, highest at the top of the frame.
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, created highest-rank first (id order -> r31, r30, …),
        // framed by the FRAME BUILDER (all saves consecutive — the canonical schedule).
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        self.output.instructions.extend(plan.prologue());

        // Each local: its producing call, then move r3 into the local's callee-saved
        // register — the first local takes the lowest (r30 when there are two), the last
        // takes r31, matching mwcc's `bl g1; mr r30,r3; bl g2; mr r31,r3`.
        for (index, local) in function.locals.iter().enumerate() {
            let (init_name, init_arguments) = &init_calls[index];
            self.emit_call(init_name, init_arguments, None, false)?;
            // The first local takes the LOWEST home (homes are highest-first).
            let register = homes[count - 1 - index];
            self.output.instructions.push(Instruction::Or { a: register, s: 3, b: 3 });
            let signed = !matches!(local.declared_type, Type::UnsignedInt);
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register, signed, width: 32, pointee: None, stride: None },
            );
        }

        // The later calls, then the return. The LR-reload hoist places the saved-LR
        // reload right after the last call, matching mwcc's epilogue order.
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Two int locals each initialized by an argument-free call, with NO trailing call —
    /// `int x = g(); int y = h(); return x OP y;`. The FIRST result is live across the
    /// second call, so it parks in one callee-saved register (r31); the second result
    /// stays in r3, and the return combines them straight from those registers:
    /// `bl g; mr r31,r3; bl h; <op> r3,r31,r3`. Distinct from
    /// `try_callee_saved_call_result`, whose model has a LATER call that BOTH locals
    /// cross (so it saves both, r30+r31) — here the last local is never live across a
    /// call and never saved. Low-latency combines only (`+ - & | ^`): a multiply's
    /// latency reschedules the epilogue restores (a scheduler concern) and defers, as
    /// does a reversed operand order. Same or different callees both work — the calls
    /// run in statement order, so the relocation order is natural (no commutative
    /// right-first reorder like the direct `return f()+g();` form).
    pub(crate) fn try_callee_saved_two_call_result_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // Exactly two int locals, each an argument-free call initializer.
        if function.locals.len() != 2 {
            return Ok(false);
        }
        let mut init_names: Vec<String> = Vec::new();
        for local in &function.locals {
            if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
                return Ok(false);
            }
            let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                return Ok(false);
            };
            if !arguments.is_empty() {
                return Ok(false);
            }
            init_names.push(name.clone());
        }
        // The return combines the two locals in declaration order with one low-latency op.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let (Expression::Variable(left_name), Expression::Variable(right_name)) = (left.as_ref(), right.as_ref()) else {
            return Ok(false);
        };
        if left_name != &function.locals[0].name || right_name != &function.locals[1].name {
            return Ok(false);
        }
        // `x OP y` from x in the saved register and y in r3. `subf d,a,b` = b - a, so
        // `subf r3,r3,saved` = saved - r3 = x - y (order-preserving).
        let combine = |saved: u8| match operator {
            BinaryOperator::Add => Some(Instruction::Add { d: 3, a: saved, b: 3 }),
            BinaryOperator::Subtract => Some(Instruction::SubtractFrom { d: 3, a: 3, b: saved }),
            BinaryOperator::BitOr => Some(Instruction::Or { a: 3, s: saved, b: 3 }),
            BinaryOperator::BitAnd => Some(Instruction::And { a: 3, s: saved, b: 3 }),
            BinaryOperator::BitXor => Some(Instruction::Xor { a: 3, s: saved, b: 3 }),
            _ => None,
        };
        if combine(0).is_none() {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the one callee-saved home.
        self.non_leaf = true;
        self.frame_size = 16;
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // First call; its result parks in the callee-saved register (live across the 2nd call).
        self.emit_call(&init_names[0], &[], None, false)?;
        self.output.instructions.push(Instruction::Or { a: saved, s: 3, b: 3 });
        // Second call; its result stays in r3, then the combine and return.
        self.emit_call(&init_names[1], &[], None, false)?;
        self.output.instructions.push(combine(saved).expect("operator checked above"));
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A single local COMPUTED from parameters (no call in its initializer) that is live
    /// across a call — passed to it and/or post-processed in the return:
    /// `int z = x + 1; g(z); return z;`. z is computed into r31 before the call, used
    /// from r31 (as a call argument and/or the return), then reloaded. Argument calls may
    /// pass only z and constants (a parameter would be call-clobbered).
    pub(crate) fn try_callee_saved_computed_local(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.locals.len() != 1 {
            return Ok(false);
        }
        let local = &function.locals[0];
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(initializer) = local.initializer.as_ref() else {
            return Ok(false);
        };
        // A genuinely computed initializer: not a bare variable (that keeps its source
        // register), not a call (the call-result path), reading no global.
        if matches!(initializer, Expression::Variable(_)) || expression_has_call(initializer) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(initializer, name)) {
            return Ok(false);
        }
        // One or more argument calls whose arguments read only z (preserved in r31) and
        // constants; the return reads z and no parameter/global. (A parameter in either
        // would be read from a call-clobbered register.)
        if function.statements.is_empty() {
            return Ok(false);
        }
        let reads_param_or_global = |this: &Self, expression: &Expression| {
            function.parameters.iter().any(|parameter| expression_reads_name(expression, &parameter.name))
                || this.globals.keys().any(|name| expression_reads_name(expression, name))
        };
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if arguments.iter().any(|argument| reads_param_or_global(self, argument)) {
                return Ok(false);
            }
        }
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !expression_reads_name(return_expr, &local.name) || reads_param_or_global(self, return_expr) {
            return Ok(false);
        }

        // Prologue, then compute z into r31, then the argument calls, then the return.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the computed local's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        self.evaluate_general(initializer, saved)?;
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

}
