//! Typed word/pair values with explicit conditional merges.
//!
//! The graph fixes memory/call ordering and assignment identities before
//! selection. A 64-bit value always has two explicit virtual-register words;
//! ordinary liveness, scheduling and frame allocation preserve both across
//! calls. Both incoming edges define a branch merge explicitly. Constants
//! remain operands until use, avoiding artificial survivors.

use super::*;
use mwcc_vreg::Label;
use std::collections::HashMap;

#[path = "wide_value_graph/arithmetic.rs"]
mod arithmetic;
#[path = "wide_value_graph/control.rs"]
mod control;

fn supported(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::UnsignedInt
            | Type::Pointer(_)
            | Type::StructPointer { .. }
            | Type::LongLong
            | Type::UnsignedLongLong
    )
}
fn wide(ty: Type) -> bool {
    matches!(ty, Type::LongLong | Type::UnsignedLongLong)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Value {
    ty: Type,
    source: Source,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Constant(u64),
    Register(usize),
}
#[derive(Debug)]
enum Operation {
    Local {
        result: Value,
    },
    Label(usize),
    Jump(usize),
    BranchIfZero {
        condition: Value,
        target: usize,
    },
    Return(Option<Value>),
    Branch {
        condition: Value,
        then_body: Vec<Operation>,
        else_body: Vec<Operation>,
    },
    Copy {
        result: Value,
        value: Value,
    },
    Offset {
        result: Value,
        pointer: Value,
        bytes: u32,
    },
    Parameter {
        result: Value,
        high: u32,
    },
    Address {
        result: Value,
        symbol: String,
    },
    Load {
        result: Value,
        pointer: Value,
    },
    Store {
        pointer: Value,
        value: Value,
    },
    Convert {
        result: Value,
        value: Value,
    },
    Binary {
        result: Value,
        operator: BinaryOperator,
        left: Value,
        right: Value,
    },
    Call {
        runtime: bool,
        name: String,
        arguments: Vec<(u32, Value)>,
        result: Option<Value>,
    },
}

struct Graph<'a> {
    operations: Vec<Operation>,
    values: usize,
    bindings: HashMap<String, Value>,
    types: HashMap<String, Type>,
    globals: &'a HashMap<String, Type>,
    returns: &'a HashMap<String, Type>,
    parameters: &'a HashMap<String, Vec<Type>>,
    returned: Option<Value>,
    return_type: Type,
    fixed_bindings: bool,
    labels: usize,
    loop_targets: Vec<(usize, usize)>,
}

/// The word/pair-only EABI subset. A pair starts at an odd GPR. Reject overflow
/// before emitting anything: stack arguments need their own pair slot layout.
fn abi_registers(types: impl IntoIterator<Item = Type>) -> Option<Vec<u32>> {
    let mut next = u32::from(Eabi::FIRST_GENERAL_ARGUMENT);
    let mut registers = Vec::new();
    for ty in types {
        if !supported(ty) {
            return None;
        }
        if wide(ty) && next % 2 == 0 {
            next += 1;
        }
        let words = if wide(ty) { 2 } else { 1 };
        if next + words - 1 > u32::from(Eabi::LAST_GENERAL_ARGUMENT) {
            return None;
        }
        registers.push(next);
        next += words;
    }
    Some(registers)
}

impl<'a> Graph<'a> {
    fn fresh(&mut self, ty: Type) -> Value {
        let value = Value {
            ty,
            source: Source::Register(self.values),
        };
        self.values += 1;
        value
    }

    fn convert(&mut self, value: Value, ty: Type) -> Option<Value> {
        if !supported(ty) {
            return None;
        }
        if let Source::Constant(bits) = value.source {
            let bits = if !wide(ty) {
                bits & 0xffff_ffff
            } else if !wide(value.ty) && value.ty.is_signed() {
                (bits as i32 as i64) as u64
            } else {
                bits
            };
            return Some(Value {
                ty,
                source: Source::Constant(bits),
            });
        }
        if wide(ty) == wide(value.ty) {
            return Some(Value { ty, ..value });
        }
        let result = self.fresh(ty);
        self.operations.push(Operation::Convert { result, value });
        Some(result)
    }

