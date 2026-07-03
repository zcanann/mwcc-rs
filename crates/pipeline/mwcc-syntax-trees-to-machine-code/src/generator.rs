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
    /// For a struct pointer, the struct's byte size — the stride for scaled pointer
    /// arithmetic (`p + n`, `p++`). `None` for scalar pointers (which scale by the
    /// `pointee` size) and non-pointers.
    pub(crate) stride: Option<u16>,
}

/// The k_cos else-branch composition payload (set by the punned arm,
/// consumed by the dual arm's else phase).
#[derive(Clone)]
pub(crate) struct FloatElseComposition {
    /// The inner compare's lis half (`lis r0, high; cmpw ix, r0`).
    pub(crate) compare_high: i16,
    /// The skip branch to the diamond's else arm (ble for Greater).
    pub(crate) skip_options: u8,
    pub(crate) skip_bit: u8,
    /// The preserved ix register (the compare's A side, the addis source).
    pub(crate) ix_register: u8,
    /// The freed raw-word register the addis result lands in (r3).
    pub(crate) addis_target: u8,
    /// The diamond's then-arm literal (qx = 0.28125).
    pub(crate) then_bits: u64,
    /// The addis immediate (ix - C, C a lis-able constant; shift = -C>>16).
    pub(crate) addis_shift: i16,
    /// The diamond local's name + frame offset (qx @ 16).
    pub(crate) qx_name: String,
    pub(crate) qx_offset: i16,
    /// The else-only fold-away locals (hz, a) with their initializers.
    pub(crate) else_locals: Vec<mwcc_syntax_trees::LocalDeclaration>,
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
    /// Whether this slot is a local array (`int buf[N];`): in value position the
    /// name decays to the slot's *address* (`addi d,r1,offset`) rather than a load.
    pub(crate) is_array: bool,
}

