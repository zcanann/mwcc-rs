//! The typed intermediate representation between syntax trees and PCode:
//! MWCC's IRO level. Every expression node carries its resolved C type;
//! member, index and dereference accesses are explicit loads from a
//! `base + index + offset` address with pointer arithmetic already scaled;
//! returns assign a single return value.
//!
//! The syntax-tree builder produces it, IRO passes normalize it (constant
//! folding, algebra, select rewriting, idiom recognition), and instruction
//! selection lowers it to PCode.

use std::collections::HashMap;
use std::fmt;

pub use mwcc_syntax_trees::{Pointee, Type};

/// A file-scope object as the IR sees it.
#[derive(Debug, Clone, Copy)]
pub struct GlobalInfo {
    pub ty: Type,
    /// Addressed through the small-data base (`@sda21`) rather than `lis/@l`.
    pub small_data: bool,
    /// An array object: its name denotes its address, not a loadable value.
    pub is_array: bool,
    /// Every read and write is an observable access (never reused).
    pub is_volatile: bool,
    /// A function (named as a value: its address).
    pub is_function: bool,
    /// `const`: a loaded value stays valid across stores.
    pub is_const: bool,
}

/// The callee name of an indirect call: the target address is the call's
/// first argument.
pub const INDIRECT_CALL: &str = "@indirect";

/// Unit-level facts about names a function refers to.
pub struct Unit<'a> {
    pub globals: &'a HashMap<String, GlobalInfo>,
    pub call_return_types: &'a HashMap<String, Type>,
    /// Calls the compiler expands inline (`__cntlzw`, `__sync`, ...).
    pub is_intrinsic: &'a dyn Fn(&str, usize) -> bool,
    /// Callees declared with `...`.
    pub variadic_callees: &'a std::collections::HashSet<String>,
    /// Callees with a prototype in scope.
    pub prototyped: &'a std::collections::HashSet<String>,
    /// Declared parameter types of callees.
    pub call_parameter_types: &'a HashMap<String, Vec<Type>>,
    /// Whether the unit has a body for a callee (MWCC may inline the call).
    pub has_body: &'a dyn Fn(&str) -> bool,
    /// The body MWCC expands for a call (a `has_body` callee whose
    /// expansion is modeled).
    pub inline_bodies: &'a HashMap<String, &'a mwcc_syntax_trees::Function>,
    /// Pointer variables (by name) whose pointee has no volatile storage:
    /// loads through them may be reused until a store or call.
    pub nonvolatile_pointers: &'a std::collections::HashSet<String>,
    /// An `-O0` compile: an expansion's parameters and result are locals
    /// (register variables).
    pub unoptimized: bool,
    /// Division by a constant multiplies by its magic number (-O4); below,
    /// it divides (`li; divw`).
    pub magic_division: bool,
    /// Scalar-replaced struct locals still store their fields (GC/3.x, Wii).
    pub keeps_struct_stores: bool,
    /// Loaded `const` globals stay valid across calls (GC/3.x, Wii).
    pub const_globals_across_calls: bool,
    /// The build's switch lowering: 0 binary search (GC), 1 binary search
    /// with in-place table loads (GC/3.x), 2 not modeled (Wii).
    pub switch_style: u8,
    /// The callee trusts narrow parameters other than `signed char` to
    /// arrive extended (GC/3.x, Wii).
    pub narrow_parameters_extended: bool,
    /// Short string literals live in small data (`li rD,@N@sda21`).
    pub strings_small_data: bool,
    /// Literals are packed into one `@stringBase` object (not modeled).
    pub strings_packed: bool,
    /// Bit-field stores use the declared type's unit (GC/3.x, Wii; the
    /// front end records the smallest covering unit).
    pub bit_field_declared_units: bool,
    /// The function returns C++ `bool`.
    pub returns_bool: bool,
    /// C++: comparisons and `!` produce `bool`.
    pub cxx: bool,
    /// Floating constants live in small data (`-sdata2` above 0): loaded
    /// `lfs fD,@N@sda21(r0)`; otherwise through an absolute address.
    pub pool_small_data: bool,
}

