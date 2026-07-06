//! The token cursor: the `Parser` state and its primitive operations.

use std::collections::HashMap;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Pointee, Type};
use mwcc_tokens::Token;

/// One resolved struct member: its type and byte offset within the struct, plus
/// the struct tag it points to when the member is itself a struct pointer (so
/// chained access `a->b->c` resolves), or the element type when it is an array.
pub(crate) struct StructField {
    pub(crate) member_type: Type,
    pub(crate) offset: u16,
    pub(crate) struct_tag: Option<String>,
    pub(crate) array_element: Option<Pointee>,
    /// For an array member, the array's TOTAL byte size (dimensions x element
    /// size) — feeds the `sizeof(s.arr)` constant fold. `None` for a scalar.
    pub(crate) array_bytes: Option<u16>,
    /// For a bit-field member, `(bit_offset, width)` — the field occupies `width` bits
    /// starting `bit_offset` bits from the most-significant end of its storage unit
    /// (which begins at byte `offset`). `None` for an ordinary member. Member access
    /// of a bit-field defers until the extract/insert codegen lands.
    pub(crate) bit_field: Option<(u8, u8)>,
}

/// A struct's layout: members by name, plus the total size (for `sizeof`/arrays,
/// later). Offsets follow natural alignment (the `-align powerpc` default).
#[derive(Default)]
pub(crate) struct StructLayout {
    pub(crate) fields: HashMap<String, StructField>,
    /// The struct's total size in bytes (members plus trailing padding to the
    /// struct's alignment) — the stride for an array/pointer of this struct.
    pub(crate) size: u16,
    /// The struct's alignment (the max member alignment) — a struct value's stack
    /// slot is aligned to this, not to its size.
    pub(crate) align: u8,
}

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) position: usize,
    /// Declared struct layouts, by tag name.
    pub(crate) structs: HashMap<String, StructLayout>,
    /// In-scope variables that are struct pointers, mapped to their struct tag,
    /// so `variable->field` resolves to the right layout.
    pub(crate) variable_structs: HashMap<String, String>,
    /// Struct-typed GLOBALS by name -> struct tag (`extern FILE_TABLE __files;`),
    /// so `&__files._stdout` in an initializer resolves its member offset.
    pub(crate) global_structs: HashMap<String, String>,
    /// In-scope variables (parameters and scalar locals) mapped to their declared type, so
    /// `sizeof(var)` folds to a constant. Cleared per function in `function_body`.
    pub(crate) variable_types: HashMap<String, Type>,
    /// Local array variables mapped to their total byte size (element size * length), so
    /// `sizeof(arr)` folds to a constant. Cleared per function.
    pub(crate) variable_array_bytes: HashMap<String, u32>,
    /// File-scope variables mapped to `(total byte size, array element size)`, so `sizeof(g)`
    /// (total) and `sizeof(g[0])` (element) fold to constants. The element size is `Some` ONLY
    /// for an ARRAY global — for a pointer global `sizeof(*p)`/`sizeof(p[0])` wants the pointee,
    /// not the 4-byte pointer, so a non-array keeps element `None` and those forms defer. NOT
    /// cleared per function — globals stay in scope for every function's body.
    pub(crate) global_sizes: HashMap<String, (u32, Option<u32>)>,
    /// `typedef`-declared type aliases (e.g. `u32` -> `unsigned int`).
    pub(crate) typedefs: HashMap<String, Type>,
    /// Set by [`Parser::parse_type`] when it just parsed a `struct Name*`, so the
    /// declarator parser can associate the variable name with the struct tag.
    pub(crate) last_struct_tag: Option<String>,
    /// The struct tag of the expression `factor` just returned, so the tag survives
    /// a wrapping `(...)` — `((struct S *)x)->field` resolves through the parens.
    pub(crate) expression_struct_tag: Option<String>,
    /// Set by [`Parser::parse_type`] when the type carried a leading `const`. It is
    /// transparent on a parameter/local/return type, but on a file-scope global it
    /// changes the section (read-only) — so the global path defers when it is set.
    pub(crate) last_type_was_const: bool,
    /// Set when a `const` TRAILS the pointer star (`void* const p`) — the POINTER
    /// OBJECT is const (read-only), distinct from a leading `const void* p` where
    /// only the pointee is const. The global path routes the former to `.sdata2`.
    pub(crate) last_pointer_const: bool,
    /// Set by `skip_type_qualifiers` when the just-parsed type carried `volatile`.
    /// Layout and a simple access ignore it; a value-tracked local guards on it and
    /// defers (a volatile local's access must not be elided/folded).
    pub(crate) last_type_was_volatile: bool,
    /// Set when a member access decays an ARRAY member to its address
    /// (`Expression::MemberAddress`): the array's total byte size. Consumed by
    /// the `sizeof(s.arr)` fold, which resets it before parsing its operand.
    pub(crate) last_member_array_bytes: Option<u16>,
    /// Active block-scope shadow renames, innermost last: (source name,
    /// internal hoisted name like `i@2`). Pushed when a block declaration
    /// shadows an existing local; truncated at the block's close brace.
    /// `factor` resolves bare names through this stack (latest wins).
    pub(crate) block_renames: Vec<(String, String)>,
    /// Monotonic counter feeding shadow-rename internal names.
    pub(crate) rename_counter: usize,
    /// `#pragma defer_codegen on` is active: functions defined under it are
    /// code-generated LAST, in reverse definition order (measured: melee
    /// mem_funcs — the whole TU's .text is reversed).
    pub(crate) defer_codegen: bool,
    /// Names of functions defined while defer_codegen was on, in definition order.
    pub(crate) deferred_function_names: Vec<String>,
    /// Names of skipped `static inline` functions with an inline `asm {}` body, in
    /// declaration order; mwcc emits a local undefined symbol for each.
    pub(crate) inline_asm_symbols: Vec<String>,
    /// Count of skipped `inline`/`static inline` FUNCTION DEFINITIONS: mwcc
    /// compiles-then-drops these, advancing the file's `@N` counter by 3 each
    /// (measured), so the writer pre-bumps the first function's numbering.
    pub(crate) skipped_inline_functions: usize,
    /// Per static-local NAME, the skipped-inline bump total at its DECLARATION
    /// point — a static numbers off the anonymous counter AS OF that position
    /// (measured: mp4 uart's initialized$4 inside the FIRST inline vs pikmin's
    /// $34 behind 30 counts of earlier header inlines).
    pub(crate) static_local_prebumps: std::collections::HashMap<String, usize>,
    /// Token positions of anonymous-`enum` bodies already counted into the
    /// anonymous-`@N` pre-bump (guards speculative re-parses from double-counting).
    pub(crate) counted_enum_positions: std::collections::HashSet<usize>,
    /// Materialized static-inline functions with NO prior prototype (the
    /// implicit-declaration shape): their call relocations bind the surviving
    /// UNDEFINED global ghost, and their local symbols order differently.
    pub(crate) implicitly_materialized: Vec<String>,
    /// PLAIN-inline materializations (WEAK globals with the 0x0d comment flag).
    pub(crate) weak_materialized: Vec<String>,
    /// Names declared `__declspec(weak)` — their definitions emit WEAK symbols.
    pub(crate) weak_functions: std::collections::HashSet<String>,
    /// A `__declspec(section "…")` seen on a function PROTOTYPE — mwcc applies it to
    /// the later definition (pikmin's `DECL_SECT(".init")` sits on the memcpy proto).
    pub(crate) section_functions: std::collections::HashMap<String, String>,
    /// Names of SKIPPED inline definitions — a call to one defers the unit.
    pub(crate) skipped_inline_names: std::collections::HashSet<String>,
    /// `#pragma cplusplus` state: declarations parsed under it mangle their
    /// symbol names (push/pop scope the switch).
    pub(crate) cplusplus: bool,
    pub(crate) cplusplus_stack: Vec<bool>,
    /// `#pragma force_active on`/`reset` state: definitions parsed under it are kept
    /// in the link even if unreferenced, carrying a `.comment` attribute (0x00080000)
    /// — animal_crossing's runtime.c wraps its register save/restore in it.
    pub(crate) force_active: bool,
    /// Parsed single-return inline bodies: name -> (parameter names, body) —
    /// substituted at call sites with pure arguments (mwcc -inline auto).
    pub(crate) inline_bodies: std::collections::HashMap<String, (Vec<String>, mwcc_syntax_trees::Expression)>,
    /// `typedef`-declared struct aliases (`typedef struct _FILE {…} FILE;`) mapped
    /// to their struct tag, so `FILE *p` resolves to the right layout.
    pub(crate) struct_typedefs: HashMap<String, String>,
    /// `typedef`-declared struct-POINTER aliases (`typedef struct {…} *VecPtr;`)
    /// mapped to their struct tag — the alias itself is already a struct pointer.
    pub(crate) struct_pointer_typedefs: HashMap<String, String>,
    /// `typedef`-declared array aliases (`typedef float Mtx[3][4];`) mapped to their
    /// element type and total element count, so a struct member of this type lays out
    /// with the right size (the `Type` model has no array variant of its own).
    pub(crate) array_typedefs: HashMap<String, (Type, u16)>,
    /// Enumeration constant values, so a bare enumerator resolves to its integer
    /// value in an expression. (`-enum int`: an enum type is a 4-byte `int`.)
    pub(crate) enum_constants: HashMap<String, i64>,
}

