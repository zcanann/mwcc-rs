//! The `Generator` — codegen state — plus its small accessors. The emit
//! logic lives in the sibling theme modules, each a further `impl Generator`.

use std::collections::{HashMap, HashSet};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{Expression, Pointee, Type, UnaryOperator};
use mwcc_versions::CompilerBuild;
use crate::analysis::*;

/// The scratch register mwcc spills the secondary operand of a binary node into.
pub(crate) const GENERAL_SCRATCH: u8 = 0; // r0
pub(crate) const FLOAT_SCRATCH: u8 = 0; // f0

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ValueClass {
    General,
    Float,
}

pub(crate) struct Location {
    pub(crate) class: ValueClass,
    pub(crate) register: u8,
    pub(crate) signed: bool,
    /// Integer width in bits (8/16/32); narrow values are extended when read.
    pub(crate) width: u8,
    /// For a pointer value, what it points to (so `*p` picks the right load).
    pub(crate) pointee: Option<Pointee>,
}

pub(crate) struct Generator {
    pub(crate) output: MachineFunction,
    pub(crate) locations: HashMap<String, Location>,
    /// Registers holding live values that must not be clobbered while a sibling
    /// sub-expression is being evaluated. The allocator draws temporaries from
    /// the registers outside this set.
    pub(crate) reserved: HashSet<u8>,
    /// Stack frame size in bytes (0 = leaf function, no frame). Set when an
    /// operation needs scratch stack space (e.g. an int/float conversion).
    pub(crate) frame_size: i16,
    /// The build we are reproducing. Its only codegen-affecting knob today is
    /// the default signedness of plain `char` (see [`Generator::signed_of`]).
    pub(crate) build: CompilerBuild,
}

pub(crate) fn class_of(declared: Type) -> Compilation<ValueClass> {
    match declared {
        Type::Float => Ok(ValueClass::Float),
        Type::Void => Err(Diagnostic::error("a value cannot have type void")),
        _ => Ok(ValueClass::General),
    }
}

impl Generator {
    /// Signedness of a source-level type for the target build. Plain `char` is
    /// the one type whose signedness is build-dependent (unsigned in GC/1.3
    /// build 53, signed from build 81 on); every other type is fixed. Routing
    /// all type-signedness queries through here makes the whole cascade — read
    /// extension, `>>`/`/`/`%` strength reduction, comparison folding, and the
    /// int->float bias — follow the build with no scattered version checks.
    pub(crate) fn signed_of(&self, declared: Type) -> bool {
        match declared {
            Type::Char => self.build.profile.char_is_signed(),
            other => other.is_signed(),
        }
    }

    /// Whether `expression` is a float-valued leaf.
    pub(crate) fn is_float_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name) if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::Float))
    }

    pub(crate) fn lookup_general(&self, name: &str) -> Option<u8> {
        self.locations.get(name).filter(|location| location.class == ValueClass::General).map(|location| location.register)
    }

    /// Whether `expression` is a narrow (sub-32-bit) integer variable. Such an
    /// operand needs width extension before use, and a few consumers (left shift
    /// and pow2 multiply) fuse extension and shift into a single `rlwinm` on the
    /// builds that treat `char` as unsigned — a peephole we do not model yet, so
    /// those callers defer narrow operands rather than emit non-matching bytes.
    pub(crate) fn is_narrow_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name)
            if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::General && l.width < 32))
    }

    /// Whether the value of `expression` is signed (for selecting `>>`). The
    /// usual arithmetic conversions make a binary expression unsigned if either
    /// operand is unsigned.
    pub(crate) fn signedness_of(&self, expression: &Expression) -> Compilation<bool> {
        match expression {
            Expression::IntegerLiteral(_) => Ok(true),
            Expression::FloatLiteral(_) => Ok(true),
            Expression::Variable(name) => {
                Ok(self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?.signed)
            }
            Expression::Binary { operator, left, right } => {
                if is_comparison(*operator) {
                    Ok(true) // a comparison yields an int (signed)
                } else {
                    Ok(self.signedness_of(left)? && self.signedness_of(right)?)
                }
            }
            Expression::Unary { operator, operand } => match operator {
                UnaryOperator::LogicalNot => Ok(true),
                _ => self.signedness_of(operand),
            },
            Expression::Conditional { when_true, when_false, .. } => {
                Ok(self.signedness_of(when_true)? && self.signedness_of(when_false)?)
            }
            Expression::Cast { target_type, .. } => Ok(self.signed_of(*target_type)),
            // `*p` has the signedness of the pointee.
            Expression::Dereference { pointer } => {
                let name = leaf_name(pointer).ok_or_else(|| Diagnostic::error("dereference needs a pointer variable (roadmap)"))?;
                let pointee = self
                    .locations
                    .get(name)
                    .and_then(|location| location.pointee)
                    .ok_or_else(|| Diagnostic::error(format!("'{name}' is not a pointer")))?;
                Ok(pointee.element().is_signed())
            }
        }
    }

    /// (register, width-bits, signed) for a general-register leaf variable.
    pub(crate) fn leaf_info(&self, expression: &Expression) -> Compilation<(u8, u8, bool)> {
        if let Expression::Variable(name) = expression {
            if let Some(location) = self.locations.get(name.as_str()) {
                if location.class == ValueClass::General {
                    return Ok((location.register, location.width, location.signed));
                }
            }
        }
        Err(Diagnostic::error("expected a general-register leaf"))
    }

    pub(crate) fn general_register_of(&self, name: &str) -> Compilation<u8> {
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        if location.class != ValueClass::General {
            return Err(Diagnostic::error(format!("'{name}' is not an integer")));
        }
        Ok(location.register)
    }

    pub(crate) fn float_register_of(&self, name: &str) -> Compilation<u8> {
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        if location.class != ValueClass::Float {
            return Err(Diagnostic::error(format!("'{name}' is not a float")));
        }
        Ok(location.register)
    }

    pub(crate) fn general_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.general_register_of(name),
            _ => Err(Diagnostic::error("v0: a leaf operand must be a variable (constants in trees: roadmap M3)")),
        }
    }

    pub(crate) fn float_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.float_register_of(name),
            _ => Err(Diagnostic::error("v0: a float leaf operand must be a variable")),
        }
    }

    /// Load a 32-bit integer constant the way mwcc does: `li`, or `lis` + `addi`
    /// with a high-adjusted upper half to absorb `addi`'s sign extension.
    pub(crate) fn load_integer_constant(&mut self, destination: u8, value: i64) {
        let value = value as i32;
        if (-0x8000..=0x7fff).contains(&value) {
            self.output.instructions.push(Instruction::load_immediate(destination, value as i16));
        } else {
            let low = (value as u32 & 0xffff) as i16;
            let high_adjusted = ((value - low as i32) >> 16) as i16;
            self.output.instructions.push(Instruction::load_immediate_shifted(destination, high_adjusted));
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: low });
        }
    }
}