/// Index into [`Function::variables`].
pub type VarId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Parameter,
    /// A declared local.
    Local,
    /// Introduced by an IRO pass (a select's destination).
    Temporary,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    /// The value type; for an array, its element type.
    pub ty: Type,
    pub kind: VariableKind,
    /// A variable that lives in the frame (an array, a struct, or a scalar
    /// whose address is taken): (bytes, alignment).
    pub frame: Option<(u32, u32)>,
}

/// A function at the IRO level.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub return_type: Type,
    /// Parameters first (in order), then locals in declaration order, then
    /// temporaries.
    pub variables: Vec<Variable>,
    pub parameter_count: usize,
    pub body: Vec<Stmt>,
    /// String literals by bytes (without the NUL), in first-use order.
    pub strings: Vec<Vec<u8>>,
    /// Constant images (initialized local arrays), by index.
    pub images: Vec<Vec<u8>>,
}

impl Function {
    pub fn add_temporary(&mut self, ty: Type) -> VarId {
        let id = self.variables.len();
        self.variables.push(Variable { name: format!("@t{id}"), ty, kind: VariableKind::Temporary, frame: None });
        id
    }
}

/// Where a store writes.
#[derive(Debug, Clone)]
pub enum Place {
    /// `base + index + offset`, `index` already scaled to bytes.
    Memory { base: Box<Expr>, index: Option<Box<Expr>>, offset: i32 },
    /// A scalar file-scope object.
    Global(String),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign { variable: VarId, value: Expr },
    /// Store `value`, of type `ty` (the stored width).
    Store { place: Place, ty: Type, value: Expr },
    /// An expression evaluated for its effect (a call).
    Eval(Expr),
    If { condition: Expr, then_body: Vec<Stmt>, else_body: Vec<Stmt> },
    /// Assign the function's return value without leaving.
    SetReturn(Expr),
    /// Assign the return value (if any) and leave.
    Return(Option<Expr>),
    /// A loop: `condition` tested before each iteration (`test_first`) or
    /// after it; `step` runs after the body (and on `continue`).
    Loop { test_first: bool, condition: Option<Expr>, body: Vec<Stmt>, step: Vec<Stmt> },
    /// A multi-way branch on an integer: each case value selects an arm;
    /// arms are laid out in order and fall through into the next; `default`
    /// is the arm taken by other values (none: leave the switch). `break`
    /// inside an arm leaves the switch.
    Switch { value: Expr, cases: Vec<(i64, usize)>, arms: Vec<Vec<Stmt>>, default: Option<usize> },
    Break,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    BitNot,
    LogicalNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    /// Arithmetic or logical by the left operand's type.
    ShiftRight,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Equal,
    NotEqual,
    LogicalAnd,
    LogicalOr,
}

impl BinaryOp {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinaryOp::Less
                | BinaryOp::Greater
                | BinaryOp::LessEqual
                | BinaryOp::GreaterEqual
                | BinaryOp::Equal
                | BinaryOp::NotEqual
        )
    }

    pub fn is_commutative(self) -> bool {
        matches!(self, BinaryOp::Add | BinaryOp::Multiply | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor)
    }

    /// `a op b` == `b mirror(op) a`.
    pub fn mirror(self) -> BinaryOp {
        match self {
            BinaryOp::Less => BinaryOp::Greater,
            BinaryOp::Greater => BinaryOp::Less,
            BinaryOp::LessEqual => BinaryOp::GreaterEqual,
            BinaryOp::GreaterEqual => BinaryOp::LessEqual,
            other => other,
        }
    }

    /// `!(a op b)` == `a invert(op) b`.
    pub fn invert(self) -> BinaryOp {
        match self {
            BinaryOp::Less => BinaryOp::GreaterEqual,
            BinaryOp::GreaterEqual => BinaryOp::Less,
            BinaryOp::Greater => BinaryOp::LessEqual,
            BinaryOp::LessEqual => BinaryOp::Greater,
            BinaryOp::Equal => BinaryOp::NotEqual,
            BinaryOp::NotEqual => BinaryOp::Equal,
            other => other,
        }
    }
}