pub(crate) struct Generator {
    pub(crate) output: MachineFunction,
    /// Branch labels awaiting resolution — the multi-block emission substrate.
    /// Resolved into `output.instructions` once body emission completes.
    pub(crate) labels: mwcc_vreg::Labels,
    pub(crate) locations: HashMap<String, Location>,
    /// File-scope globals by name; a reference to one loads from the small-data
    /// area (an `R_PPC_EMB_SDA21` relocation off r13, the `0(r0)` placeholder).
    pub(crate) globals: HashMap<String, Type>,
    /// Total byte size of each file-scope *array* global, by name. Drives the
    /// per-symbol address mode when subscripting it: a small array (≤ 8 bytes,
    /// `.sdata`) materializes via SDA21, a large one (`.data`/`.bss`) via ADDR16.
    pub(crate) global_array_sizes: HashMap<String, u32>,
    /// Registers holding live values that must not be clobbered while a sibling
    /// sub-expression is being evaluated. The allocator draws temporaries from
    /// the registers outside this set.
    pub(crate) reserved: HashSet<u8>,
    /// Stack frame size in bytes (0 = leaf function, no frame). Set when an
    /// operation needs scratch stack space (e.g. an int/float conversion).
    pub(crate) frame_size: i16,
    /// The float DAG tail reloads x from this frame offset (the fctiwz
    /// punned-guard composition): x's references become a frame lfd node
    /// and f1 frees for the chain.
    pub(crate) float_reload_x: Option<i16>,
    /// Extra float bindings for a DAG tail: shared dual-tail locals already
    /// materialized in registers (name -> FPR).
    pub(crate) float_pseudo_params: Vec<(String, u8)>,
    /// The k_cos-family BIG-constant dual compare: (lis high, addi low, the
    /// preserved ix register). The in-frame dual weaves `lis r3,high;
    /// addi r0,r3,low` right after the x reload and `cmpw ix,r0` after the
    /// fourth shared load (measured at chain depths 3 and 4).
    pub(crate) float_dual_compare: Option<(i16, i16, u8)>,
    /// A double local defined by a CONDITIONAL diamond ahead of the float
    /// tail (k_cos's qx): the tail's DAG allocates it as a window-top tier
    /// value (a PHANTOM node, value id 8, emitting nothing) and reports the
    /// assigned register back so the diamond arms load into it.
    pub(crate) float_phantom_local: Option<String>,
    pub(crate) float_phantom_register: Option<u8>,
    /// A double local resident in a FRAME slot (the punned qx diamond): the
    /// tail's DAG reads it as a FrameLoad node (value id 7).
    pub(crate) float_frame_local: Option<(String, i16)>,
    /// The k_cos ELSE composition: the dual's else branch opens with a
    /// frame-punned diamond (an inner lis/cmpw against the preserved ix)
    /// and its tail reads x RE-reloaded plus the diamond local from the
    /// frame, with fold-away else-only locals.
    pub(crate) float_else_composition: Option<FloatElseComposition>,
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
    /// Parameter types of each callable name, so a call places each argument in the
    /// register its parameter requires (a float parameter takes f1.., an integer
    /// takes r3..) and a type mismatch is detected rather than silently mis-passed.
    pub(crate) call_parameter_types: HashMap<String, Vec<Type>>,
    /// A global just stored, with the register holding the stored value and the
    /// instruction count at the moment of the store. A subsequent read of the
    /// global reuses that register instead of reloading — but only while no
    /// instruction has been emitted since (so the value is provably still there).
    /// This reproduces mwcc keeping a just-written global live in its register.
    pub(crate) stored_globals: HashMap<String, (u8, usize)>,
    /// Non-empty once a constant-address access in this function has materialized a
    /// base register (`lis hi`). mwcc handles multiple such accesses by allocating
    /// ALL the bases up front, chosen by look-ahead over every value and (for the
    /// same high half) reusing one `lis` across the run — keystone-level register
    /// allocation. So only the FIRST high-half base is emitted; a second const-address
    /// access defers rather than emitting a fresh, mis-scheduled `lis` (a correct
    /// value, but the wrong bytes). Accesses with a zero high half (r0=0 base, no
    /// `lis`) never record here and are unaffected.
    pub(crate) const_address_bases: HashSet<i16>,
    /// Address-taken variables and their stack-frame slots. A name here is
    /// frame-resident: `&v` and type-punned accesses read/write its slot.
    pub(crate) frame_slots: HashMap<String, FrameSlot>,
    /// Slot offsets STORED THROUGH during emission (a pun store, a writeback).
    /// A spilled float parameter reloads at its return only when its slot is
    /// here — otherwise the value is still live in the incoming register
    /// (measured: `x *= c` reloads, an untouched x does not).
    pub(crate) written_slots: HashSet<i16>,
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
    /// Emit the saved-LR reload BEFORE the callee-saved GPR reloads in the epilogue. mwcc
    /// orders it this way for a callee-saved STORE sink (`foo(); gi = a;` — the saved value
    /// is stored after the call, then `lwz r0,20; lwz r31,12; mtlr`), as opposed to the
    /// return sink where the LR-reload hoist issues it right after the last call.
    pub(crate) epilogue_lr_first: bool,
    /// Emit the saved-LR reload BEFORE *all* callee-saved GPR reloads (highest-first), for a
    /// multi-pointer store sink: `void s(int*a,int*b){ *a=g(); *b=h(); }` saves both pointers
    /// (r31,r30), runs the calls, then `lwz r0,20; lwz r31,12; lwz r30,8; mtlr`. Distinct from
    /// `epilogue_lr_first`, whose two-GPR form interleaves the LR reload between the GPRs.
    pub(crate) epilogue_lr_before_gprs: bool,
    /// Set while evaluating a narrow-return expression whose result is truncated, so a
    /// narrow leaf operand is read raw (no leading sign/zero extension) — the final
    /// truncation makes the extension redundant. Only enabled for truncation-safe
    /// operators with leaf operands, never for div/mod/shift-right.
    pub(crate) narrow_truncation_context: bool,
    /// The current function's declared local names — a CALL through one of these that
    /// never got a register must defer (the fallback would emit a direct `bl <local>`,
    /// a relocation against the local's name).
    pub(crate) known_locals: std::collections::HashSet<String>,
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

    /// A fresh, unbound branch label. Branches emitted through
    /// [`Self::emit_branch_conditional_to`]/[`Self::emit_branch_to`] may target it
    /// before [`Self::bind_label`] pins where it lands; one resolve pass at the
    /// end of body emission writes every target.
    pub(crate) fn fresh_label(&mut self) -> mwcc_vreg::Label {
        self.labels.fresh()
    }

    /// Pin `label` to the next instruction to be emitted.
    pub(crate) fn bind_label(&mut self, label: mwcc_vreg::Label) {
        let at = self.output.instructions.len();
        self.labels.bind(label, at);
    }

    /// Emit a conditional branch to `label` (target written at resolution).
    pub(crate) fn emit_branch_conditional_to(&mut self, options: u8, condition_bit: u8, label: mwcc_vreg::Label) {
        self.labels.use_at(self.output.instructions.len(), label);
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
    }

