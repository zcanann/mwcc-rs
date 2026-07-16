//! Function definitions and the declarations that make up their bodies.

use crate::expression::Expression;
use crate::types::Type;

/// A function parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub parameter_type: Type,
    pub name: String,
}

/// A local variable declaration: `type name [= expression];`. The initializer is
/// `None` for an uninitialized local (`int x;`), whose value comes from a later
/// assignment.
#[derive(Debug, Clone)]
pub struct LocalDeclaration {
    pub declared_type: Type,
    pub name: String,
    pub initializer: Option<Expression>,
    /// Declared array length `[N]` for a local array (`int buf[N];`), whose storage
    /// is a frame slot of `N * sizeof(element)` bytes; `None` for a scalar. The
    /// `declared_type` is the element type.
    pub array_length: Option<u16>,
    /// A `static` (function-local) variable: it has STATIC storage (an anonymous
    /// `<name>$N` object in `.sdata`/`.sbss`, persisting across calls) rather than a
    /// frame slot, so it is codegen'd like a file-scope global, not a local. The
    /// `register`/`auto` hints do not set this — they are ordinary automatic locals.
    pub is_static: bool,
    /// A static local's constant byte image (a brace-initialized array or a
    /// scalar literal); `None` for a zero-initialized or automatic local.
    pub data_bytes: Option<Vec<u8>>,
    /// Whether the static local was declared `const` (routes .sdata2/.rodata).
    pub is_const: bool,
}

/// A guarded early return: `if (condition) return value;`.
#[derive(Debug, Clone)]
pub struct GuardedReturn {
    pub condition: Expression,
    pub value: Expression,
}

/// One arm of a `switch`: a case value and the value it returns. The subset
/// handles switches whose every case is `case V: return E;`.
#[derive(Debug, Clone)]
pub struct SwitchArm {
    pub value: i64,
    pub body: ArmBody,
    /// True when the arm's body ends WITHOUT a `break;` or `return` — control
    /// falls through into the next arm (an empty body is a shared label like
    /// `case 'd': case 'i':`). Recorded for AST-hash fidelity; the general
    /// switch lowering only accepts non-fallthrough shapes.
    pub falls_through: bool,
}

/// A switch arm's payload: the common `return <expr>;` form, or a general
/// statement body ending at its `break`/return (the fminmaxdim/wchar_io
/// class — mwcc BRANCHES these; lowering to a ternary is byte-different).
#[derive(Debug, Clone)]
pub enum ArmBody {
    Return(Expression),
    Statements(Vec<Statement>),
}

impl SwitchArm {
    /// The return expression for a plain `case V: return E;` arm.
    pub fn result(&self) -> Option<&Expression> {
        self.body.return_expression()
    }
}

impl ArmBody {
    /// The return expression for the plain `return E;` body form.
    pub fn return_expression(&self) -> Option<&Expression> {
        match self {
            ArmBody::Return(expression) => Some(expression),
            ArmBody::Statements(_) => None,
        }
    }
}

/// A body statement (beyond declarations, guards, and the return).
#[derive(Debug, Clone)]
pub enum Statement {
    /// `*pointer = value;` or `base[index] = value;` — a store to memory. The
    /// target is a `Dereference` or `Index` expression.
    Store { target: Expression, value: Expression },
    /// `local = value;` — reassignment of a local variable (value-tracked, not a
    /// memory store).
    Assign { name: String, value: Expression },
    /// A bare expression evaluated for its side effects, e.g. `g();`.
    Expression(Expression),
    /// `if (condition) { then_body } [else { else_body }]` — a conditional block.
    If { condition: Expression, then_body: Vec<Statement>, else_body: Vec<Statement> },
    /// `return [value];` — an early return from within the body (as opposed to the
    /// function's trailing `return_expression`). `None` for `return;` in a void
    /// function.
    Return(Option<Expression>),
    /// `switch (scrutinee) { case V: return E; ... default: return D; }` — a
    /// terminal multi-way return (each arm and the default return a value).
    Switch { scrutinee: Expression, arms: Vec<SwitchArm>, default: Option<ArmBody> },
    /// `break;` — exit the innermost enclosing loop or switch. (A switch ARM's
    /// own terminating `break` is represented by `SwitchArm.falls_through`,
    /// not a trailing `Break`; this variant is a break in a NESTED position,
    /// e.g. inside an if-body within a loop.)
    Break,
    /// `continue;` — jump to the innermost enclosing loop's next iteration.
    Continue,
    /// `goto label;` — an unconditional jump to a named label.
    Goto(String),
    /// `label:` — a goto target, kept at its statement position.
    Label(String),
    /// A loop: `while (c) { body }`, `do { body } while (c);`, or
    /// `for (init; c; step) { body }`. `initializer`/`step` are the for-clause
    /// expressions (evaluated for effect); a `None` `condition` is an always-true
    /// loop (`for (;;)`).
    Loop {
        kind: LoopKind,
        initializer: Option<Expression>,
        condition: Option<Expression>,
        step: Option<Expression>,
        body: Vec<Statement>,
    },
}

