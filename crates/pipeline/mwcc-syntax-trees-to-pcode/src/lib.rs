//! INITIAL CODE: lower a function's syntax tree to PCode over virtual
//! registers, as MWCC's `CodeGen_Generator` does before any backend pass.
//!
//! Virtual-register numbering follows MWCC's object-preallocation walk
//! (`CodeGen_PreallocateObjectRegisters`): parameters, then declared locals,
//! then the coalescing window opens and lowering temporaries follow in
//! creation order. Every unsupported construct rejects the whole function
//! with a diagnostic so the caller can fall back to another lowering.

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind, RelocationTarget};
use mwcc_pcode::{
    AttachedRelocation, Block, Class, PCodeFunction, PInstr, Register, ReturnRegisters,
};
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, Pointee, Statement, Type, UnaryOperator,
};

/// A file-scope object as lowering sees it.
#[derive(Debug, Clone, Copy)]
pub struct GlobalInfo {
    pub ty: Type,
    /// Addressed through the small-data base (`@sda21`) rather than `lis/@l`.
    pub small_data: bool,
}

/// Unit-level facts lowering needs.
pub struct LoweringContext<'a> {
    pub globals: &'a HashMap<String, GlobalInfo>,
    pub call_return_types: &'a HashMap<String, Type>,
}

/// A lowered function.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub pcode: PCodeFunction,
    /// The function calls another (it needs a frame and the saved LR).
    pub makes_calls: bool,
}

const FIRST_GENERAL_ARGUMENT: u32 = 3;
const LAST_GENERAL_ARGUMENT: u32 = 10;
const VOLATILE_GENERAL: [u32; 11] = [0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

fn unsupported(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("PCode lowering: {} (not yet supported)", what.into()))
}

/// Lower `function` to PCode.
pub fn lower(function: &Function, context: &LoweringContext<'_>) -> Compilation<Lowered> {
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return Err(unsupported("inline assembly"));
    }
    if !function.guards.is_empty() {
        return Err(unsupported("guarded returns"));
    }
    let returns = match function.return_type {
        Type::Void => ReturnRegisters::None,
        ty if is_general_word(ty) => ReturnRegisters::General,
        other => return Err(unsupported(format!("return type {other:?}"))),
    };
    let mut lowerer = Lowerer {
        context,
        pcode: PCodeFunction::new(function.name.clone(), returns),
        variables: HashMap::new(),
        makes_calls: false,
        return_type: function.return_type,
    };
    lowerer.lower_function(function)?;
    Ok(Lowered { pcode: lowerer.pcode, makes_calls: lowerer.makes_calls })
}

fn is_general_word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::UnsignedInt
            | Type::Char
            | Type::UnsignedChar
            | Type::Short
            | Type::UnsignedShort
            | Type::Pointer(_)
            | Type::StructPointer { .. }
    )
}

fn is_unsigned(ty: Type) -> bool {
    matches!(
        ty,
        Type::UnsignedInt
            | Type::UnsignedChar
            | Type::UnsignedShort
            | Type::Pointer(_)
            | Type::StructPointer { .. }
    )
}

/// The C integer promotion of an operand type.
fn promote(ty: Type) -> Type {
    match ty {
        Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort => Type::Int,
        other => other,
    }
}

fn pointee_type(pointee: Pointee) -> Option<Type> {
    Some(match pointee {
        Pointee::Int => Type::Int,
        Pointee::UnsignedInt => Type::UnsignedInt,
        Pointee::Char => Type::Char,
        Pointee::UnsignedChar => Type::UnsignedChar,
        Pointee::Short => Type::Short,
        Pointee::UnsignedShort => Type::UnsignedShort,
        Pointee::Pointer | Pointee::WordPointer => Type::Pointer(Pointee::Int),
        _ => return None,
    })
}

struct Variable {
    register: u32,
    ty: Type,
}

struct Lowerer<'a> {
    context: &'a LoweringContext<'a>,
    pcode: PCodeFunction,
    variables: HashMap<String, Variable>,
    makes_calls: bool,
    return_type: Type,
}