impl Parser {
    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }
    /// The token `offset` positions ahead, clamped to the final (end-of-input)
    /// token so lookahead never runs off the end.
    pub(crate) fn peek_at(&self, offset: usize) -> &Token {
        let index = (self.position + offset).min(self.tokens.len() - 1);
        &self.tokens[index]
    }
    /// If the next two tokens are an arithmetic/bitwise operator followed by `=`
    /// (a compound assignment like `+=`), return the operator. The operator and
    /// `=` are NOT consumed.
    pub(crate) fn peek_compound_assignment(&self) -> Option<mwcc_syntax_trees::BinaryOperator> {
        use mwcc_syntax_trees::BinaryOperator;
        if *self.peek_at(1) != Token::Equals {
            return None;
        }
        Some(match self.peek() {
            Token::Plus => BinaryOperator::Add,
            Token::Minus => BinaryOperator::Subtract,
            Token::Star => BinaryOperator::Multiply,
            Token::Slash => BinaryOperator::Divide,
            Token::Percent => BinaryOperator::Modulo,
            Token::Ampersand => BinaryOperator::BitAnd,
            Token::Pipe => BinaryOperator::BitOr,
            Token::Caret => BinaryOperator::BitXor,
            Token::ShiftLeft => BinaryOperator::ShiftLeft,
            Token::ShiftRight => BinaryOperator::ShiftRight,
            _ => return None,
        })
    }
    pub(crate) fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }
    pub(crate) fn expect(&mut self, expected: Token) -> Compilation<()> {
        if *self.peek() == expected {
            self.position += 1;
            Ok(())
        } else {
            Err(Diagnostic::error(format!("expected {expected}, found {}", self.peek())))
        }
    }

    pub(crate) fn parse_identifier(&mut self) -> Compilation<String> {
        match self.advance() {
            Token::Identifier(name) => Ok(name),
            other => Err(Diagnostic::error(format!("expected an identifier, found {other}"))),
        }
    }
}