/// Which loop form a [`Statement::Loop`] is — the condition is tested before the
/// body (`While`/`For`) or after it (`DoWhile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopKind {
    While,
    DoWhile,
    For,
}

/// A file-scope global variable, e.g. `int g;`, `extern int g;`, or `static int
/// g;`. A non-`extern` declaration is a (tentative) definition that the object
/// places in a data section; an `extern` one is just a reference to a symbol
/// defined elsewhere. `array_length` is set for `type g[N];`.
#[derive(Debug, Clone)]
pub struct GlobalDeclaration {
    pub declared_type: Type,
    pub name: String,
    pub is_extern: bool,
    pub is_static: bool,
    /// A weak object symbol (an inline function's emitted static local).
    pub is_weak: bool,
    /// How many NON-STATIC functions were defined before this declaration —
    /// mwcc's symbol table interleaves defined data with function symbols by
    /// source position (static functions' LOCAL symbols precede the data run,
    /// so only global functions shift an object's slot).
    pub non_static_functions_before: usize,
    /// Count of ALL function definitions (static included) before this
    /// declaration — the initialized-global symbol interleave keys on this
    /// (measured: ansi_fp's lbl table, declared between static functions,
    /// emits at its source position, not up front).
    pub functions_before: usize,
    /// Declared array length `[N]`; `Some` for an array (an empty `[]` infers it
    /// from the initializer), `None` for a scalar.
    pub array_length: Option<u16>,
    /// The constant initializer's element values, in order (a scalar is one
    /// element, an aggregate `{a, b, ...}` is several). `Some` with any non-zero
    /// value places the global in `.sdata` (initialized data); `None` or all-zero
    /// leaves it in `.sbss` (zero-initialized).
    pub initializer: Option<Vec<i64>>,
    /// Whether the declaration carried a leading `const`. A const file-scope global
    /// lands in a *read-only* section: `.sdata2` (small, ≤ 8 bytes) or `.rodata`
    /// (larger), rather than the writable `.sdata`/`.sbss`.
    pub is_const: bool,
    /// For a pointer global initialized with addresses (`int *p = &g;`, a string
    /// `char *s = "…"`, or a `{…}` table of them), each element's target. `None`
    /// overall = not a pointer/address initializer.
    pub address_initializer: Option<Vec<PointerElement>>,
    /// A struct value/array initializer pre-serialized to its raw object bytes — each
    /// field written at its own offset and width (so `char`/`short`/`double` and
    /// nested-struct fields, and inter-field padding, are exact). The driver emits
    /// these directly, bypassing the word-stride `initializer` path. `None` for a
    /// non-struct global.
    pub data_bytes: Option<Vec<u8>>,
    /// `R_PPC_ADDR32` relocations the data image carries: (byte offset, target
    /// symbol, addend) — a function pointer or a self-referential address in a
    /// struct-array initializer (ansi_files' FILE table).
    pub data_relocations: Vec<(u32, String, i32)>,
    /// An explicit output section from `__declspec(section "…")` — overrides the
    /// default `.sdata`/`.sdata2`/`.sbss` placement (e.g. `.dtors` for a global
    /// destructor-chain reference). `None` uses the default rules.
    pub section: Option<String>,
    /// An explicit alignment from `__attribute__((aligned(n)))` on the declarator
    /// (dolphin's `ATTRIBUTE_ALIGN(n)` on a DMA buffer). The lowering folds it into
    /// the object's alignment (`max` with the natural/array minimum). `None` = none.
    pub attribute_alignment: Option<u16>,
}