    fn address(&mut self, name: &str) -> Option<Value> {
        let ty = *self.globals.get(name)?;
        if !(supported(ty) || matches!(ty, Type::Struct { .. })) || self.types.contains_key(name) {
            return None;
        }
        // The pointee identity is supplied by the load/store, so this internal
        // address is an unsigned word independent of the source object width.
        let result = self.fresh(Type::Pointer(Pointee::UnsignedInt));
        self.operations.push(Operation::Address {
            result,
            symbol: name.into(),
        });
        Some(result)
    }

    fn expression(&mut self, expression: &Expression) -> Option<Value> {
        match expression {
            Expression::IntegerLiteral(bits) => Some(Value {
                ty: if i32::try_from(*bits).is_ok() {
                    Type::Int
                } else if u32::try_from(*bits).is_ok() {
                    Type::UnsignedInt
                } else {
                    Type::LongLong
                },
                source: Source::Constant(*bits as u64),
            }),
            Expression::Variable(name) => {
                if let Some(value) = self.bindings.get(name) {
                    return Some(*value);
                }
                let ty = *self.globals.get(name)?;
                if !supported(ty) {
                    return None;
                }
                let pointer = self.address(name)?;
                let result = self.fresh(ty);
                self.operations.push(Operation::Load { result, pointer });
                Some(result)
            }
            Expression::Cast {
                target_type,
                operand,
            } => {
                let value = self.expression(operand)?;
                self.convert(value, *target_type)
            }
            Expression::AddressOf { operand } => {
                let Expression::Variable(name) = operand.as_ref() else {
                    return None;
                };
                let address = self.address(name)?;
                let ty = match self.globals.get(name)? {
                    Type::LongLong => Type::Pointer(Pointee::LongLong),
                    Type::UnsignedLongLong => Type::Pointer(Pointee::UnsignedLongLong),
                    Type::Int => Type::Pointer(Pointee::Int),
                    Type::UnsignedInt => Type::Pointer(Pointee::UnsignedInt),
                    _ => return None,
                };
                Some(Value { ty, ..address })
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                let Type::Pointer(pointee) = pointer.ty else {
                    return None;
                };
                let ty = pointee.element();
                if !supported(ty) {
                    return None;
                }
                let result = self.fresh(ty);
                self.operations.push(Operation::Load { result, pointer });
                Some(result)
            }
            Expression::Member {
                base,
                offset,
                member_type,
                ..
            } => {
                let pointer = self.member_address(base, *offset)?;
                if !supported(*member_type) {
                    return None;
                }
                let result = self.fresh(*member_type);
                self.operations.push(Operation::Load { result, pointer });
                Some(result)
            }
            Expression::Assign { target, value } => {
                let value = self.expression(value)?;
                self.store(target, value)
            }
            Expression::Call { name, arguments } => self.call(name, arguments, true)?,
            Expression::Binary {
                operator,
                left,
                right,
            } if matches!(
                operator,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual
            ) =>
            {
                // Call-bearing RHS first matches the time-adjust transaction:
                // obtain the current clock, then read the adjustment memory.
                // Separate statements and explicit comma expressions never
                // enter this rule, so their sequencing cannot be crossed.
                let (left, right) = if expression_has_side_effect(right) {
                    let right = self.expression(right)?;
                    (self.expression(left)?, right)
                } else {
                    let left = self.expression(left)?;
                    (left, self.expression(right)?)
                };
                let pointers = matches!(left.ty, Type::Pointer(_) | Type::StructPointer { .. })
                    || matches!(right.ty, Type::Pointer(_) | Type::StructPointer { .. });
                if pointers && !is_comparison(*operator) {
                    return None;
                }
                let shift = matches!(
                    operator,
                    BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                );
                let ty = if shift {
                    left.ty
                } else if pointers {
                    Type::UnsignedInt
                } else if left.ty == Type::UnsignedLongLong || right.ty == Type::UnsignedLongLong {
                    Type::UnsignedLongLong
                } else if wide(left.ty) || wide(right.ty) {
                    Type::LongLong
                } else if left.ty == Type::UnsignedInt || right.ty == Type::UnsignedInt {
                    Type::UnsignedInt
                } else {
                    Type::Int
                };
                let left = self.convert(left, ty)?;
                let right = self.convert(right, if shift { Type::UnsignedInt } else { ty })?;
                if let (Source::Constant(left), Source::Constant(right)) =
                    (left.source, right.source)
                {
                    if let Some(bits) = arithmetic::fold(*operator, ty, left, right) {
                        return Some(Value {
                            ty: if is_comparison(*operator) {
                                Type::Int
                            } else {
                                ty
                            },
                            source: Source::Constant(bits),
                        });
                    }
                }
                let result = self.fresh(if is_comparison(*operator) {
                    Type::Int
                } else {
                    ty
                });
                if wide(ty) && matches!(operator, BinaryOperator::Divide | BinaryOperator::Modulo) {
                    let name = match (*operator, ty.is_signed()) {
                        (BinaryOperator::Divide, true) => "__div2i",
                        (BinaryOperator::Divide, false) => "__div2u",
                        (BinaryOperator::Modulo, true) => "__mod2i",
                        _ => "__mod2u",
                    };
                    self.operations.push(Operation::Call {
                        runtime: true,
                        name: name.into(),
                        arguments: vec![(3, left), (5, right)],
                        result: Some(result),
                    });
                    return Some(result);
                }
                // Variable pair shifts have a different runtime ABI and are
                // kept outside this graph until their lowering is measured.
                if wide(ty) && shift && !matches!(right.source, Source::Constant(n) if n < 64) {
                    return None;
                }
                self.operations.push(Operation::Binary {
                    result,
                    operator: *operator,
                    left,
                    right,
                });
                Some(result)
            }
            _ => None,
        }
    }

