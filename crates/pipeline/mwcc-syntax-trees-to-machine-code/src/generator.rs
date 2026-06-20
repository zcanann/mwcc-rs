//! The `Generator` — codegen state — plus its small accessors. The emit
//! logic lives in the sibling theme modules, each a further `impl Generator`.

use std::collections::{HashMap, HashSet};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget};
use mwcc_syntax_trees::{Expression, Pointee, Type, UnaryOperator};
use mwcc_versions::Behavior;
use mwcc_vreg::{Reg, RegisterConstraints};
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

/// A variable whose address is taken: it lives in a stack-frame slot rather than
/// a register. `&v` is `addi d, r1, offset`, and a type-punned access `*(t*)&v`
/// is a displacement load/store from `r1`.
#[derive(Clone, Copy)]
pub(crate) struct FrameSlot {
    /// Byte offset from the stack pointer (`r1`).
    pub(crate) offset: i16,
    /// Whether the variable is a float/double (spilled with `stfd`/`stfs`).
    pub(crate) class: ValueClass,
    /// Byte size of the variable (4 or 8).
    pub(crate) size: u8,
    /// The incoming argument register, if this is a spilled parameter.
    pub(crate) parameter_register: Option<u8>,
}

pub(crate) struct Generator {
    pub(crate) output: MachineFunction,
    pub(crate) locations: HashMap<String, Location>,
    /// File-scope globals by name; a reference to one loads from the small-data
    /// area (an `R_PPC_EMB_SDA21` relocation off r13, the `0(r0)` placeholder).
    pub(crate) globals: HashMap<String, Type>,
    /// Registers holding live values that must not be clobbered while a sibling
    /// sub-expression is being evaluated. The allocator draws temporaries from
    /// the registers outside this set.
    pub(crate) reserved: HashSet<u8>,
    /// Stack frame size in bytes (0 = leaf function, no frame). Set when an
    /// operation needs scratch stack space (e.g. an int/float conversion).
    pub(crate) frame_size: i16,
    /// The resolved codegen decisions for the configuration we are reproducing.
    /// Every version- or flag-varying choice is read from this one flat set,
    /// computed once from the build's profile and flags — never re-derived in
    /// instruction selection.
    pub(crate) behavior: Behavior,
    /// The target's register-allocation rules — the allocatable pools and scratch.
    /// The free-register helpers draw from here, so the pools have one authoritative
    /// home (shared with the future allocator) rather than literals in placement.
    pub(crate) constraints: RegisterConstraints,
    /// Whether the function makes a call: it then saves/restores the link register
    /// around a stack frame (the non-leaf prologue/epilogue).
    pub(crate) non_leaf: bool,
    /// The next virtual-register id to hand out. A migrated selection site asks
    /// for a fresh virtual instead of picking a physical register itself; the
    /// allocation pass assigns the physical home from liveness.
    pub(crate) next_virtual: u32,
    /// Per-virtual placement hints: registers the allocator must avoid for a
    /// given virtual id. Selection records these (e.g. "a comparison operand must
    /// avoid the destination") so the allocation pass reproduces mwcc's coalescing
    /// of result-path temporaries onto the destination register.
    pub(crate) register_avoid: HashMap<u32, Vec<u8>>,
    /// Return type of each callable name (prototypes + definitions), so a call's
    /// result type is known — e.g. `(float)cos(x)` rounds a double with `frsp`.
    pub(crate) call_return_types: HashMap<String, Type>,
    /// A global just stored, with the register holding the stored value and the
    /// instruction count at the moment of the store. A subsequent read of the
    /// global reuses that register instead of reloading — but only while no
    /// instruction has been emitted since (so the value is provably still there).
    /// This reproduces mwcc keeping a just-written global live in its register.
    pub(crate) stored_globals: HashMap<String, (u8, usize)>,
    /// Address-taken variables and their stack-frame slots. A name here is
    /// frame-resident: `&v` and type-punned accesses read/write its slot.
    pub(crate) frame_slots: HashMap<String, FrameSlot>,
    /// When set, a constant store value reuses the scratch register if it already
    /// holds that constant (`scratch_constant`). Enabled only by the
    /// constant-store-fill path, which guarantees nothing clobbers the scratch
    /// between stores, so the reuse is provably valid.
    pub(crate) reuse_scratch_constant: bool,
    /// The constant currently materialized in the scratch register, during a
    /// constant-store-fill run.
    pub(crate) scratch_constant: Option<i32>,
    /// Constants pre-materialized into specific registers ahead of a run of
    /// distinct-constant stores, so each store reuses its register rather than
    /// re-materializing (mwcc materializes both values up front, then stores).
    pub(crate) prematerialized_constants: Vec<(i32, u8)>,
    /// Callee-saved general registers this function uses (r31 first, descending) to
    /// hold values live across a call. They are saved high-to-low in the prologue
    /// and reloaded in the epilogue, and drive the unwind table's saved-GPR count.
    pub(crate) callee_saved: Vec<u8>,
}

