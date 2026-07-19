//! The token cursor: the `Parser` state and its primitive operations.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Pointee, Type};
use mwcc_tokens::Token;
use std::collections::HashMap;

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

/// Recoverable layout information from a skipped single-parameter C++ struct
/// template. Methods and static members are irrelevant to object layout; each
/// recorded name is an instance field whose type is the template parameter.
pub(crate) struct StructTemplate {
    pub(crate) fields: Vec<String>,
}

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) position: usize,
    /// Whether plain (unqualified) `char` is signed — the build's `char` default
    /// (mainline/1.3.2+ signed; GC/1.3 build 53 and `-char unsigned` unsigned).
    /// A plain `char` declaration becomes `Type::UnsignedChar` when this is false,
    /// so the codegen omits the `extsb` a signed char would take. `signed char`
    /// and `unsigned char` are explicit and unaffected.
    pub(crate) char_is_signed: bool,
    /// First suffix for a plain inline's `$localstaticN` weak objects.
    pub(crate) plain_inline_localstatic_base: u8,
    /// Base anonymous-label cost of a skipped static-inline definition.
    pub(crate) skipped_static_inline_label_base: u8,
    /// Declared struct layouts, by tag name.
    pub(crate) structs: HashMap<String, StructLayout>,
    /// C++-specific base and declaration-order information for class layouts.
    pub(crate) cxx_classes: HashMap<String, crate::cxx::ClassLayout>,
    /// Single-parameter C++ struct templates whose instance fields can be laid
    /// out when a concrete typedef such as `Vector3<float>` is encountered.
    pub(crate) struct_templates: HashMap<String, StructTemplate>,
    /// In-scope variables that are struct pointers, mapped to their struct tag,
    /// so `variable->field` resolves to the right layout.
    pub(crate) variable_structs: HashMap<String, String>,
    /// Functions that RETURN a struct pointer, mapped to the pointee's struct tag,
    /// so `get()->field` resolves the returned pointer's layout (populated when a
    /// `struct S *get(...)` prototype/definition is parsed).
    pub(crate) function_return_structs: HashMap<String, String>,
    /// Fixed-address globals declared with `AT_ADDRESS` (`Type Name : addr;` — mwcc's `: (addr)`
    /// placement): name -> (address, cast-target POINTER type, struct/union tag). A reference to one
    /// desugars to a const-address deref `*(cast-target)addr` at its use site — a `StructPointer`
    /// for an aggregate (the GX write-gather FIFO `GXWGFifo.u32 = v`, member access via the const-
    /// address path) or a scalar `Pointer` (a hardware register like `__OSBusClock`, direct load/store).
    pub(crate) fixed_address_globals: HashMap<String, (i64, Type, Option<String>)>,
    /// Fixed-address ARRAY globals (`vu32 __EXIRegs[16] : 0xCC006800;`): name -> (address, element
    /// type). Unlike a scalar/aggregate placement, an array is NOT desugared to a const-address cast
    /// (whose subscript folds differently than mwcc's array `lis; addi; lwzx`); the name stays a
    /// variable and this map is handed to codegen, which lays out the array-form subscript.
    pub(crate) fixed_address_arrays: HashMap<String, (i64, Type)>,
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
    /// Names of variadic function DEFINITIONS (side-set — never in the hashed AST).
    pub(crate) variadic_definitions: std::collections::HashSet<String>,
    /// A float-array element whose initializer did NOT fold to a constant —
    /// stashed by `parse_scalar_constant` for the caller to attribute to an
    /// element index (mwcc zero-fills the image and synthesizes a `__sinit`
    /// startup initializer instead).
    pub(crate) unfolded_float_element: Option<mwcc_syntax_trees::Expression>,
    /// (element index, expression) pairs of the CURRENT initializer that need
    /// startup assignment; drained by the global-declaration parser.
    pub(crate) initializer_pending: Vec<(usize, mwcc_syntax_trees::Expression)>,
    /// (array name, element index, expression) triples across the unit — the
    /// synthesized `__sinit_ctx_c`'s assignment list (side-table, hash-safe).
    pub(crate) pending_sinit: Vec<(String, usize, mwcc_syntax_trees::Expression)>,
    /// The current inline-`asm` function's REGISTER PARAMETERS: `(name, gpr,
    /// struct tag)` in declaration order (r3, r4, … positional). An asm operand
    /// naming a parameter resolves to its register (`mr r3,val`), and
    /// `param->field` to a displacement memory operand off it (`stw r5,env->pc`).
    /// Set by `parse_asm_function` for the body parse, cleared after.
    pub(crate) asm_parameters: Vec<(String, u8, Option<String>)>,
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
    /// Names of skipped PLAIN (non-static) `inline` functions with an inline `asm {}`
    /// body (OSFastCast's `inline __OSf32tos16`). mwcc materializes each as a GLOBAL
    /// UND symbol from the dropped compilation; the general codegen path does not emit
    /// them, so an object carrying one (with no capture declaring it) must DEFER.
    pub(crate) plain_inline_asm_helpers: Vec<String>,
    /// Anonymous-label cost accumulated while mwcc compiles and then drops
    /// inline definitions; the base and body-label weights are generation-aware.
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
    /// Names given internal linkage by an earlier `static` function declaration.
    /// A later definition may legally omit `static`; C keeps the prior internal
    /// linkage (`static void f(void); void f(void) {}`).
    pub(crate) static_functions: std::collections::HashSet<String>,
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
    pub(crate) inline_bodies:
        std::collections::HashMap<String, (Vec<String>, mwcc_syntax_trees::Expression)>,
    /// `typedef`-declared struct aliases (`typedef struct _FILE {…} FILE;`) mapped
    /// to their struct tag, so `FILE *p` resolves to the right layout.
    pub(crate) struct_typedefs: HashMap<String, String>,
    /// `typedef`-declared struct-POINTER aliases (`typedef struct {…} *VecPtr;`)
    /// mapped to their struct tag — the alias itself is already a struct pointer.
    pub(crate) struct_pointer_typedefs: HashMap<String, String>,
    /// `typedef`-declared array aliases (`typedef float Mtx[3][4];`) mapped to their
    /// element type, total element count, and INNER-dimension element count (the
    /// product of every dimension after the first; 1 for a 1-D typedef) — the row
    /// stride a decayed parameter subscripts by. The `Type` model has no array
    /// variant of its own, so struct members of this type lay out from the total.
    pub(crate) array_typedefs: HashMap<String, (Type, u16, u16)>,
    /// `typedef`-declared pointer-to-array aliases (`typedef float (*MtxPtr)[4];`)
    /// mapped to their element type and pointed-to-array length — a value of this
    /// type is a ROW pointer (`p[i][j]` strides by the array length).
    pub(crate) row_pointer_typedefs: HashMap<String, (Type, u16)>,
    /// Set by `parse_type` when the type it just returned was an array typedef
    /// (`(element, total, inner)`; a row-pointer typedef reports `total == 0`).
    /// Callers that must not treat the decayed pointer as the object type — the
    /// global-declaration path (an array typedef declares a whole array object) and
    /// the parameter path (records the row stride for subscript desugaring) —
    /// `.take()` this, exactly like `last_struct_tag`.
    pub(crate) last_array_typedef: Option<(Type, u16, u16)>,
    /// Variables (parameters) of a decayed array-typedef / row-pointer-typedef type,
    /// mapped to `(element type, row stride in BYTES)`. A two-constant-subscript use
    /// (`m[i][j]`) desugars to a Member access at `i*stride + j*element`; every OTHER
    /// subscript/deref form on such a variable is an error (defer) — falling through
    /// to the plain `Index` stride would compute the wrong address.
    pub(crate) decayed_row_pointers: HashMap<String, (Type, u16)>,
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
            if std::env::var_os("MWCC_PARSE_DEBUG").is_some() {
                let start = self.position.saturating_sub(8);
                let end = (self.position + 9).min(self.tokens.len());
                eprintln!(
                    "parse context at token {}: {:?}",
                    self.position,
                    &self.tokens[start..end]
                );
            }
            Err(Diagnostic::error(format!(
                "expected {expected}, found {} at token {}",
                self.peek(),
                self.position
            )))
        }
    }

    pub(crate) fn parse_identifier(&mut self) -> Compilation<String> {
        match self.advance() {
            Token::Identifier(name) => Ok(name),
            other => Err(Diagnostic::error(format!(
                "expected an identifier, found {other}"
            ))),
        }
    }
}