    fn call(&mut self, name: &str, arguments: &[Expression], used: bool) -> Option<Option<Value>> {
        if self.types.contains_key(name) || self.globals.contains_key(name) {
            return None;
        }
        let formals = self.parameters.get(name)?.clone();
        if formals.len() != arguments.len() {
            return None;
        }
        let registers = abi_registers(formals.iter().copied())?;
        let mut inputs = Vec::new();
        // Evaluate all arguments before touching ABI destinations. Physical
        // parallel-copy constraints are then visible to the normal allocator.
        for ((argument, ty), high) in arguments.iter().zip(formals).zip(registers) {
            let value = self.expression(argument)?;
            inputs.push((high, self.convert(value, ty)?));
        }
        let ty = *self.returns.get(name)?;
        if !(supported(ty) || ty == Type::Void) || (used && ty == Type::Void) {
            return None;
        }
        let result = used.then(|| self.fresh(ty));
        self.operations.push(Operation::Call {
            runtime: false,
            name: name.into(),
            arguments: inputs,
            result,
        });
        Some(result)
    }

    fn assign(&mut self, name: &str, expression: &Expression) -> Option<()> {
        let ty = *self.types.get(name)?;
        let value = self.expression(expression)?;
        let value = self.convert(value, ty)?;
        if self.fixed_bindings {
            self.operations.push(Operation::Copy {
                result: *self.bindings.get(name)?,
                value,
            });
        } else {
            self.bindings.insert(name.into(), value);
        }
        Some(())
    }