/// One element of a pointer global's initializer.
#[derive(Debug, Clone)]
pub enum PointerElement {
    /// `&name` or a bare function name — an `ADDR32` relocation to that symbol.
    Symbol(String),
    /// A string literal — its bytes (plus a NUL) are pooled as an anonymous
    /// read-only object and the pointer relocates to it.
    Str(Vec<u8>),
    /// A null pointer (`0`).
    Null,
    /// A 4-byte scalar slot — an integer field of a struct-array element (e.g. the
    /// `id` in `{ "name", id }`). Written as literal bytes, no relocation.
    Scalar(i64),
}

/// A translation unit: file-scope globals (and skipped prototypes) interleaved
/// with one or more function definitions, in source order.
#[derive(Debug, Clone)]
pub struct TranslationUnit {
    pub globals: Vec<GlobalDeclaration>,
    pub functions: Vec<Function>,
    /// Function prototypes (`type name(params);`) by name, return type, and
    /// parameter types, so a call to an externally-defined function knows its
    /// result type (e.g. a `double`-returning math routine) and its parameter
    /// types (so an argument's int<->float register placement is correct).
    pub prototypes: Vec<(String, Type, Vec<Type>)>,
    /// Skipped `inline` function definitions: each advanced mwcc's `@N` counter
    /// by 3 (compiled then dropped), so the writer pre-bumps the numbering.
    pub skipped_inline_functions: usize,
    /// Names of VARIADIC function definitions in this unit. Kept OUTSIDE
    /// `Function` so capture AST hashes stay stable; the general lowering
    /// defers these (the variadic-register-save prologue is capture-only).
    pub variadic_definitions: std::collections::HashSet<String>,
    /// Per static-local NAME, the skipped-inline bump total at its declaration
    /// point (the parser's positional sample) — statics number off the
    /// anonymous counter as of that position, not the owner's whole pre-bump.
    pub static_local_prebumps: std::collections::HashMap<String, usize>,
    /// Materialized static-inline functions with NO prior prototype (implicit
    /// declaration): calls bind the surviving UND ghost; the local FUNC symbol
    /// trails its own static locals.
    pub implicitly_materialized: Vec<String>,
    /// PLAIN-inline materializations — weak FUNC symbols carrying the
    /// weak-OBJECT 0x0d comment flag (not declspec-weak's 0x0e).
    pub weak_materialized: Vec<String>,
    /// The skipped inline functions' NAMES: a body that calls one must defer
    /// at codegen (mwcc inlines the body; a `bl` to the undefined local would
    /// be wrong bytes) — checked AFTER the exact-match templates get a claim.
    pub skipped_inline_names: std::collections::HashSet<String>,
    /// Functions defined under `#pragma defer_codegen on`, in definition order.
    /// mwcc code-generates these LAST, in REVERSE definition order (measured:
    /// melee mem_funcs — its whole .text is reversed). The unit assembly
    /// reorders `functions` accordingly before the writer runs.
    pub deferred_function_names: Vec<String>,
    /// Names of `static inline` functions whose body contains an inline `asm {}`
    /// block, in declaration order. mwcc keeps each as a deferred function and
    /// emits a local *undefined* symbol for it even when unused (it cannot inline
    /// the assembly) — e.g. the `OSFastCast.h` fast-cast helpers.
    pub inline_asm_symbols: Vec<String>,
    /// Names of PLAIN (non-static) `inline` asm helpers (OSFastCast's
    /// `inline __OSf32tos16`). mwcc materializes each as a GLOBAL UND symbol; the
    /// general codegen path does not, so a non-captured object carrying one defers.
    pub plain_inline_asm_helpers: Vec<String>,
    /// Fixed-address ARRAY globals (`vu32 __EXIRegs[16] : 0xCC006800;` — an `AT_ADDRESS` array):
    /// name -> (address, element type). A `name[i]` subscript materializes the constant base and
    /// indexes it (`lis; addi; lwzx`), distinct from a pointer cast's high-adjusted fold — so codegen
    /// keeps the address rather than desugaring the name (which would compile as the wrong form).
    pub fixed_address_arrays: std::collections::HashMap<String, (i64, Type)>,
}