/// Branch-free selects MWCC's IRO recognizes (the 2.4.x sign-mask idioms).
#[derive(Debug, Clone)]
pub enum Idiom {
    /// `x < 0 ? -x : x` and its spellings: `srawi; xor; subf`.
    Absolute(Box<Expr>),
    /// `(tested REL 0) ? value : 0` (`and` with the relation's mask) or
    /// `(tested REL 0) ? 0 : value` (`andc`).
    Masked { relation: BinaryOp, tested: Box<Expr>, value: Box<Expr>, keep_when_true: bool },
    /// `rlwimi`: `value` rotated left by `shift` replaces bits `begin..=end`
    /// of `base` (a bit-field store's read-modify-write).
    Insert { base: Box<Expr>, value: Box<Expr>, shift: u8, begin: u8, end: u8 },
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    /// A floating-point constant (typed `Float` or `Double`).
    Float(f64),
    Var(VarId),
    /// The value of a scalar file-scope object.
    Global(String),
    /// The address of a file-scope object (typed as a pointer).
    GlobalAddress(String),
    /// The address of a frame-resident variable (typed as a pointer).
    LocalAddress(VarId),
    /// The address of the function's i-th string literal.
    StringAddress(usize),
    /// The address of the function's i-th constant image (an initialized
    /// local array's bytes, copied into its frame slot).
    Image(usize),
    /// A load of `ty` from `base + index + offset` (`index` scaled).
    Load { base: Box<Expr>, index: Option<Box<Expr>>, offset: i32 },
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// Conversion of the operand to this node's type.
    Convert(Box<Expr>),
    Select { condition: Box<Expr>, when_true: Box<Expr>, when_false: Box<Expr> },
    Call { name: String, arguments: Vec<Expr> },
    Idiom(Idiom),
}

/// A typed expression.
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
}

impl Expr {
    pub fn int(value: i64) -> Expr {
        Expr { kind: ExprKind::Int(value), ty: Type::Int }
    }

    pub fn typed_int(value: i64, ty: Type) -> Expr {
        Expr { kind: ExprKind::Int(value), ty }
    }

    pub fn binary(op: BinaryOp, left: Expr, right: Expr, ty: Type) -> Expr {
        Expr { kind: ExprKind::Binary(op, Box::new(left), Box::new(right)), ty }
    }