    fn store(&mut self, target: &Expression, value: Value) -> Option<Value> {
        if let Expression::Variable(name) = target {
            if let Some(ty) = self.types.get(name).copied() {
                let value = self.convert(value, ty)?;
                if self.fixed_bindings {
                    self.operations.push(Operation::Copy {
                        result: *self.bindings.get(name)?,
                        value,
                    });
                } else {
                    self.bindings.insert(name.clone(), value);
                }
                return Some(value);
            }
        }
        let (pointer, ty) = match target {
            Expression::Variable(name) => (self.address(name)?, *self.globals.get(name)?),
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                let Type::Pointer(pointee) = pointer.ty else {
                    return None;
                };
                (pointer, pointee.element())
            }
            Expression::Member {
                base,
                offset,
                member_type,
                ..
            } => (self.member_address(base, *offset)?, *member_type),
            _ => return None,
        };
        let value = self.convert(value, ty)?;
        self.operations.push(Operation::Store { pointer, value });
        Some(value)
    }

    fn member_address(&mut self, base: &Expression, bytes: u32) -> Option<Value> {
        let pointer = if let Expression::Variable(name) = base {
            if !self.types.contains_key(name)
                && matches!(self.globals.get(name), Some(Type::Struct { .. }))
            {
                self.address(name)?
            } else {
                self.expression(base)?
            }
        } else {
            self.expression(base)?
        };
        if !matches!(pointer.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
            return None;
        }
        let result = self.fresh(Type::Pointer(Pointee::UnsignedInt));
        self.operations.push(Operation::Offset {
            result,
            pointer,
            bytes,
        });
        Some(result)
    }

    fn statements(&mut self, statements: &[Statement]) -> Option<()> {
        for statement in statements {
            match statement {
                Statement::Assign { name, value } => self.assign(name, value)?,
                Statement::Expression(expression) => {
                    self.effect(expression)?;
                }
                Statement::Store { target, value } => {
                    let value = self.expression(value)?;
                    self.store(target, value)?;
                }
                Statement::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => {
                    self.loop_body(
                        *kind,
                        initializer.as_ref(),
                        condition.as_ref(),
                        step.as_ref(),
                        body,
                    )?;
                }
                Statement::Break => self
                    .operations
                    .push(Operation::Jump(self.loop_targets.last()?.0)),
                Statement::Continue => self
                    .operations
                    .push(Operation::Jump(self.loop_targets.last()?.1)),
                Statement::Return(value) => {
                    let value = match (self.return_type, value) {
                        (Type::Void, None) => None,
                        (Type::Void, Some(_)) | (_, None) => return None,
                        (ty, Some(expression)) => {
                            let value = self.expression(expression)?;
                            Some(self.convert(value, ty)?)
                        }
                    };
                    self.operations.push(Operation::Return(value));
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    let condition = self.expression(condition)?;
                    let condition = self.truth(condition);
                    let before = self.bindings.clone();
                    let outer = std::mem::take(&mut self.operations);
                    self.statements(then_body)?;
                    let mut then_ops = std::mem::take(&mut self.operations);
                    let then_values = std::mem::replace(&mut self.bindings, before.clone());
                    self.statements(else_body)?;
                    let mut else_ops = std::mem::take(&mut self.operations);
                    let else_values = std::mem::take(&mut self.bindings);
                    // A merge is explicit in both predecessor blocks. Ignore
                    // locals without a definition on both incoming paths.
                    let mut names: Vec<_> = then_values
                        .keys()
                        .filter(|n| else_values.contains_key(*n))
                        .cloned()
                        .collect();
                    names.sort();
                    for name in names {
                        let yes = then_values[&name];
                        let no = else_values[&name];
                        let value = if yes == no {
                            yes
                        } else {
                            let result = self.fresh(self.types[&name]);
                            then_ops.push(Operation::Copy { result, value: yes });
                            else_ops.push(Operation::Copy { result, value: no });
                            result
                        };
                        self.bindings.insert(name, value);
                    }
                    self.operations = outer;
                    self.operations.push(Operation::Branch {
                        condition,
                        then_body: then_ops,
                        else_body: else_ops,
                    });
                }
                _ => return None,
            }
        }
        Some(())
    }

    fn recognize(
        function: &Function,
        globals: &'a HashMap<String, Type>,
        returns: &'a HashMap<String, Type>,
        parameters: &'a HashMap<String, Vec<Type>>,
    ) -> Option<Self> {
        if !(supported(function.return_type) || function.return_type == Type::Void)
            || function.locals.iter().any(|l| {
                !supported(l.declared_type)
                    || l.is_volatile
                    || l.is_static
                    || l.array_length.is_some()
            })
        {
            return None;
        }
        // The parser extracts terminal guarded returns after the statement
        // prefix. Restore that position before building control-flow edges.
        let guarded_statements;
        let source_statements = if function.guards.is_empty() {
            function.statements.as_slice()
        } else {
            guarded_statements = function
                .statements
                .iter()
                .cloned()
                .chain(function.guards.iter().map(|guard| Statement::If {
                    condition: guard.condition.clone(),
                    then_body: vec![Statement::Return(Some(guard.value.clone()))],
                    else_body: Vec::new(),
                }))
                .collect::<Vec<_>>();
            guarded_statements.as_slice()
        };
        let incoming = abi_registers(function.parameters.iter().map(|p| p.parameter_type))?;
        let mut graph = Self {
            operations: Vec::new(),
            values: 0,
            bindings: HashMap::new(),
            types: function
                .locals
                .iter()
                .map(|l| (l.name.clone(), l.declared_type))
                .chain(
                    function
                        .parameters
                        .iter()
                        .map(|p| (p.name.clone(), p.parameter_type)),
                )
                .collect(),
            globals,
            returns,
            parameters,
            returned: None,
            return_type: function.return_type,
            fixed_bindings: control::requires_homes(source_statements, false),
            labels: 0,
            loop_targets: Vec::new(),
        };
        for (parameter, high) in function.parameters.iter().zip(incoming) {
            let result = graph.fresh(parameter.parameter_type);
            graph.operations.push(Operation::Parameter { result, high });
            graph.bindings.insert(parameter.name.clone(), result);
        }
        if graph.fixed_bindings {
            for local in &function.locals {
                let result = graph.fresh(local.declared_type);
                graph.operations.push(Operation::Local { result });
                graph.bindings.insert(local.name.clone(), result);
            }
        }
        for local in &function.locals {
            if let Some(value) = &local.initializer {
                graph.assign(&local.name, value)?;
            }
        }
        let mut returned = function.return_expression.as_ref();
        let mut statements = source_statements;
        if let Some((Statement::Return(value), prefix)) = statements.split_last() {
            if returned.is_some() {
                return None;
            }
            returned = value.as_ref();
            statements = prefix;
        }
        graph.statements(statements)?;
        if function.return_type == Type::Void {
            if returned.is_some() {
                return None;
            }
        } else if let Some(expression) = returned {
            let value = graph.expression(expression)?;
            graph.returned = Some(graph.convert(value, function.return_type)?);
        } else if control::falls_through(statements) {
            return None;
        }
        Some(graph)
    }
}