    /// Emit an unconditional branch to `label` (target written at resolution).
    #[allow(dead_code)]
    pub(crate) fn emit_branch_to(&mut self, label: mwcc_vreg::Label) {
        self.labels.use_at(self.output.instructions.len(), label);
        self.output.instructions.push(Instruction::Branch { target: 0 });
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

    /// See through a redundant `(double)` cast of an already-`double` value — a
    /// semantic no-op mwcc emits nothing for (`(double)dbl_call()`, `(double)dbl_x`).
    /// Peels every such layer, returning the innermost double operand. A `(float)`
    /// cast (a real narrowing) and a `(double)` of a non-double value are left intact.
    pub(crate) fn peel_redundant_double_cast<'a>(&self, mut expression: &'a Expression) -> &'a Expression {
        while let Expression::Cast { target_type: Type::Double, operand } = expression {
            if self.is_double_value(operand) {
                expression = operand;
            } else {
                break;
            }
        }
        expression
    }

    /// Whether this expression yields a floating-point value — a float-register leaf,
    /// a float file-scope global, or a float-typed struct member — so a comparison on
    /// it routes to the FPU compare (`fcmpo`/`fcmpu`) path rather than the integer one.
    pub(crate) fn is_float_operand(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(name) => {
                self.locations.get(name.as_str()).is_some_and(|location| location.class == ValueClass::Float)
                    || (!self.locations.contains_key(name.as_str())
                        && matches!(self.globals.get(name.as_str()), Some(Type::Float | Type::Double)))
            }
            Expression::Member { member_type, .. } => matches!(member_type, Type::Float | Type::Double),
            _ => false,
        }
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
            // A string literal is an address — an unsigned pointer value.
            Expression::StringLiteral(_) => Ok(false),
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
            // A comma operator yields its right operand.
            Expression::Comma { right, .. } => self.signedness_of(right),
            // A call's signedness is its declared return type's; an unknown callee
            // defaults to a signed int.
            Expression::Call { name, .. } => Ok(self
                .call_return_types
                .get(name)
                .map_or(true, |return_type| self.signed_of(*return_type))),
        }
    }

    /// The pointee type of a pointer leaf variable.
    pub(crate) fn pointee_of(&self, pointer: &Expression) -> Compilation<mwcc_syntax_trees::Pointee> {
        // `*(p + i)` / `p[i]` of a pointer-plus-index dereferences the pointer operand's
        // pointee (the integer offset does not change the element type). `+` commutes. This
        // gives `signedness_of(*(p + i))` the element signedness, so `is_signed_byte_load`
        // recognizes a narrow `*(char* p + i)`.
        if let Expression::Binary { operator: mwcc_syntax_trees::BinaryOperator::Add, left, right } = pointer {
            if let Ok(pointee) = self.pointee_of(left) {
                return Ok(pointee);
            }
            if let Ok(pointee) = self.pointee_of(right) {
                return Ok(pointee);
            }
        }
        // `*(T*)p` — a pointer cast reinterprets the address: the pointee is the cast's
        // target regardless of what `p` is (mirrors `resolve_pointer`, so value tracking
        // classifies a punned `*(int*)&x` the same way the direct evaluator emits it).
        if let Expression::Cast { target_type: Type::Pointer(pointee), .. } = pointer {
            return Ok(*pointee);
        }
        let name = leaf_name(pointer).ok_or_else(|| Diagnostic::error("pointer access needs a pointer variable (roadmap)"))?;
        if let Some(pointee) = self.locations.get(name).and_then(|location| location.pointee) {
            return Ok(pointee);
        }
        // A global ARRAY's name classifies by its element type (`map[i]` over
        // `unsigned char map[256]` reads a byte) — the subscript emitters carry
        // the addressing; this is only the width/signedness classification.
        if self.global_array_sizes.contains_key(name) {
            if let Some(pointee) = self.globals.get(name).copied().and_then(crate::expressions::pointee_of_type) {
                return Ok(pointee);
            }
        }
        Err(Diagnostic::error(format!("'{name}' is not a pointer")))
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
            // The `addi` that folds in the low half reads `destination` as a base, but
            // `addi rA=r0` denotes the literal 0, not r0 — so materializing into r0
            // (the scratch) needs the `lis` in a separate register: `lis t,hi; addi
            // r0,t,lo` (mwcc colors `t` the lowest free GPR). Any other destination
            // folds in place.
            if destination == GENERAL_SCRATCH && low != 0 {
                let temp = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::load_immediate_shifted(temp, high_adjusted));
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: temp, immediate: low });
            } else {
                self.output.instructions.push(Instruction::load_immediate_shifted(destination, high_adjusted));
                // A constant whose low half is zero (`0x10000`, `0x80000000`) is a
                // single `lis`; mwcc omits the redundant `addi d,d,0`.
                if low != 0 {
                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: low });
                }
            }
        }
    }
}