impl Lowerer<'_> {
    fn emit(&mut self, instruction: PInstr) {
        let block = self.pcode.blocks.len() - 1;
        self.pcode.blocks[block].instructions.push(instruction);
    }

    fn emit_plain(&mut self, instruction: Instruction) {
        self.emit(PInstr::new(instruction));
    }

    /// An instruction whose `base` operand may not be r0.
    fn emit_based(&mut self, instruction: Instruction, base: u32) {
        let mut instruction = PInstr::new(instruction);
        instruction.not_r0.push(base);
        self.emit(instruction);
    }

    fn temporary(&mut self) -> u32 {
        self.pcode.fresh(Class::General)
    }

    fn lower_function(&mut self, function: &Function) -> Compilation<()> {
        // Object preallocation: parameters, then locals in reverse declaration
        // order, then the coalescing window opens.
        let mut incoming = Vec::new();
        for (index, parameter) in function.parameters.iter().enumerate() {
            if !is_general_word(parameter.parameter_type) {
                return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
            }
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            if register > LAST_GENERAL_ARGUMENT {
                return Err(unsupported("stack-passed parameters"));
            }
            let virtual_register = self.temporary();
            self.variables.insert(
                parameter.name.clone(),
                Variable { register: virtual_register, ty: parameter.parameter_type },
            );
            incoming.push((virtual_register, register));
        }
        for local in function.locals.iter().rev() {
            if local.is_static || local.is_volatile || local.array_length.is_some() {
                return Err(unsupported("static, volatile, or array locals"));
            }
            if !is_general_word(local.declared_type) {
                return Err(unsupported(format!("local type {:?}", local.declared_type)));
            }
            let register = self.temporary();
            self.variables
                .insert(local.name.clone(), Variable { register, ty: local.declared_type });
        }
        self.pcode.begin_coalesce_window();

        for (virtual_register, register) in incoming {
            self.emit_plain(Instruction::Or { a: virtual_register, s: register, b: register });
        }
        for local in &function.locals {
            if let Some(initializer) = &local.initializer {
                self.assign(&local.name, initializer)?;
            }
        }
        for statement in &function.statements {
            self.statement(statement)?;
        }
        if let Some(value) = &function.return_expression {
            self.return_value(value)?;
        }
        // The exit block: the epilogue is generated here after coloring.
        self.pcode.blocks.last_mut().expect("entry block").successors.push(1);
        self.pcode.blocks.push(Block { weight: 1, ..Block::default() });
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Assign { name, value } => self.assign(name, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.call(name, arguments).map(|_| ())
            }
            Statement::Store { target, value } => self.store(target, value),
            other => Err(unsupported(format!("statement {:?}", std::mem::discriminant(other)))),
        }
    }

    fn return_value(&mut self, value: &Expression) -> Compilation<()> {
        let (register, _) = self.expression(value)?;
        let _ = self.return_type;
        self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
        Ok(())
    }

    fn assign(&mut self, name: &str, value: &Expression) -> Compilation<()> {
        let (source, _) = self.expression(value)?;
        let Some(variable) = self.variables.get(name) else {
            return Err(unsupported(format!("assignment to non-local '{name}'")));
        };
        let destination = variable.register;
        self.emit_plain(Instruction::Or { a: destination, s: source, b: source });
        Ok(())
    }

    /// Evaluate a word-sized integer/pointer expression into a register.
    fn expression(&mut self, expression: &Expression) -> Compilation<(u32, Type)> {
        match expression {
            Expression::IntegerLiteral(value) => {
                let register = self.temporary();
                self.load_constant(register, *value)?;
                Ok((register, Type::Int))
            }
            Expression::Variable(name) => {
                if let Some(variable) = self.variables.get(name) {
                    return Ok((variable.register, variable.ty));
                }
                if let Some(global) = self.context.globals.get(name).copied() {
                    return self.load_global(name, global);
                }
                Err(unsupported(format!("unknown variable '{name}'")))
            }
            Expression::Binary { operator, left, right } => self.binary(*operator, left, right),
            Expression::Unary { operator, operand } => {
                let (source, ty) = self.expression(operand)?;
                let destination = self.temporary();
                match operator {
                    UnaryOperator::Negate => {
                        self.emit_plain(Instruction::Negate { d: destination, a: source })
                    }
                    UnaryOperator::BitNot => {
                        self.emit_plain(Instruction::Nor { a: destination, s: source, b: source })
                    }
                    UnaryOperator::LogicalNot => return Err(unsupported("logical not")),
                }
                Ok((destination, promote(ty)))
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let (base, _) = self.expression(base)?;
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                self.load(*member_type, base, offset, None)
            }
            Expression::Dereference { pointer } => {
                let (base, ty) = self.expression(pointer)?;
                let Type::Pointer(pointee) = ty else {
                    return Err(unsupported("dereference of a non-scalar pointer"));
                };
                let loaded = pointee_type(pointee).ok_or_else(|| unsupported("pointee type"))?;
                self.load(loaded, base, 0, None)
            }
            Expression::Call { name, arguments } => {
                let ty = self.context.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_general_word(ty) {
                    return Err(unsupported("non-integer call result"));
                }
                let register = self.call(name, arguments)?;
                Ok((register, ty))
            }
            Expression::Cast { target_type, operand } if is_general_word(*target_type) => {
                let (source, source_type) = self.expression(operand)?;
                self.convert(source, source_type, *target_type)
            }
            _ => Err(unsupported("expression form")),
        }
    }

    fn convert(&mut self, source: u32, from: Type, to: Type) -> Compilation<(u32, Type)> {
        let narrow = match to {
            Type::Char => Some(Instruction::ExtendSignByte { a: 0, s: source }),
            Type::Short => Some(Instruction::ExtendSignHalfword { a: 0, s: source }),
            Type::UnsignedChar => Some(Instruction::ClearLeftImmediate { a: 0, s: source, clear: 24 }),
            Type::UnsignedShort => Some(Instruction::ClearLeftImmediate { a: 0, s: source, clear: 16 }),
            _ => None,
        };
        if from == to || narrow.is_none() {
            return Ok((source, to));
        }
        let destination = self.temporary();
        let instruction = match narrow.expect("checked") {
            Instruction::ExtendSignByte { s, .. } => Instruction::ExtendSignByte { a: destination, s },
            Instruction::ExtendSignHalfword { s, .. } => Instruction::ExtendSignHalfword { a: destination, s },
            Instruction::ClearLeftImmediate { s, clear, .. } => {
                Instruction::ClearLeftImmediate { a: destination, s, clear }
            }
            other => other,
        };
        self.emit_plain(instruction);
        Ok((destination, to))
    }

    fn load_constant(&mut self, register: u32, value: i64) -> Compilation<()> {
        let value = i32::try_from(value)
            .or_else(|_| u32::try_from(value).map(|value| value as i32))
            .map_err(|_| unsupported("64-bit constant"))?;
        if let Ok(small) = i16::try_from(value) {
            self.emit_plain(Instruction::AddImmediate { d: register, a: 0, immediate: small });
            return Ok(());
        }
        let low = value as i16;
        let high = ((value - i32::from(low)) >> 16) as i16;
        self.emit_plain(Instruction::AddImmediateShifted { d: register, a: 0, immediate: high });
        if low != 0 {
            self.emit_based(Instruction::AddImmediate { d: register, a: register, immediate: low }, register);
        }
        Ok(())
    }

    fn load_instruction(ty: Type, d: u32, a: u32, offset: i16) -> Compilation<(Instruction, Option<Instruction>)> {
        Ok(match ty {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                (Instruction::LoadWord { d, a, offset }, None)
            }
            Type::Short => (Instruction::LoadHalfwordAlgebraic { d, a, offset }, None),
            Type::UnsignedShort => (Instruction::LoadHalfwordZero { d, a, offset }, None),
            Type::UnsignedChar => (Instruction::LoadByteZero { d, a, offset }, None),
            Type::Char => (
                Instruction::LoadByteZero { d, a, offset },
                Some(Instruction::ExtendSignByte { a: d, s: d }),
            ),
            other => return Err(unsupported(format!("load of {other:?}"))),
        })
    }

    fn load(
        &mut self,
        ty: Type,
        base: u32,
        offset: i16,
        relocation: Option<AttachedRelocation>,
    ) -> Compilation<(u32, Type)> {
        let destination = self.temporary();
        let (load, extend) = Self::load_instruction(ty, destination, base, offset)?;
        let mut instruction = PInstr::new(load);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        if let Some(extend) = extend {
            let extended = self.temporary();
            let extend = match extend {
                Instruction::ExtendSignByte { .. } => Instruction::ExtendSignByte { a: extended, s: destination },
                other => other,
            };
            self.emit_plain(extend);
            return Ok((extended, ty));
        }
        Ok((destination, ty))
    }

    fn load_global(&mut self, name: &str, global: GlobalInfo) -> Compilation<(u32, Type)> {
        if global.small_data {
            return self.load(
                global.ty,
                0,
                0,
                Some(AttachedRelocation {
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::External(name.to_owned()),
                }),
            );
        }
        let high = self.temporary();
        let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
        lis.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(name.to_owned()),
        });
        self.emit(lis);
        self.load(
            global.ty,
            high,
            0,
            Some(AttachedRelocation {
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External(name.to_owned()),
            }),
        )
    }

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<(u32, Type)> {
        let immediate = match right {
            Expression::IntegerLiteral(value) => i16::try_from(*value).ok(),
            _ => None,
        };
        let (a, left_type) = self.expression(left)?;
        let result_type = promote(left_type);
        match (operator, immediate) {
            (BinaryOperator::Add, Some(value)) => {
                let d = self.temporary();
                self.emit_based(Instruction::AddImmediate { d, a, immediate: value }, a);
                return Ok((d, result_type));
            }
            (BinaryOperator::Subtract, Some(value)) if value != i16::MIN => {
                let d = self.temporary();
                self.emit_based(Instruction::AddImmediate { d, a, immediate: -value }, a);
                return Ok((d, result_type));
            }
            (BinaryOperator::Multiply, Some(value)) => {
                let d = self.temporary();
                self.emit_plain(Instruction::MultiplyImmediate { d, a, immediate: value });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftLeft, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.temporary();
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift: shift as u8 });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftRight, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.temporary();
                let shift = shift as u8;
                self.emit_plain(if is_unsigned(result_type) {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: a, shift }
                } else {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: a, shift }
                });
                return Ok((d, result_type));
            }
            _ => {}
        }
        let (b, right_type) = self.expression(right)?;
        let result_type = if is_unsigned(result_type) || is_unsigned(promote(right_type)) {
            if matches!(result_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                result_type
            } else {
                Type::UnsignedInt
            }
        } else {
            result_type
        };
        let d = self.temporary();
        let instruction = match operator {
            BinaryOperator::Add => Instruction::Add { d, a, b },
            BinaryOperator::Subtract => Instruction::SubtractFrom { d, a: b, b: a },
            BinaryOperator::Multiply => Instruction::MultiplyLow { d, a, b },
            BinaryOperator::BitAnd => Instruction::And { a: d, s: a, b },
            BinaryOperator::BitOr => Instruction::Or { a: d, s: a, b },
            BinaryOperator::BitXor => Instruction::Xor { a: d, s: a, b },
            BinaryOperator::ShiftLeft => Instruction::ShiftLeftWord { a: d, s: a, b },
            BinaryOperator::ShiftRight if is_unsigned(result_type) => {
                Instruction::ShiftRightWord { a: d, s: a, b }
            }
            BinaryOperator::ShiftRight => Instruction::ShiftRightAlgebraicWord { a: d, s: a, b },
            other => return Err(unsupported(format!("binary operator {other:?}"))),
        };
        self.emit_plain(instruction);
        Ok((d, result_type))
    }

    fn store(&mut self, target: &Expression, value: &Expression) -> Compilation<()> {
        let (source, _) = self.expression(value)?;
        let (base, offset, ty, relocation) = match target {
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let (base, _) = self.expression(base)?;
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                (base, offset, *member_type, None)
            }
            Expression::Dereference { pointer } => {
                let (base, ty) = self.expression(pointer)?;
                let Type::Pointer(pointee) = ty else {
                    return Err(unsupported("store through a non-scalar pointer"));
                };
                (base, 0, pointee_type(pointee).ok_or_else(|| unsupported("pointee type"))?, None)
            }
            Expression::Variable(name) if self.context.globals.contains_key(name) => {
                let global = self.context.globals[name];
                if !global.small_data {
                    return Err(unsupported("absolute global store"));
                }
                (
                    0,
                    0,
                    global.ty,
                    Some(AttachedRelocation {
                        kind: RelocationKind::EmbSda21,
                        target: RelocationTarget::External(name.clone()),
                    }),
                )
            }
            _ => return Err(unsupported("store target")),
        };
        let store = match ty {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                Instruction::StoreWord { s: source, a: base, offset }
            }
            Type::Short | Type::UnsignedShort => Instruction::StoreHalfword { s: source, a: base, offset },
            Type::Char | Type::UnsignedChar => Instruction::StoreByte { s: source, a: base, offset },
            other => return Err(unsupported(format!("store of {other:?}"))),
        };
        let mut instruction = PInstr::new(store);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        Ok(())
    }

    fn call(&mut self, name: &str, arguments: &[Expression]) -> Compilation<u32> {
        if arguments.len() > (LAST_GENERAL_ARGUMENT - FIRST_GENERAL_ARGUMENT + 1) as usize {
            return Err(unsupported("stack-passed arguments"));
        }
        let mut values = Vec::new();
        for argument in arguments {
            let (register, ty) = self.expression(argument)?;
            if !is_general_word(ty) {
                return Err(unsupported("non-integer argument"));
            }
            values.push(register);
        }
        let mut argument_registers = Vec::new();
        for (index, value) in values.into_iter().enumerate() {
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            self.emit_plain(Instruction::Or { a: register, s: value, b: value });
            argument_registers.push(Register::general(register));
        }
        let mut call = PInstr::new(Instruction::BranchAndLink { target: name.to_owned() });
        call.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(name.to_owned()),
        });
        call.implicit_uses = argument_registers;
        call.implicit_defs = VOLATILE_GENERAL.iter().map(|&r| Register::general(r)).collect();
        call.implicit_defs.extend((0..14).map(Register::float));
        self.emit(call);
        self.makes_calls = true;
        let result = self.temporary();
        self.emit_plain(Instruction::Or { a: result, s: 3, b: 3 });
        Ok(result)
    }
}