/// A function definition. Bodies are zero or more local declarations, then zero
/// or more statements, then zero or more `if (...) return ...;` guards, then an
/// optional final `return <expression>;` (absent for a `void` function).
#[derive(Debug, Clone)]
pub struct Function {
    pub return_type: Type,
    pub name: String,
    /// A `static` (file-local) function — emitted with a LOCAL symbol.
    pub is_static: bool,
    /// Declared `__declspec(weak)` (on a prior prototype or the definition) —
    /// emitted with a WEAK symbol binding.
    pub is_weak: bool,
    pub parameters: Vec<Parameter>,
    pub locals: Vec<LocalDeclaration>,
    pub statements: Vec<Statement>,
    pub guards: Vec<GuardedReturn>,
    pub return_expression: Option<Expression>,
    /// An explicit `__declspec(section "…")` code section (e.g. `.init` for the
    /// runtime's `__mem.c`), overriding the default `.text` placement. `None` = `.text`.
    pub section: Option<String>,
    /// The body of a Metrowerks inline-`asm` function — a sequence of instructions
    /// and label definitions, emitted verbatim (no prologue/epilogue synthesis, no
    /// optimizer beyond mwcc's branch-to-return peephole; a trailing `blr` is added
    /// when the body has none). `None` for an ordinary C function. Kept AFTER
    /// `section` so the capture `ast_hash` strip (which truncates from `, section: `
    /// onward) also elides it, preserving every template's hash (fire-465 hazard).
    pub asm_body: Option<Vec<AsmItem>>,
    /// Defined under `#pragma force_active on`: the symbol is forced live in the link
    /// and carries a `.comment` attribute (0x00080000) — animal_crossing's runtime.c
    /// wraps its register save/restore in it. Also AFTER `section` so the `ast_hash`
    /// strip elides it. `false` for the common case.
    pub force_active: bool,
    /// A static inline MATERIALIZED by the call-count heuristic: its `.text`
    /// bytes lay out AFTER the next real function while its SYMBOL stays at
    /// the source position (measured: ww alloc's dealloc_var/__pool_free).
    /// AFTER `section` in declaration order — the ast_hash strip excludes it.
    pub text_deferred: bool,
}

/// One line of an inline-`asm` body: either a label definition (`name:`) or an
/// instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum AsmItem {
    /// A branch-target label definition, e.g. `lbl_80362740:`.
    Label(String),
    /// An `entry <name>` directive: defines an additional GLOBAL symbol at the
    /// current position in the function's `.text` (the runtime's `_savefpr_14` …
    /// register save/restore entry points). Emits no instruction.
    Entry(String),
    /// An assembled instruction line.
    Instruction(AsmInstruction),
}

/// One operand of an inline-`asm` instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum AsmOperand {
    /// A general-purpose register: `rN` (0..=31), or an alias (`sp`/`SP` → r1,
    /// `RTOC` → r2).
    Gpr(u8),
    /// A floating-point register: `fpN` or `fN` (0..=31).
    Fpr(u8),
    /// An integer immediate operand.
    Immediate(i64),
    /// A displacement memory operand `<disp>(<gpr>)` — e.g. `stwu r1, -0x10(r1)`
    /// (`base` = r1, `displacement` = -16). Used by load/store instructions.
    Memory { displacement: i16, base: u8 },
    /// A branch-target label reference (an identifier that is not a register), e.g.
    /// the `lbl_80362740` in `blt lbl_80362740`.
    Label(String),
    /// A condition-register field `crN` (0..=7), e.g. the `cr0` in `cmpwi cr0, r3, 0`.
    ConditionRegister(u8),
    /// A relocated symbol reference `symbol@suffix`, e.g. `__constants@h` in
    /// `lis r4, __constants@h`. The immediate field is filled by the linker.
    Symbol { name: String, suffix: AsmRelocSuffix },
}

/// The `@`-suffix on an inline-`asm` symbol operand, selecting which 16-bit part of
/// the symbol's absolute address the relocation patches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmRelocSuffix {
    /// `@h` — the plain high 16 bits (`R_PPC_ADDR16_HI`).
    Hi,
    /// `@ha` — the high 16 bits, adjusted (`R_PPC_ADDR16_HA`).
    Ha,
    /// `@l` — the low 16 bits (`R_PPC_ADDR16_LO`).
    Lo,
}

/// One instruction line inside an inline-`asm` body: a mnemonic and its operands.
#[derive(Debug, Clone, PartialEq)]
pub struct AsmInstruction {
    pub mnemonic: String,
    pub operands: Vec<AsmOperand>,
}