/// Scalar words use `low`; pairs use `high:low` in big-endian ABI order.
#[derive(Clone, Copy)]
struct Registers {
    high: Option<u32>,
    low: u32,
}

impl Generator {
    pub(crate) fn try_wide_value_graph(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !self.output.instructions.is_empty() {
            return Ok(false);
        }
        let Some(graph) = Graph::recognize(
            function,
            &self.globals,
            &self.call_return_types,
            &self.call_parameter_types,
        ) else {
            return Ok(false);
        };
        let operations = graph.operations;
        let returned = graph.returned;
        let mut registers = vec![None; graph.values];
        // Explicit pairs and memory sequencing are selected here. The allocator
        // still owns survivors and their frame; do not reschedule split loads
        // across calls or separate a carry producer from its consumer.
        self.output.pre_scheduled = true;
        fn has_call(operations: &[Operation]) -> bool {
            operations.iter().any(|op| match op {
                Operation::Call { .. } => true,
                Operation::Branch {
                    then_body,
                    else_body,
                    ..
                } => has_call(then_body) || has_call(else_body),
                _ => false,
            })
        }
        if has_call(&operations) {
            // Allocation grows this canonical frame and then applies the
            // generation's linkage-first/predecrement convention.
            self.non_leaf = true;
            self.frame_size = 16;
            self.output.instructions.extend([
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16,
                },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                },
            ]);
        }
        let labels: Vec<_> = (0..graph.labels).map(|_| self.fresh_label()).collect();
        let exit = self.fresh_label();
        self.emit_wide_graph_operations(operations, &mut registers, &labels, exit)?;
        if let Some(value) = returned {
            if let Source::Constant(bits) = value.source {
                // The return ABI is the destination of a terminal constant;
                // do not create a temporary pair and then copy it back.
                self.load_integer_constant(if wide(value.ty) { 4 } else { 3 }, bits as i64);
                if wide(value.ty) {
                    self.load_integer_constant(3, (bits >> 32) as i64);
                }
            } else {
                let value = self.wide_graph_operand(value, &registers);
                self.output.instructions.push(Instruction::move_register(
                    if value.high.is_some() { 4 } else { 3 },
                    value.low,
                ));
                if let Some(high) = value.high {
                    self.output
                        .instructions
                        .push(Instruction::move_register(3, high));
                }
            }
        }
        self.bind_label(exit);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_wide_graph_operations(
        &mut self,
        operations: Vec<Operation>,
        registers: &mut [Option<Registers>],
        labels: &[Label],
        exit: Label,
    ) -> Compilation<()> {
        for operation in operations {
            match operation {
                Operation::Local { result } => {
                    self.wide_graph_destination(result, registers);
                }
                Operation::Label(label) => self.bind_label(labels[label]),
                Operation::Jump(label) => self.emit_branch_to(labels[label]),
                Operation::BranchIfZero { condition, target } => {
                    let condition = self.wide_graph_operand(condition, registers).low;
                    self.output
                        .instructions
                        .push(Instruction::CompareWordImmediate {
                            a: condition,
                            immediate: 0,
                        });
                    self.emit_branch_conditional_to(12, 2, labels[target]);
                }
                Operation::Return(value) => {
                    if let Some(value) = value {
                        let value = self.wide_graph_operand(value, registers);
                        self.output.instructions.push(Instruction::move_register(
                            if value.high.is_some() { 4 } else { 3 },
                            value.low,
                        ));
                        if let Some(high) = value.high {
                            self.output
                                .instructions
                                .push(Instruction::move_register(3, high));
                        }
                    }
                    self.emit_branch_to(exit);
                }
                Operation::Branch {
                    condition,
                    then_body,
                    else_body,
                } => {
                    let condition = self.wide_graph_operand(condition, registers).low;
                    let otherwise = self.fresh_label();
                    let join = self.fresh_label();
                    self.output
                        .instructions
                        .push(Instruction::CompareWordImmediate {
                            a: condition,
                            immediate: 0,
                        });
                    self.emit_branch_conditional_to(12, 2, otherwise);
                    self.emit_wide_graph_operations(then_body, registers, labels, exit)?;
                    self.emit_branch_to(join);
                    self.bind_label(otherwise);
                    self.emit_wide_graph_operations(else_body, registers, labels, exit)?;
                    self.bind_label(join);
                }
                Operation::Copy { result, value } => {
                    let source = self.wide_graph_operand(value, registers);
                    let destination = self.wide_graph_destination(result, registers);
                    self.output
                        .instructions
                        .push(Instruction::move_register(destination.low, source.low));
                    if let Some(high) = destination.high {
                        self.output
                            .instructions
                            .push(Instruction::move_register(high, source.high.unwrap()));
                    }
                }
                Operation::Offset {
                    result,
                    pointer,
                    bytes,
                } => {
                    let pointer = self.wide_graph_operand(pointer, registers).low;
                    let destination = self.wide_graph_destination(result, registers).low;
                    if let Ok(immediate) = i16::try_from(bytes) {
                        self.output.instructions.push(Instruction::AddImmediate {
                            d: destination,
                            a: pointer,
                            immediate,
                        });
                    } else {
                        let offset = self.fresh_virtual_general();
                        self.load_integer_constant(offset, i64::from(bytes));
                        self.output.instructions.push(Instruction::Add {
                            d: destination,
                            a: pointer,
                            b: offset,
                        });
                    }
                }
                Operation::Parameter { result, high } => {
                    let destination = self.wide_graph_destination(result, registers);
                    if let Some(d) = destination.high {
                        self.output
                            .instructions
                            .push(Instruction::move_register(destination.low, high + 1));
                        self.output
                            .instructions
                            .push(Instruction::move_register(d, high));
                    } else {
                        self.output
                            .instructions
                            .push(Instruction::move_register(destination.low, high));
                    }
                }
                Operation::Address { result, symbol } => {
                    let destination = self.wide_graph_destination(result, registers).low;
                    self.emit_address_high(destination, &symbol);
                    self.record_relocation(RelocationKind::Addr16Lo, &symbol);
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: destination,
                        a: destination,
                        immediate: 0,
                    });
                }
                Operation::Load { result, pointer } => {
                    let pointer = self.wide_graph_operand(pointer, registers).low;
                    let destination = self.wide_graph_destination(result, registers);
                    if let Some(high) = destination.high {
                        self.output.instructions.push(Instruction::LoadWord {
                            d: high,
                            a: pointer,
                            offset: 0,
                        });
                        self.output.instructions.push(Instruction::LoadWord {
                            d: destination.low,
                            a: pointer,
                            offset: 4,
                        });
                    } else {
                        self.output.instructions.push(Instruction::LoadWord {
                            d: destination.low,
                            a: pointer,
                            offset: 0,
                        });
                    }
                }
                Operation::Store { pointer, value } => {
                    let pointer = self.wide_graph_operand(pointer, registers).low;
                    let value = self.wide_graph_operand(value, registers);
                    self.output.instructions.push(Instruction::StoreWord {
                        s: value.low,
                        a: pointer,
                        offset: if value.high.is_some() { 4 } else { 0 },
                    });
                    if let Some(high) = value.high {
                        self.output.instructions.push(Instruction::StoreWord {
                            s: high,
                            a: pointer,
                            offset: 0,
                        });
                    }
                }
                Operation::Convert { result, value } => {
                    let source = self.wide_graph_operand(value, registers);
                    let destination = self.wide_graph_destination(result, registers);
                    self.output
                        .instructions
                        .push(Instruction::move_register(destination.low, source.low));
                    if let Some(high) = destination.high {
                        if value.ty.is_signed() {
                            self.output.instructions.push(
                                Instruction::ShiftRightAlgebraicImmediate {
                                    a: high,
                                    s: source.low,
                                    shift: 31,
                                },
                            );
                        } else {
                            self.load_integer_constant(high, 0);
                        }
                    }
                }
                Operation::Binary {
                    result,
                    operator,
                    left,
                    right,
                } => {
                    self.emit_wide_graph_arithmetic(result, operator, left, right, registers)?;
                }
                Operation::Call {
                    runtime,
                    name,
                    arguments,
                    result,
                } => {
                    if runtime {
                        self.call_parameter_types
                            .insert(name.clone(), vec![Type::LongLong, Type::LongLong]);
                        self.call_return_types.insert(name.clone(), Type::LongLong);
                        if !self.compiler_generated_symbols.contains(&name) {
                            self.compiler_generated_symbols.push(name.clone());
                        }
                    }
                    // Materialize every operand before starting the ABI copy
                    // group; constants must not clobber already filled inputs.
                    let arguments: Vec<_> = arguments
                        .into_iter()
                        .map(|(high, value)| (high, self.wide_graph_operand(value, registers)))
                        .collect();
                    for (high, value) in arguments.into_iter().rev() {
                        if let Some(source) = value.high {
                            self.output
                                .instructions
                                .push(Instruction::move_register(high + 1, value.low));
                            self.output
                                .instructions
                                .push(Instruction::move_register(high, source));
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::move_register(high, value.low));
                        }
                    }
                    self.record_relocation(RelocationKind::Rel24, &name);
                    self.output
                        .instructions
                        .push(Instruction::BranchAndLink { target: name });
                    if let Some(result) = result {
                        let destination = self.wide_graph_destination(result, registers);
                        self.output.instructions.push(Instruction::move_register(
                            destination.low,
                            if destination.high.is_some() { 4 } else { 3 },
                        ));
                        if let Some(high) = destination.high {
                            self.output
                                .instructions
                                .push(Instruction::move_register(high, 3));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn wide_graph_destination(
        &mut self,
        value: Value,
        registers: &mut [Option<Registers>],
    ) -> Registers {
        let Source::Register(id) = value.source else {
            unreachable!()
        };
        if let Some(destination) = registers[id] {
            return destination;
        }
        let destination = Registers {
            low: self.fresh_virtual_general(),
            high: wide(value.ty).then(|| self.fresh_virtual_general()),
        };
        registers[id] = Some(destination);
        destination
    }

    fn wide_graph_operand(&mut self, value: Value, registers: &[Option<Registers>]) -> Registers {
        match value.source {
            Source::Register(id) => registers[id].expect("graph values are defined before use"),
            Source::Constant(bits) => {
                let low = self.fresh_virtual_general();
                self.load_integer_constant(low, bits as i64);
                let high = wide(value.ty).then(|| {
                    let high = self.fresh_virtual_general();
                    self.load_integer_constant(high, (bits >> 32) as i64);
                    high
                });
                Registers { high, low }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_pair_abi_pads_and_rejects_overflow() {
        assert_eq!(
            abi_registers([Type::Int, Type::LongLong, Type::Int]),
            Some(vec![3, 5, 7])
        );
        assert_eq!(abi_registers([Type::LongLong; 4]), Some(vec![3, 5, 7, 9]));
        assert!(abi_registers([Type::LongLong; 5]).is_none());
        assert!(abi_registers([Type::Int, Type::Double]).is_none());
    }

    #[test]
    fn word_promotion_preserves_source_signedness() {
        let globals = HashMap::new();
        let returns = HashMap::new();
        let parameters = HashMap::new();
        let mut graph = Graph {
            operations: Vec::new(),
            values: 0,
            bindings: HashMap::new(),
            types: HashMap::new(),
            globals: &globals,
            returns: &returns,
            parameters: &parameters,
            returned: None,
            return_type: Type::Void,
            fixed_bindings: false,
            labels: 0,
            loop_targets: Vec::new(),
        };
        for (ty, expected) in [(Type::Int, u64::MAX), (Type::UnsignedInt, 0xffff_ffff)] {
            let input = Value {
                ty,
                source: Source::Constant(0xffff_ffff),
            };
            let value = graph.convert(input, Type::UnsignedLongLong).unwrap();
            assert!(matches!(value.source, Source::Constant(bits) if bits == expected));
        }
        let wide_value = Value {
            ty: Type::UnsignedLongLong,
            source: Source::Constant(0x123456789abcdef0),
        };
        let low = graph.convert(wide_value, Type::UnsignedInt).unwrap();
        assert!(matches!(low.source, Source::Constant(0x9abcdef0)));
        assert!(graph.convert(wide_value, Type::Double).is_none());
        let sum = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Cast {
                target_type: Type::UnsignedLongLong,
                operand: Box::new(Expression::IntegerLiteral(0xffff_ffff)),
            }),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        let folded = graph.expression(&sum).unwrap();
        assert!(matches!(folded.source, Source::Constant(0x100000000)));
        assert!(graph.operations.is_empty());
    }
}