    pub fn unary(op: UnaryOp, operand: Expr, ty: Type) -> Expr {
        Expr { kind: ExprKind::Unary(op, Box::new(operand)), ty }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self.kind {
            ExprKind::Int(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_var(&self) -> Option<VarId> {
        match self.kind {
            ExprKind::Var(id) => Some(id),
            _ => None,
        }
    }

    /// Whether the expression may read `variable` (conservative).
    pub fn mentions(&self, variable: VarId) -> bool {
        match &self.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Global(_)
            | ExprKind::GlobalAddress(_)
            | ExprKind::StringAddress(_) | ExprKind::Image(_) => false,
            ExprKind::Var(id) | ExprKind::LocalAddress(id) => *id == variable,
            ExprKind::Load { base, index, .. } => {
                base.mentions(variable) || index.as_ref().is_some_and(|index| index.mentions(variable))
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => operand.mentions(variable),
            ExprKind::Binary(_, left, right) => left.mentions(variable) || right.mentions(variable),
            ExprKind::Select { condition, when_true, when_false } => {
                condition.mentions(variable) || when_true.mentions(variable) || when_false.mentions(variable)
            }
            ExprKind::Call { arguments, .. } => arguments.iter().any(|argument| argument.mentions(variable)),
            ExprKind::Idiom(Idiom::Absolute(value)) => value.mentions(variable),
            ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => {
                tested.mentions(variable) || value.mentions(variable)
            }
            ExprKind::Idiom(Idiom::Insert { base, value, .. }) => base.mentions(variable) || value.mentions(variable),
        }
    }
}

pub fn is_general_word(ty: Type) -> bool {
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

pub fn is_unsigned(ty: Type) -> bool {
    matches!(
        ty,
        Type::UnsignedInt | Type::UnsignedChar | Type::UnsignedShort | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

/// A floating-point scalar.
pub fn is_float(ty: Type) -> bool {
    matches!(ty, Type::Float | Type::Double)
}

/// A scalar held in one general or floating-point register.
pub fn is_value_type(ty: Type) -> bool {
    is_general_word(ty) || is_float(ty)
}

pub fn is_narrow(ty: Type) -> bool {
    matches!(ty, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
}

pub fn is_unsigned_narrow(ty: Type) -> bool {
    matches!(ty, Type::UnsignedChar | Type::UnsignedShort)
}

/// The C integer promotion of an operand type.
pub fn promote(ty: Type) -> Type {
    if is_narrow(ty) {
        Type::Int
    } else {
        ty
    }
}

/// The scalar type a pointer's element loads as.
pub fn pointee_type(pointee: Pointee) -> Option<Type> {
    Some(match pointee {
        Pointee::Int => Type::Int,
        Pointee::UnsignedInt => Type::UnsignedInt,
        Pointee::Char => Type::Char,
        Pointee::UnsignedChar => Type::UnsignedChar,
        Pointee::Short => Type::Short,
        Pointee::UnsignedShort => Type::UnsignedShort,
        // A pointer whose own pointee is unknown: a word, but arithmetic
        // and indexing through it are refused (unsized element).
        Pointee::Pointer => Type::StructPointer { element_size: 0 },
        Pointee::WordPointer => Type::Pointer(Pointee::Int),
        Pointee::Float => Type::Float,
        Pointee::Double => Type::Double,
        _ => return None,
    })
}

/// The byte size of a pointer operand's element, `Some(0)` when unknown
/// (opaque struct or function pointer), `None` for a non-pointer.
pub fn element_size(ty: Type) -> Option<u32> {
    match ty {
        Type::Pointer(pointee) => Some(match pointee {
            Pointee::Char | Pointee::UnsignedChar => 1,
            Pointee::Short | Pointee::UnsignedShort => 2,
            Pointee::Int | Pointee::UnsignedInt | Pointee::Float | Pointee::Pointer | Pointee::WordPointer => 4,
            Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong => 8,
        }),
        Type::StructPointer { element_size } => Some(element_size),
        _ => None,
    }
}

/// A pointer to objects of type `ty`.
pub fn pointer_to(ty: Type) -> Option<Type> {
    Some(match ty {
        Type::Int => Type::Pointer(Pointee::Int),
        Type::UnsignedInt => Type::Pointer(Pointee::UnsignedInt),
        Type::Char => Type::Pointer(Pointee::Char),
        Type::UnsignedChar => Type::Pointer(Pointee::UnsignedChar),
        Type::Short => Type::Pointer(Pointee::Short),
        Type::UnsignedShort => Type::Pointer(Pointee::UnsignedShort),
        Type::Float => Type::Pointer(Pointee::Float),
        Type::Double => Type::Pointer(Pointee::Double),
        Type::Pointer(_) | Type::StructPointer { .. } => Type::Pointer(Pointee::Pointer),
        Type::Struct { size, .. } => Type::StructPointer { element_size: size },
        _ => return None,
    })
}

/// Byte width of a stored or loaded scalar.
pub fn width(ty: Type) -> u32 {
    match ty {
        Type::Char | Type::UnsignedChar => 1,
        Type::Short | Type::UnsignedShort => 2,
        Type::Double => 8,
        _ => 4,
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ExprKind::Int(value) => write!(f, "{value}"),
            ExprKind::Float(value) => write!(f, "{value:?}f"),
            ExprKind::Var(id) => write!(f, "v{id}"),
            ExprKind::Global(name) => write!(f, "{name}"),
            ExprKind::GlobalAddress(name) => write!(f, "&{name}"),
            ExprKind::StringAddress(index) => write!(f, "&@str{index}"),
            ExprKind::Image(index) => write!(f, "&@image{index}"),
            ExprKind::LocalAddress(id) => write!(f, "&v{id}"),
            ExprKind::Load { base, index, offset } => {
                write!(f, "load.{:?}[{base}", self.ty)?;
                if let Some(index) = index {
                    write!(f, " + {index}")?;
                }
                write!(f, " + {offset}]")
            }
            ExprKind::Unary(op, operand) => write!(f, "{op:?}({operand})"),
            ExprKind::Binary(op, left, right) => write!(f, "({left} {op:?} {right})"),
            ExprKind::Convert(operand) => write!(f, "({:?}){operand}", self.ty),
            ExprKind::Select { condition, when_true, when_false } => {
                write!(f, "({condition} ? {when_true} : {when_false})")
            }
            ExprKind::Call { name, arguments } => {
                write!(f, "{name}(")?;
                for (index, argument) in arguments.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{argument}")?;
                }
                write!(f, ")")
            }
            ExprKind::Idiom(idiom) => write!(f, "{idiom:?}"),
        }
    }
}

impl Function {
    /// A readable listing (MWCC_IRO_DUMP).
    pub fn listing(&self) -> String {
        fn statements(out: &mut String, body: &[Stmt], depth: usize) {
            let pad = "  ".repeat(depth);
            for statement in body {
                match statement {
                    Stmt::Assign { variable, value } => out.push_str(&format!("{pad}v{variable} = {value}\n")),
                    Stmt::Store { place, ty, value } => {
                        out.push_str(&format!("{pad}store.{ty:?} {place:?} = {value}\n"))
                    }
                    Stmt::Eval(value) => out.push_str(&format!("{pad}{value}\n")),
                    Stmt::If { condition, then_body, else_body } => {
                        out.push_str(&format!("{pad}if {condition}\n"));
                        statements(out, then_body, depth + 1);
                        if !else_body.is_empty() {
                            out.push_str(&format!("{pad}else\n"));
                            statements(out, else_body, depth + 1);
                        }
                    }
                    Stmt::SetReturn(value) => out.push_str(&format!("{pad}result = {value}\n")),
                    Stmt::Return(Some(value)) => out.push_str(&format!("{pad}return {value}\n")),
                    Stmt::Return(None) => out.push_str(&format!("{pad}return\n")),
                    Stmt::Loop { test_first, condition, body, step } => {
                        let condition = condition.as_ref().map_or("forever".to_owned(), |c| c.to_string());
                        out.push_str(&format!("{pad}loop ({}) {condition}\n", if *test_first { "while" } else { "do" }));
                        statements(out, body, depth + 1);
                        if !step.is_empty() {
                            out.push_str(&format!("{pad}step\n"));
                            statements(out, step, depth + 1);
                        }
                    }
                    Stmt::Switch { value, cases, arms, default } => {
                        out.push_str(&format!("{pad}switch {value}\n"));
                        for (index, arm) in arms.iter().enumerate() {
                            let labels: Vec<String> = cases
                                .iter()
                                .filter(|(_, target)| *target == index)
                                .map(|(value, _)| value.to_string())
                                .chain((*default == Some(index)).then(|| "default".to_owned()))
                                .collect();
                            out.push_str(&format!("{pad}case {}\n", labels.join(", ")));
                            statements(out, arm, depth + 1);
                        }
                    }
                    Stmt::Break => out.push_str(&format!("{pad}break\n")),
                    Stmt::Continue => out.push_str(&format!("{pad}continue\n")),
                }
            }
        }
        let mut out = format!("{} -> {:?}\n", self.name, self.return_type);
        for (id, variable) in self.variables.iter().enumerate() {
            out.push_str(&format!("  v{id}: {} {:?} {:?}\n", variable.name, variable.ty, variable.kind));
        }
        statements(&mut out, &self.body, 1);
        out
    }
}