pub(crate) fn class_of(declared: Type) -> Compilation<ValueClass> {
    match declared {
        Type::Float | Type::Double => Ok(ValueClass::Float),
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
            Type::Char => self.behavior.char_is_signed,
            other => other.is_signed(),
        }
    }

    /// A fresh general-purpose virtual register, as the u8 field value selection
    /// emits. The allocation pass resolves it to a physical register from liveness.
    pub(crate) fn fresh_virtual_general(&mut self) -> u8 {
        let register = Reg::general(self.next_virtual);
        self.next_virtual += 1;
        register.to_field()
    }

    /// A fresh floating-point virtual register. The allocator draws float homes
    /// from the FPR pool, kept distinct from the general pool by the class the
    /// machine description reports for each operand.
    pub(crate) fn fresh_virtual_float(&mut self) -> u8 {
        let register = Reg::float(self.next_virtual);
        self.next_virtual += 1;
        register.to_field()
    }

    /// A fresh general virtual register that the allocator must not place in any
    /// of `avoid` — a placement hint recorded for the allocation pass.
    pub(crate) fn fresh_virtual_general_avoiding(&mut self, avoid: Vec<u8>) -> u8 {
        let id = self.next_virtual;
        self.register_avoid.insert(id, avoid);
        let register = Reg::general(id);
        self.next_virtual += 1;
        register.to_field()
    }

    /// Whether `expression` is a float-valued leaf.
    pub(crate) fn is_float_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name) if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::Float))
    }

    /// Record a relocation against the instruction that is about to be pushed.
    pub(crate) fn record_relocation(&mut self, kind: RelocationKind, symbol: &str) {
        self.record_target(kind, RelocationTarget::External(symbol.to_string()));
    }

    /// Record a relocation with an explicit target (external symbol or pooled
    /// constant) against the instruction about to be pushed.
    pub(crate) fn record_target(&mut self, kind: RelocationKind, target: RelocationTarget) {
        let instruction_index = self.output.instructions.len();
        self.output.relocations.push(Relocation { instruction_index, kind, target });
    }

    /// Emit a load of a single-precision constant from `.sdata2`: `lfs fD, 0(r0)`
    /// (the zero placeholder the SDA21 relocation patches), pooling the value.
    pub(crate) fn load_float_constant(&mut self, destination: u8, value: f32) {
        let index = self.output.intern_constant(value.to_bits() as u64, 4);
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadFloatSingle { d: destination, a: 0, offset: 0 });
    }

    /// Emit a load of a double-precision constant from `.sdata2`: `lfd fD, 0(r0)`
    /// with the SDA21 relocation the pooled value needs.
    pub(crate) fn load_double_constant(&mut self, destination: u8, bits: u64) {
        let index = self.output.intern_constant(bits, 8);
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(index));
        self.output.instructions.push(Instruction::LoadFloatDouble { d: destination, a: 0, offset: 0 });
    }

    /// Load a float-literal operand, choosing 8-byte `lfd` in a double context and
    /// 4-byte `lfs` (the value rounded to single) otherwise.
    pub(crate) fn load_float_literal(&mut self, destination: u8, value: f64, double: bool) {
        if double {
            self.load_double_constant(destination, value.to_bits());
        } else {
            self.load_float_constant(destination, value as f32);
        }
    }

    pub(crate) fn lookup_general(&self, name: &str) -> Option<u8> {
        self.locations.get(name).filter(|location| location.class == ValueClass::General).map(|location| location.register)
    }

    /// The register of a full-width, non-pointer integer leaf variable — the
    /// operand shape that participates in mwcc's additive-chain reassociation.
    /// Narrow leaves (which need width extension) and pointers (scaled
    /// arithmetic) return `None`.
    pub(crate) fn plain_integer_leaf_register(&self, expression: &Expression) -> Option<u8> {
        let name = leaf_name(expression)?;
        let location = self.locations.get(name)?;
        (location.class == ValueClass::General && location.width == 32 && location.pointee.is_none())
            .then_some(location.register)
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
                if let Some(location) = self.locations.get(name) {
                    Ok(location.signed)
                } else if let Some(global_type) = self.globals.get(name) {
                    Ok(global_type.is_signed())
                } else {
                    Err(Diagnostic::error(format!("unknown variable '{name}'")))
                }
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
            // `*p` and `p[i]` have the signedness of the pointee.
            Expression::Dereference { pointer } => Ok(self.pointee_of(pointer)?.element().is_signed()),
            Expression::Index { base, .. } => Ok(self.pointee_of(base)?.element().is_signed()),
            // `p->field` has the signedness of the member type.
            Expression::Member { member_type, .. } => Ok(self.signed_of(*member_type)),
            // An array member's address is an unsigned pointer.
            Expression::MemberAddress { .. } => Ok(false),
            // The address of an lvalue is an unsigned pointer.
            Expression::AddressOf { .. } => Ok(false),
            // An assignment yields the stored value.
            Expression::Assign { value, .. } => self.signedness_of(value),
            // A call returns an int by default (we have no prototype types yet).
            Expression::Call { .. } => Ok(true),
        }
    }

    /// The pointee type of a pointer leaf variable.
    pub(crate) fn pointee_of(&self, pointer: &Expression) -> Compilation<mwcc_syntax_trees::Pointee> {
        let name = leaf_name(pointer).ok_or_else(|| Diagnostic::error("pointer access needs a pointer variable (roadmap)"))?;
        self.locations
            .get(name)
            .and_then(|location| location.pointee)
            .ok_or_else(|| Diagnostic::error(format!("'{name}' is not a pointer")))
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
            // A constant whose low half is zero (`0x10000`, `0x80000000`) is a
            // single `lis`; mwcc omits the redundant `addi d,d,0`.
            if low != 0 {
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: low });
            }
        }
    }
}
