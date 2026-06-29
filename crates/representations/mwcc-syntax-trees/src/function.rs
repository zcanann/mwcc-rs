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
    pub result: Expression,
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
    Switch { scrutinee: Expression, arms: Vec<SwitchArm>, default: Option<Expression> },
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
    /// Names of `static inline` functions whose body contains an inline `asm {}`
    /// block, in declaration order. mwcc keeps each as a deferred function and
    /// emits a local *undefined* symbol for it even when unused (it cannot inline
    /// the assembly) — e.g. the `OSFastCast.h` fast-cast helpers.
    pub inline_asm_symbols: Vec<String>,
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
    pub parameters: Vec<Parameter>,
    pub locals: Vec<LocalDeclaration>,
    pub statements: Vec<Statement>,
    pub guards: Vec<GuardedReturn>,
    pub return_expression: Option<Expression>,
}
