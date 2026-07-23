//! Metrowerks C++ surface syntax kept out of the general C item parser.
//!
//! Linkage specifications are declaration wrappers, not declarations themselves;
//! normalization removes those wrappers before recursive descent. Symbol names
//! use CodeWarrior's own mangling rather than the Itanium ABI.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{
    Expression, Function, Parameter, Pointee, SourceFundamentalType, Statement, Type,
};
use mwcc_tokens::{LocatedToken, Token};

use crate::cxx_analysis_facts::{
    function_declaration_virtuality, inline_control_flow_labels,
    nested_explicit_virtual_declarations,
};
use crate::items::{pointee_of, type_alignment, type_size};
use crate::parser::{Parser, StructField, StructLayout};

/// Give inline special members identities that cannot collide with one
/// another. Their source spellings both contain the class name (`C()` and
/// `~C()`), while out-of-class recovery needs to distinguish the two before
/// either one has been mangled.
pub(crate) fn canonical_inline_member_name(
    class: &str,
    source_name: &str,
    is_destructor: bool,
) -> String {
    if is_destructor {
        "__dt".to_string()
    } else if class.rsplit("::").next() == Some(source_name) {
        "__ct".to_string()
    } else {
        source_name.to_string()
    }
}

/// The C++-only information that a plain C struct layout cannot retain.
/// Declaration order controls constructor initialization order, while base
/// names distinguish a base initializer from an identically shaped member.
#[derive(Clone, Default)]
pub(crate) struct ClassLayout {
    pub(crate) bases: Vec<BaseClass>,
    /// Size occupied before deferred virtual-base subobjects are appended.
    /// A derived class reuses this boundary for its own members and appends one
    /// copy of every inherited virtual base after its own non-virtual region.
    pub(crate) nonvirtual_size: u32,
    /// Unique virtual-base identities inherited or declared by this class.
    pub(crate) virtual_bases: Vec<String>,
    pub(crate) fields: Vec<String>,
    pub(crate) constructors: Vec<ClassParameterTypes>,
    pub(crate) methods: std::collections::HashMap<String, Vec<MemberMethod>>,
    /// The class has a virtual dispatch table pointer. This is layout state,
    /// not merely syntax: a polymorphic primary base already supplies the slot.
    pub(crate) is_polymorphic: bool,
    /// Byte offset of the primary vptr. CodeWarrior places a class's first vptr
    /// at the declaration position of its first virtual member rather than
    /// unconditionally at offset zero.
    pub(crate) vptr_offset: Option<u32>,
    /// Number of primary-table callable slots introduced by this class. The
    /// two ABI header words are not included.
    pub(crate) virtual_slots: usize,
    /// Non-pure virtual function definitions keyed by their byte offset in the
    /// primary vtable (including the two ABI header words).
    pub(crate) virtual_definitions: Vec<(u16, String)>,
    /// First non-inline, non-pure virtual member declared by this class. Its
    /// out-of-line definition is the ABI key function and owns the vtable when
    /// the class has no earlier virtual destructor owner.
    pub(crate) vtable_key_function: Option<String>,
    /// Whether the class declares a virtual destructor. Its out-of-line
    /// definition is a key function and owns the primary vtable in the subset
    /// currently materialized by the frontend.
    pub(crate) has_virtual_destructor: bool,
    /// Whether this class writes a destructor declaration of its own. A class
    /// can inherit virtual destruction without spelling one; that distinction
    /// determines whether the frontend may synthesize the implicit definition.
    pub(crate) declares_destructor: bool,
    /// Primary-vtable byte offset of the deleting-destructor entry.
    pub(crate) virtual_destructor_slot: Option<u16>,
    /// Every polymorphic non-virtual base subobject contributing a vptr to the
    /// complete object, in depth-first declaration order. The first component
    /// is the primary table; later components become contiguous subtables in
    /// CodeWarrior's vtable group.
    pub(crate) vtable_components: Vec<VtableComponent>,
}

#[derive(Clone)]
pub(crate) struct VtableComponent {
    /// Adjustment from the complete object to the base subobject passed as
    /// `this`. This can differ from `vptr_offset` when a base declares data
    /// before its first virtual member.
    pub(crate) object_offset: u32,
    pub(crate) vptr_offset: u32,
    pub(crate) virtual_slots: usize,
    pub(crate) virtual_destructor_slot: Option<u16>,
}

#[derive(Clone)]
pub(crate) struct MemberMethod {
    pub(crate) parameters: Vec<Type>,
    cxx_parameters: Vec<CxxParameterType>,
    return_struct_tag: Option<String>,
    pub(crate) is_inline: bool,
    is_const_member: bool,
    virtual_dispatch: Option<VirtualDispatch>,
}

#[derive(Clone)]
pub(crate) struct ClassParameterTypes {
    pub(crate) parameters: Vec<Type>,
    pub(crate) cxx_parameters: Vec<CxxParameterType>,
}

/// Constructor work split at the point where CodeWarrior installs this
/// class's vptrs: non-virtual bases are initialized first, then the vptrs,
/// followed by class-valued members and the source-written body.
#[derive(Default)]
pub(crate) struct ConstructorInitialization {
    pub(crate) statements: Vec<Statement>,
    pub(crate) vptr_insertion_index: usize,
}

/// A callable class declaration recovered without requiring class layout.
/// The ready-mangled name keeps overload selection independent of expression
/// type inference; fixed arity plus the variadic bit is enough to reject
/// ambiguous calls safely.
#[derive(Clone)]
pub(crate) struct RecoveredCxxMethod {
    pub(crate) mangled: String,
    pub(crate) fixed_parameter_count: usize,
    pub(crate) variadic: bool,
    pub(crate) parameters: Vec<Type>,
    pub(crate) cxx_parameters: Vec<CxxParameterType>,
}

/// One entry in CodeWarrior's primary virtual table. Slot offsets include the
/// two-word ABI header: the first callable entry is therefore byte offset 8.
#[derive(Clone)]
pub(crate) struct RecoveredCxxVirtualMethod {
    pub(crate) return_type: Type,
    pub(crate) return_struct_tag: Option<String>,
    pub(crate) parameters: Vec<Type>,
    pub(crate) fixed_parameter_count: usize,
    pub(crate) variadic: bool,
    pub(crate) vptr_offset: u16,
    pub(crate) slot_offset: u16,
}

/// Declaration-only virtual dispatch state. This is intentionally independent
/// of [`ClassLayout`]: large preprocessed headers often contain class syntax we
/// cannot lay out yet, while their primary vtable declarations remain enough
/// to lower a call safely.
#[derive(Clone)]
pub(crate) struct RecoveredCxxDispatchTable {
    pub(crate) methods: std::collections::HashMap<String, Vec<RecoveredCxxVirtualMethod>>,
    pub(crate) next_slot_offset: u16,
}

impl Default for RecoveredCxxDispatchTable {
    fn default() -> Self {
        Self {
            methods: std::collections::HashMap::new(),
            next_slot_offset: 8,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct VirtualDispatch {
    pub(crate) vptr_offset: u16,
    pub(crate) slot_offset: u16,
    pub(crate) return_type: Type,
    pub(crate) variadic: bool,
}

pub(crate) enum ImplicitMemberCall {
    Direct {
        name: String,
        is_inline: bool,
        this_adjustment: u32,
        parameters: Vec<Type>,
    },
    Virtual {
        dispatch: VirtualDispatch,
        return_struct_tag: Option<String>,
        this_adjustment: u32,
        direct_name: Option<String>,
        direct_is_inline: bool,
        parameters: Vec<Type>,
    },
}

impl ImplicitMemberCall {
    pub(crate) fn parameters(&self) -> &[Type] {
        match self {
            Self::Direct { parameters, .. } | Self::Virtual { parameters, .. } => parameters,
        }
    }
}

/// The C++ ABI identity of one source parameter. The general syntax-tree
/// [`Type`] intentionally describes storage and register class only; it cannot
/// distinguish `A*` from `B*`, or a reference from its pointer-shaped calling
/// convention. Name mangling needs those distinctions, so they live in this
/// declaration-only companion instead of leaking into C code generation.
#[derive(Clone, PartialEq)]
pub(crate) struct CxxParameterType {
    source_type: Type,
    source_fundamental: Option<SourceFundamentalType>,
    qualified_name: Option<String>,
    is_wchar: bool,
    is_reference: bool,
    pointee_const: bool,
    pointer_const: bool,
    pointer_depth: u8,
    pointer_base: Option<Type>,
    function_type: Option<Box<CxxFunctionType>>,
}

/// Source-level function type retained behind a function-pointer declarator.
///
/// The executable IR only needs to know that a function pointer is one word,
/// but CodeWarrior's C++ ABI encodes the complete parameter and return type in
/// every symbol that mentions it. Keeping that declaration-only identity here
/// avoids widening storage/codegen types while preserving enough information
/// for nested `P F <arguments> _ <return>` mangling.
#[derive(Clone, PartialEq)]
pub(crate) struct CxxFunctionType {
    return_type: CxxParameterType,
    parameters: Vec<CxxParameterType>,
    variadic: bool,
}

impl CxxFunctionType {
    pub(crate) fn new(
        return_type: CxxParameterType,
        parameters: Vec<CxxParameterType>,
        variadic: bool,
    ) -> Self {
        Self {
            return_type,
            parameters,
            variadic,
        }
    }

    pub(crate) fn source_identity(&self) -> mwcc_syntax_trees::SourceFunctionType {
        mwcc_syntax_trees::SourceFunctionType {
            return_type: self.return_type.source_identity(),
            parameters: self
                .parameters
                .iter()
                .map(CxxParameterType::source_identity)
                .collect(),
            variadic: self.variadic,
        }
    }
}

impl CxxParameterType {
    fn source_identity(&self) -> mwcc_syntax_trees::SourceTypeIdentity {
        mwcc_syntax_trees::SourceTypeIdentity {
            declared_type: self.source_type,
            source_fundamental: self.source_fundamental,
            aggregate_tag: self.qualified_name.clone(),
            pointer_depth: self.pointer_depth,
            is_reference: self.is_reference,
            function_type: self
                .function_type
                .as_deref()
                .map(CxxFunctionType::source_identity)
                .map(Box::new),
        }
    }

    /// Compare source-level callable identity without allowing the recovered
    /// size of an aggregate to change its type. A self-reference parsed inside
    /// its own class is necessarily incomplete (size zero), while the same
    /// qualified type is complete by the time a derived override is parsed.
    fn same_declaration_identity(&self, other: &Self) -> bool {
        let source_type_matches = self.source_type == other.source_type
            || (self.qualified_name.is_some()
                && self.qualified_name == other.qualified_name
                && matches!(
                    (self.source_type, other.source_type),
                    (Type::Struct { .. }, Type::Struct { .. })
                        | (Type::StructPointer { .. }, Type::StructPointer { .. })
                ));
        source_type_matches
            && self.source_fundamental == other.source_fundamental
            && self.qualified_name == other.qualified_name
            && self.is_wchar == other.is_wchar
            && self.is_reference == other.is_reference
            && self.pointee_const == other.pointee_const
            && self.pointer_const == other.pointer_const
            && self.pointer_depth == other.pointer_depth
            && self.pointer_base == other.pointer_base
            && self.function_type == other.function_type
    }

    pub(crate) fn parsed(
        source_type: Type,
        qualified_name: Option<String>,
        is_wchar: bool,
        is_reference: bool,
        source_is_aggregate_value: bool,
        pointee_const: bool,
        pointer_const: bool,
    ) -> Self {
        Self {
            source_type,
            source_fundamental: None,
            qualified_name,
            is_wchar,
            is_reference,
            pointee_const,
            pointer_const,
            pointer_depth: u8::from(
                !source_is_aggregate_value
                    && matches!(source_type, Type::Pointer(_) | Type::StructPointer { .. }),
            ),
            pointer_base: None,
            function_type: None,
        }
    }

    pub(crate) fn with_pointer_shape(
        mut self,
        pointer_depth: u8,
        pointer_base: Option<Type>,
    ) -> Self {
        if pointer_depth != 0 {
            self.pointer_depth = pointer_depth;
            self.pointer_base = pointer_base;
        }
        self
    }

    pub(crate) fn with_source_fundamental(
        mut self,
        source_fundamental: Option<SourceFundamentalType>,
    ) -> Self {
        self.source_fundamental = source_fundamental;
        self
    }

    pub(crate) fn with_function_type(mut self, function_type: Option<CxxFunctionType>) -> Self {
        self.function_type = function_type.map(Box::new);
        self
    }

    pub(crate) fn plain(source_type: Type) -> Self {
        Self::parsed(source_type, None, false, false, false, false, false)
    }
}

#[derive(Clone)]
pub(crate) struct BaseClass {
    pub(crate) name: String,
    pub(crate) offset: u32,
    pub(crate) is_virtual: bool,
}

/// Normalize C++ linkage specifications into the same scoped language pragmas
/// the top-level parser already understands. The braces are declaration
/// wrappers rather than C++ scopes, while a single-declaration form retains
/// `extern` as its storage class.
pub(crate) fn normalize_linkage_specifications(mut tokens: Vec<LocatedToken>) -> Vec<LocatedToken> {
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        let starts_linkage = matches!(&tokens[index].token, Token::Identifier(word) if word == "extern")
            && matches!(&tokens[index + 1].token, Token::StringLiteral(language) if language == b"C" || language == b"C++");
        if !starts_linkage {
            index += 1;
            continue;
        }

        let location = tokens[index].location;
        let cplusplus = matches!(&tokens[index + 1].token, Token::StringLiteral(language) if language == b"C++");
        let push = LocatedToken {
            token: Token::Pragma("push".to_string()),
            location,
        };
        let language = LocatedToken {
            token: Token::Pragma(if cplusplus {
                "cplusplus on".to_string()
            } else {
                "cplusplus off".to_string()
            }),
            location,
        };
        let pop = LocatedToken {
            token: Token::Pragma("pop".to_string()),
            location,
        };

        if tokens
            .get(index + 2)
            .is_some_and(|located| located.token == Token::BraceOpen)
        {
            let mut cursor = index + 2;
            let mut depth = 0i32;
            let mut close = None;
            while cursor < tokens.len() {
                match tokens[cursor].token {
                    Token::BraceOpen => depth += 1,
                    Token::BraceClose => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(cursor);
                            break;
                        }
                    }
                    Token::EndOfFile => break,
                    _ => {}
                }
                cursor += 1;
            }
            if let Some(close) = close {
                tokens[close] = pop;
                tokens.splice(index..index + 3, [push, language]);
                index += 2;
                continue;
            }
        } else {
            // Keep `extern`, but scope the language mode through the declaration's
            // terminal semicolon. Function definitions in this spelling are rare;
            // their closing brace is accepted when no semicolon follows it.
            tokens.remove(index + 1);
            tokens.insert(index, push);
            tokens.insert(index + 1, language);
            let mut cursor = index + 3;
            let mut parens = 0i32;
            let mut brackets = 0i32;
            let mut braces = 0i32;
            let mut terminal = None;
            while cursor < tokens.len() {
                match tokens[cursor].token {
                    Token::ParenOpen => parens += 1,
                    Token::ParenClose => parens -= 1,
                    Token::BracketOpen => brackets += 1,
                    Token::BracketClose => brackets -= 1,
                    Token::BraceOpen => braces += 1,
                    Token::BraceClose => {
                        braces -= 1;
                        if braces == 0
                            && parens == 0
                            && brackets == 0
                            && !tokens
                                .get(cursor + 1)
                                .is_some_and(|next| next.token == Token::Semicolon)
                        {
                            terminal = Some(cursor);
                            break;
                        }
                    }
                    Token::Semicolon if parens == 0 && brackets == 0 && braces == 0 => {
                        terminal = Some(cursor);
                        break;
                    }
                    Token::EndOfFile => break,
                    _ => {}
                }
                cursor += 1;
            }
            if let Some(terminal) = terminal {
                tokens.insert(terminal + 1, pop);
                index = terminal + 2;
                continue;
            }
        }
        index += 1;
    }
    tokens
}

/// C++ constructors and destructors have no written return type. Insert the
/// parser-internal `void` only for top-level `Class::Class(` / `Class::~Class(`
/// declarators, leaving class-body prototypes and expression-level qualified
/// names untouched. A destructor's written `~Class` is normalized to its
/// CodeWarrior ABI source name `__dt` so the ordinary qualified-member parser
/// can handle both special members through one path.
pub(crate) fn normalize_constructor_declarators(
    mut tokens: Vec<LocatedToken>,
) -> Vec<LocatedToken> {
    let mut index = 0usize;
    // Track which braces are declaration-only namespace wrappers. A constructor
    // inside a namespace is still a top-level declarator; one inside a class or
    // function body is not. Keeping the scope kind avoids treating all nonzero
    // brace depth alike.
    let mut declaration_scopes: Vec<bool> = Vec::new();
    while index + 4 < tokens.len() {
        match tokens[index].token {
            Token::BraceOpen => {
                let opens_namespace = matches!(
                    tokens.get(index.wrapping_sub(1)).map(|located| &located.token),
                    Some(Token::Identifier(word)) if word == "namespace"
                ) || (matches!(
                    tokens.get(index.wrapping_sub(2)).map(|located| &located.token),
                    Some(Token::Identifier(word)) if word == "namespace"
                ) && matches!(
                    tokens
                        .get(index.wrapping_sub(1))
                        .map(|located| &located.token),
                    Some(Token::Identifier(_))
                ));
                declaration_scopes.push(opens_namespace);
            }
            Token::BraceClose => {
                declaration_scopes.pop();
            }
            _ => {}
        }
        // Scan a complete qualified declarator prefix. Besides `C::C()`, a
        // definition may be written at global scope as `N::C::C()` (and with
        // arbitrarily many namespace components). The final class and member
        // names identify a constructor; `N::C::~C()` identifies a destructor.
        let mut names = Vec::new();
        let mut cursor = index;
        if let Token::Identifier(name) = &tokens[cursor].token {
            names.push(name.clone());
            cursor += 1;
            while cursor + 2 < tokens.len()
                && tokens[cursor].token == Token::Colon
                && tokens[cursor + 1].token == Token::Colon
                && matches!(tokens[cursor + 2].token, Token::Identifier(_))
            {
                let Token::Identifier(name) = &tokens[cursor + 2].token else {
                    unreachable!();
                };
                names.push(name.clone());
                cursor += 3;
            }
        }
        let at_declaration_scope = declaration_scopes.iter().all(|scope| *scope);
        let constructor = at_declaration_scope
            && names.len() >= 2
            && names[names.len() - 2] == names[names.len() - 1]
            && tokens.get(cursor).is_some_and(|token| token.token == Token::ParenOpen);
        let destructor = at_declaration_scope
            && !names.is_empty()
            && tokens.get(cursor).is_some_and(|token| token.token == Token::Colon)
            && tokens
                .get(cursor + 1)
                .is_some_and(|token| token.token == Token::Colon)
            && tokens
                .get(cursor + 2)
                .is_some_and(|token| token.token == Token::Tilde)
            && matches!(tokens.get(cursor + 3).map(|token| &token.token), Some(Token::Identifier(name)) if name == names.last().unwrap())
            && tokens
                .get(cursor + 4)
                .is_some_and(|token| token.token == Token::ParenOpen);
        if constructor {
            let location = tokens[index].location;
            tokens.insert(
                index,
                LocatedToken {
                    token: Token::KeywordVoid,
                    location,
                },
            );
            index = cursor + 2;
        } else if destructor {
            let location = tokens[index].location;
            tokens[cursor + 2].token = Token::Identifier("__dt".to_string());
            tokens.remove(cursor + 3);
            tokens.insert(
                index,
                LocatedToken {
                    token: Token::KeywordVoid,
                    location,
                },
            );
            index = cursor + 4;
        } else {
            index += 1;
        }
    }
    tokens
}

impl Parser {
    /// Charge one source-written function-parameter name exactly once even when
    /// declaration recovery speculatively parses the same token range again.
    pub(crate) fn record_named_parameter_at(&mut self, position: usize) {
        if self.counted_named_parameter_positions.insert(position) {
            self.named_prototype_parameters += 1;
        }
    }

    /// Merge source facts learned by an isolated recovery parser. Token indices
    /// remain stable because every probe clones the same translation-unit stream.
    pub(crate) fn merge_named_parameter_positions_from(&mut self, probe: &Parser) {
        for position in &probe.counted_named_parameter_positions {
            self.record_named_parameter_at(*position);
        }
    }

    pub(crate) fn remove_named_parameters_in(&mut self, open: usize, close: usize) {
        self.counted_named_parameter_positions
            .retain(|position| *position <= open || *position >= close);
        self.named_prototype_parameters = (self.counted_named_parameter_positions.len()
            + self.removed_template_named_parameters)
            .saturating_sub(self.reused_template_named_parameters);
    }

    /// Consume the source-only facts left by `parse_type` into one ABI type.
    /// Storage/codegen keeps using the returned `Type` independently.
    pub(crate) fn take_cxx_type_identity(
        &mut self,
        source_type: Type,
        is_reference: bool,
    ) -> CxxParameterType {
        let qualified_name = self.last_enum_tag.take().or_else(|| {
            self.last_struct_tag
                .take()
                .map(|tag| self.struct_typedefs.get(&tag).cloned().unwrap_or(tag))
        });
        CxxParameterType::parsed(
            source_type,
            qualified_name,
            self.last_type_was_wchar,
            is_reference,
            self.last_type_was_aggregate_reference,
            self.last_type_was_const,
            self.last_pointer_const,
        )
        .with_source_fundamental(self.last_source_fundamental.take())
        .with_pointer_shape(self.last_cxx_pointer_depth, self.last_cxx_pointer_base)
        .with_function_type(self.last_cxx_function_type.take())
    }

    /// Consume a function-pointer declarator after its return type:
    /// `(*name)(parameter-types)`. Both ordinary functions and recovered class
    /// members use this spelling; keeping its semantic signature parsing here
    /// prevents the declarator grammars from drifting apart.
    pub(crate) fn try_cxx_function_pointer_declarator(
        &mut self,
        return_type: CxxParameterType,
    ) -> Compilation<Option<(String, Option<usize>, CxxFunctionType)>> {
        if *self.peek() != Token::ParenOpen || *self.peek_at(1) != Token::Star {
            return Ok(None);
        }
        self.advance(); // `(`
        self.advance(); // `*`
        let (name, name_position) = if matches!(self.peek(), Token::Identifier(_)) {
            let name_position = self.position;
            (self.parse_identifier()?, Some(name_position))
        } else {
            (String::new(), None)
        };
        self.expect(Token::ParenClose)?;
        let function_type = self.parse_cxx_function_type(return_type)?;
        Ok(Some((name, name_position, function_type)))
    }

    /// Parse the `(parameters)` portion of a function type after its return
    /// type and pointer declarator have already been consumed.
    pub(crate) fn parse_cxx_function_type(
        &mut self,
        return_type: CxxParameterType,
    ) -> Compilation<CxxFunctionType> {
        self.expect(Token::ParenOpen)?;
        let mut parameters = Vec::new();
        let mut variadic = false;
        if *self.peek() == Token::KeywordVoid && *self.peek_at(1) == Token::ParenClose {
            self.advance();
        } else {
            while *self.peek() != Token::ParenClose {
                if matches!(
                    self.tokens.get(self.position..self.position + 3),
                    Some([Token::Dot, Token::Dot, Token::Dot])
                ) {
                    self.position += 3;
                    variadic = true;
                    break;
                }
                let mut storage_type = self.parse_type()?;
                while *self.peek() == Token::Star {
                    self.advance();
                    self.last_cxx_pointer_depth =
                        self.last_cxx_pointer_depth.saturating_add(1).max(1);
                    storage_type = Type::Pointer(Pointee::Pointer);
                }
                let is_reference = self.eat_keyword(Token::Ampersand);
                if matches!(self.peek(), Token::Identifier(_)) {
                    let name_position = self.position;
                    self.advance();
                    self.record_named_parameter_at(name_position);
                }
                if *self.peek() == Token::BracketOpen {
                    self.advance();
                    while !matches!(self.peek(), Token::BracketClose | Token::EndOfFile) {
                        self.advance();
                    }
                    self.expect(Token::BracketClose)?;
                    self.last_cxx_pointer_depth =
                        self.last_cxx_pointer_depth.saturating_add(1).max(1);
                }
                parameters.push(self.take_cxx_type_identity(storage_type, is_reference));
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::ParenClose)?;
        Ok(CxxFunctionType::new(return_type, parameters, variadic))
    }

    /// Consume and register a block-scope function declaration such as
    /// `extern void F(float*);`. It has external linkage but declaration scope
    /// limited to the containing block; recording it in the current parser's
    /// free-function table gives later calls their C++ ABI name without
    /// materializing a file-scope definition.
    pub(crate) fn parse_block_function_prototype(
        &mut self,
        source_name: &str,
        return_type: Type,
    ) -> Compilation<()> {
        let return_identity = self.take_cxx_type_identity(return_type, false);
        let function_type = self.parse_cxx_function_type(return_identity)?;
        if self.cplusplus {
            let mangled = self.mangle_typed_free_function(
                source_name,
                &function_type.parameters,
                function_type.variadic,
            )?;
            let storage_parameters: Vec<Type> = function_type
                .parameters
                .iter()
                .map(|parameter| parameter.source_type)
                .collect();
            self.register_free_cxx_function(
                source_name,
                &mangled,
                &storage_parameters,
                &function_type.parameters,
                function_type.variadic,
            );
        }
        Ok(())
    }

    fn named_namespace_scopes(&self) -> Vec<&str> {
        self.namespace_stack
            .iter()
            .map(String::as_str)
            .filter(|scope| !scope.is_empty())
            .collect()
    }

    /// Resolve a namespace qualifier using ordinary enclosing-namespace
    /// lookup. Inside `Game::Baby`, an unqualified `EnemyFunc::call()` first
    /// probes `Game::Baby::EnemyFunc`, then `Game::EnemyFunc`, then the global
    /// namespace. Namespace calls need the resolved declaration scope both to
    /// select their overload set and to preserve the ABI's qualified mangling.
    pub(crate) fn resolve_scoped_cxx_namespace_name(&self, namespace: &str) -> Option<String> {
        if self.cxx_namespaces.contains(namespace) {
            return Some(namespace.to_owned());
        }
        let lexical_scopes = self.named_namespace_scopes();
        for depth in (0..=lexical_scopes.len()).rev() {
            let candidate = if depth == 0 {
                namespace.to_owned()
            } else {
                format!("{}::{namespace}", lexical_scopes[..depth].join("::"))
            };
            if self.cxx_namespaces.contains(&candidate) {
                return Some(candidate);
            }
        }
        // In-class inline bodies are reparsed as fully qualified out-of-class
        // definitions on an isolated parser. Their lexical namespace stack is
        // intentionally empty, but ordinary lookup still begins in the class's
        // enclosing namespace. Walk each enclosing component; non-namespace
        // components (for nested classes) naturally fail the membership test.
        if let Some(class) = self.current_cxx_member_class.as_deref() {
            let class_scopes = class.split("::").collect::<Vec<_>>();
            for depth in (0..class_scopes.len()).rev() {
                let candidate = if depth == 0 {
                    namespace.to_owned()
                } else {
                    format!("{}::{namespace}", class_scopes[..depth].join("::"))
                };
                if self.cxx_namespaces.contains(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn free_cxx_source_name(&self, function: &str) -> String {
        let scopes = self.named_namespace_scopes();
        if scopes.is_empty() {
            function.to_string()
        } else {
            format!("{}::{function}", scopes.join("::"))
        }
    }

    pub(crate) fn register_free_cxx_function(
        &mut self,
        source_name: &str,
        mangled: &str,
        parameters: &[Type],
        cxx_parameters: &[CxxParameterType],
        variadic: bool,
    ) {
        let key = self.free_cxx_source_name(source_name);
        let methods = self.cxx_free_functions.entry(key).or_default();
        if !methods.iter().any(|method| method.mangled == mangled) {
            methods.push(RecoveredCxxMethod {
                mangled: mangled.to_string(),
                fixed_parameter_count: parameters.len(),
                variadic,
                parameters: parameters.to_vec(),
                cxx_parameters: cxx_parameters.to_vec(),
            });
        }
    }

    pub(crate) fn register_qualified_free_cxx_function(
        &mut self,
        scope: &str,
        source_name: &str,
        mangled: &str,
        parameters: &[Type],
        cxx_parameters: &[CxxParameterType],
        variadic: bool,
    ) {
        let key = format!("{scope}::{source_name}");
        let methods = self.cxx_free_functions.entry(key).or_default();
        if !methods.iter().any(|method| method.mangled == mangled) {
            methods.push(RecoveredCxxMethod {
                mangled: mangled.to_string(),
                fixed_parameter_count: parameters.len(),
                variadic,
                parameters: parameters.to_vec(),
                cxx_parameters: cxx_parameters.to_vec(),
            });
        }
    }

    pub(crate) fn resolve_free_cxx_call(
        &self,
        source_name: &str,
        arguments: &[Expression],
    ) -> Compilation<Option<String>> {
        let scopes = self.named_namespace_scopes();
        let Some(key) = (0..=scopes.len()).rev().find_map(|depth| {
            let candidate = if depth == 0 {
                source_name.to_owned()
            } else {
                format!("{}::{source_name}", scopes[..depth].join("::"))
            };
            self.cxx_free_functions
                .contains_key(&candidate)
                .then_some(candidate)
        }) else {
            return Ok(None);
        };
        let candidates: Vec<&RecoveredCxxMethod> = self
            .cxx_free_functions
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|method| {
                method.fixed_parameter_count == arguments.len()
                    || (method.variadic && arguments.len() >= method.fixed_parameter_count)
            })
            .collect();
        match candidates.as_slice() {
            [] => Ok(None),
            [method] => Ok(Some(method.mangled.clone())),
            _ => self.resolve_exact_cxx_overload(&key, &candidates, arguments),
        }
    }

    /// Resolve a bare free-function name used as a value (typically a callback
    /// argument). Without an expected function-pointer signature, only a single
    /// recovered overload is unambiguous; overloaded names remain unresolved.
    pub(crate) fn resolve_free_cxx_function_address(&self, source_name: &str) -> Option<String> {
        let scopes = self.named_namespace_scopes();
        let key = (0..=scopes.len()).rev().find_map(|depth| {
            let candidate = if depth == 0 {
                source_name.to_owned()
            } else {
                format!("{}::{source_name}", scopes[..depth].join("::"))
            };
            self.cxx_free_functions
                .contains_key(&candidate)
                .then_some(candidate)
        })?;
        let candidates = self.cxx_free_functions.get(&key)?;
        match candidates.as_slice() {
            [method] => Some(method.mangled.clone()),
            _ => None,
        }
    }

    /// Resolve a source-level name used as an initializer address to its ABI
    /// symbol. Function and data addresses share this boundary so scalar
    /// pointer initializers and relocated aggregate fields cannot disagree.
    pub(crate) fn resolve_cxx_initializer_address(&self, source_name: &str) -> Option<String> {
        self.resolve_free_cxx_function_address(source_name)
            .or_else(|| self.resolve_cxx_data_object(source_name))
    }

    pub(crate) fn resolve_qualified_free_cxx_call(
        &self,
        scope: &str,
        source_name: &str,
        arguments: &[Expression],
    ) -> Compilation<Option<String>> {
        let key = format!("{scope}::{source_name}");
        let candidates: Vec<&RecoveredCxxMethod> = self
            .cxx_free_functions
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|method| {
                method.fixed_parameter_count == arguments.len()
                    || (method.variadic && arguments.len() >= method.fixed_parameter_count)
            })
            .collect();
        match candidates.as_slice() {
            [] => Ok(None),
            [method] => Ok(Some(method.mangled.clone())),
            _ => self.resolve_exact_cxx_overload(&key, &candidates, arguments),
        }
    }

    /// Resolve an arity collision only when every argument's source type is
    /// recoverable and selects exactly one declaration. This intentionally
    /// implements exact matches before C++ conversion ranking; unresolved
    /// promotion/conversion cases continue to defer instead of guessing.
    fn resolve_exact_cxx_overload(
        &self,
        key: &str,
        candidates: &[&RecoveredCxxMethod],
        arguments: &[Expression],
    ) -> Compilation<Option<String>> {
        let exact: Vec<_> = candidates
            .iter()
            .filter(|method| {
                self.cxx_arguments_exactly_match(
                    &method.parameters,
                    &method.cxx_parameters,
                    method.variadic,
                    arguments,
                )
            })
            .collect();
        match exact.as_slice() {
            [method] => Ok(Some(method.mangled.clone())),
            _ => Err(Diagnostic::error(format!(
                "C++ function call '{key}' is ambiguous (roadmap)"
            ))),
        }
    }

    /// Whether every modeled argument exactly matches a candidate declaration.
    /// Aggregate identity is stronger than storage type: both `Creature*` and
    /// `Vector3f&` occupy one address word, but only the qualified source type
    /// can select the correct overload. Unknown argument types deliberately do
    /// not disqualify a candidate, so an unresolved tie still defers safely.
    fn cxx_arguments_exactly_match(
        &self,
        parameters: &[Type],
        cxx_parameters: &[CxxParameterType],
        variadic: bool,
        arguments: &[Expression],
    ) -> bool {
        arguments.iter().enumerate().all(|(index, argument)| {
            let Some(parameter) = cxx_parameters.get(index) else {
                return variadic;
            };
            if let (Some(expected), Some(actual)) = (
                parameter.qualified_name.as_deref(),
                self.cxx_expression_struct_tag(argument),
            ) {
                return expected == actual
                    || expected.rsplit("::").next() == actual.rsplit("::").next();
            }
            self.cxx_expression_type(argument)
                .is_none_or(|actual| parameters.get(index) == Some(&actual))
        })
    }

    pub(crate) fn cxx_expression_type(&self, expression: &Expression) -> Option<Type> {
        match expression {
            Expression::Variable(name) => self
                .variable_types
                .get(name)
                .or_else(|| self.global_types.get(name))
                .copied(),
            Expression::IntegerLiteral(_) => Some(Type::Int),
            Expression::FloatLiteral(_) => Some(Type::Float),
            Expression::Cast { target_type, .. } => Some(*target_type),
            Expression::Member { member_type, .. } => Some(*member_type),
            Expression::Dereference { pointer } | Expression::Index { base: pointer, .. } => {
                match self.cxx_expression_type(pointer)? {
                    Type::Pointer(pointee) => Some(pointee.element()),
                    Type::StructPointer { element_size } => Some(Type::Struct {
                        size: element_size,
                        align: 1,
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn cxx_expression_struct_tag<'a>(
        &'a self,
        expression: &'a Expression,
    ) -> Option<&'a str> {
        match expression {
            Expression::Variable(name) => self
                .variable_structs
                .get(name)
                .or_else(|| self.global_structs.get(name))
                .map(String::as_str),
            Expression::AddressOf { operand }
            | Expression::Cast { operand, .. }
            | Expression::Dereference { pointer: operand } => {
                self.cxx_expression_struct_tag(operand)
            }
            _ => None,
        }
    }

    pub(crate) fn qualify_cxx_class_name(&self, class: &str) -> String {
        let scopes = self.named_namespace_scopes();
        if scopes.is_empty() {
            class.to_string()
        } else {
            format!("{}::{class}", scopes.join("::"))
        }
    }

    /// Resolve a class name from the innermost active namespace outward. Large
    /// game headers reuse names such as `Obj` in many sibling namespaces;
    /// whichever declaration appeared first must not decide every later
    /// `Obj*`. Qualified-but-relative names use the same enclosing-scope walk.
    pub(crate) fn resolve_scoped_cxx_class_name(&self, class: &str) -> Option<String> {
        let declared = |qualified: &str| {
            self.cxx_dispatch_tables.contains_key(qualified)
                || self.cxx_classes.contains_key(qualified)
                || self.structs.contains_key(qualified)
        };
        if let Some(layout_scope) = &self.current_cxx_layout_scope {
            // C++ injects the class name into its own body before the layout is
            // complete and registered. Self-referential pointers therefore
            // resolve against the in-progress lexical scope.
            if layout_scope.rsplit("::").next() == Some(class) {
                return Some(layout_scope.clone());
            }
            let components: Vec<&str> = layout_scope.split("::").collect();
            for depth in (1..=components.len()).rev() {
                let qualified = format!("{}::{class}", components[..depth].join("::"));
                if declared(&qualified) {
                    return Some(qualified);
                }
            }
        }
        // An out-of-class member definition retains its lexical class scope.
        // This covers both the injected class name (`Inner* p` inside
        // `Outer::Inner::method`) and sibling nested types declared by an owner.
        if let Some(member_scope) = &self.current_cxx_member_class {
            let components: Vec<&str> = member_scope.split("::").collect();
            for depth in (1..=components.len()).rev() {
                let qualified = format!("{}::{class}", components[..depth].join("::"));
                if declared(&qualified) {
                    return Some(qualified);
                }
            }
        }
        let scopes = self.named_namespace_scopes();
        for depth in (0..=scopes.len()).rev() {
            let qualified = if depth == 0 {
                class.to_owned()
            } else {
                format!("{}::{class}", scopes[..depth].join("::"))
            };
            if declared(&qualified) {
                return Some(qualified);
            }
        }
        None
    }

    /// Whether the cursor begins an exactly declared qualified aggregate or
    /// enum type. The first component alone is insufficient: `Class::member`
    /// is an expression, while `Class::Nested` or `Namespace::Type` can begin a
    /// cast/sizeof type-id. Resolve the complete chain to preserve that
    /// distinction without teaching the general item parser C++ name lookup.
    pub(crate) fn peek_is_qualified_cxx_type(&self) -> bool {
        if !self.cplusplus
            || !matches!(self.peek(), Token::Identifier(_))
            || *self.peek_at(1) != Token::Colon
            || *self.peek_at(2) != Token::Colon
        {
            return false;
        }
        let mut scan = self.position;
        let mut components = Vec::new();
        if let Some(Token::Identifier(first)) = self.tokens.get(scan) {
            components.push(first.clone());
            scan += 1;
        }
        while self.tokens.get(scan) == Some(&Token::Colon)
            && self.tokens.get(scan + 1) == Some(&Token::Colon)
        {
            let Some(Token::Identifier(component)) = self.tokens.get(scan + 2) else {
                break;
            };
            components.push(component.clone());
            scan += 3;
        }
        if self.tokens.get(scan) == Some(&Token::ParenOpen) {
            // `Qualified::Type()` is value construction. It begins with a
            // known type name but is an expression at this cursor, not a
            // declaration or a parenthesized cast type-id.
            return false;
        }
        let qualified = components.join("::");
        self.resolve_scoped_cxx_class_name(&qualified).is_some()
            || self.enum_types.contains_key(&qualified)
            || self
                .struct_typedefs
                .values()
                .any(|mapped| mapped == &qualified)
    }

    /// Recover declaration semantics from a C++ aggregate independently of
    /// layout parsing. Methods defined in a class body are implicitly inline;
    /// declarations carrying `inline` remain inline when a later out-of-class
    /// definition omits the keyword. Static method declarations supply callable
    /// signatures and their source-order prototype symbols.
    ///
    /// This deliberately never infers layout. Calls to skipped inline names still
    /// defer, while a static call is admitted only when one recovered overload
    /// matches its arity.
    pub(crate) fn capture_cxx_class_declarations(&mut self) -> Vec<(String, Type, Vec<Type>)> {
        if !self.cplusplus {
            return Vec::new();
        }
        let start = self.position;
        let follows_typedef = matches!(self.tokens.get(start + 1), Some(Token::KeywordStruct))
            || matches!(self.tokens.get(start + 1), Some(Token::Identifier(word)) if word == "class");
        let aggregate_start = if matches!(self.tokens.get(start), Some(Token::Identifier(word)) if word == "typedef")
            && follows_typedef
        {
            start + 1
        } else {
            start
        };
        let is_aggregate = matches!(self.tokens.get(aggregate_start), Some(Token::KeywordStruct))
            || matches!(self.tokens.get(aggregate_start), Some(Token::Identifier(word)) if word == "class");
        if !is_aggregate {
            return Vec::new();
        }
        let Some(Token::Identifier(source_class)) = self.tokens.get(aggregate_start + 1) else {
            return Vec::new();
        };
        let source_class = source_class.clone();
        let class = self.qualify_cxx_class_name(&source_class);
        // In C++, the class tag is also an ordinary type name. Preserve that
        // fact even when layout recovery later rejects the body, so pointers to
        // the class retain their semantic tag.
        self.struct_typedefs
            .entry(source_class.clone())
            .or_insert_with(|| class.clone());
        let mut index = aggregate_start + 2;
        while !matches!(
            self.tokens.get(index),
            Some(Token::BraceOpen | Token::Semicolon | Token::EndOfFile) | None
        ) {
            index += 1;
        }
        if self.tokens.get(index) != Some(&Token::BraceOpen) {
            return Vec::new();
        }
        self.capture_cxx_class_layout(aggregate_start, &class);
        self.cxx_inline_ordinal_facts.class_definitions += 1;

        // Retain the primary-base identity independently from virtual-table
        // recovery. In ordinary multiple inheritance the first base begins at
        // offset zero, so an inherited direct call needs no `this` adjustment.
        // Secondary-base calls and inherited virtual dispatch still defer.
        let header = &self.tokens[aggregate_start + 2..index];
        let mut dispatch = RecoveredCxxDispatchTable::default();
        let mut inherits_virtual_destructor = false;
        if let Some(colon) = header.iter().position(|token| *token == Token::Colon) {
            let inheritance = &header[colon + 1..];
            let multiple = inheritance.iter().any(|token| token == &Token::Comma);
            let virtual_base = inheritance.iter().any(
                |token| matches!(token, Token::Identifier(word) if word == "virtual"),
            );
            let base = inheritance.iter().find_map(|token| match token {
                Token::Identifier(word)
                    if !matches!(word.as_str(), "public" | "private" | "protected") =>
                {
                    Some(word.as_str())
                }
                _ => None,
            });
            if let Some(base) = base {
                let qualified_base = self
                    .resolve_scoped_cxx_class_name(base)
                    .unwrap_or_else(|| self.qualify_cxx_class_name(base));
                inherits_virtual_destructor = self
                    .cxx_virtual_destructor_classes
                    .contains(&qualified_base)
                    || self.cxx_virtual_destructor_classes.contains(base);
                if !virtual_base {
                    self.cxx_primary_bases
                        .insert(class.clone(), qualified_base.clone());
                }
                if multiple || virtual_base {
                    self.incomplete_cxx_dispatch.insert(class.clone());
                } else {
                    if let Some(base_dispatch) = self
                        .cxx_dispatch_tables
                        .get(&qualified_base)
                        .or_else(|| self.cxx_dispatch_tables.get(base))
                    {
                        dispatch = base_dispatch.clone();
                    } else {
                        self.incomplete_cxx_dispatch.insert(class.clone());
                    }
                }
            }
        }
        self.cxx_dispatch_tables.insert(class.clone(), dispatch);

        index += 1;
        let body_start = index;
        let mut prototypes = Vec::new();
        let mut brace_depth = 1i32;
        let mut paren_depth = 0i32;
        let mut explicitly_inline = false;
        let mut member_name: Option<String> = None;
        let mut member_parameter_count: Option<usize> = None;
        let mut member_declaration_start = body_start;
        let mut inline_body = None;
        while index < self.tokens.len() {
            if brace_depth == 1
                && paren_depth == 0
                && self.tokens.get(index) == Some(&Token::ParenOpen)
                && crate::parameter_names::could_be_parameter_list(&self.tokens, index)
            {
                if let Some((_, positions)) = crate::parameter_names::positions(&self.tokens, index)
                {
                    for position in positions {
                        self.record_named_parameter_at(position);
                    }
                }
                let mut parameter_probe = self.clone();
                parameter_probe.position = index;
                if let Ok(signature) = parameter_probe.parse_class_parameter_types() {
                    member_parameter_count = Some(signature.parameters.len());
                    self.merge_named_parameter_positions_from(&parameter_probe);
                }
            }
            let Some(token) = self.tokens.get(index) else {
                break;
            };
            let begins_member = brace_depth == 1
                && paren_depth == 0
                && (index == body_start
                    || matches!(
                        self.tokens.get(index.wrapping_sub(1)),
                        Some(Token::Semicolon | Token::BraceClose)
                    )
                    || (matches!(self.tokens.get(index.wrapping_sub(1)), Some(Token::Colon))
                        && matches!(self.tokens.get(index.wrapping_sub(2)), Some(Token::Identifier(access)) if matches!(access.as_str(), "public" | "private" | "protected"))));
            if brace_depth == 1 && paren_depth == 0 {
                if matches!(token, Token::Identifier(word) if word == "inline" || word == "__inline")
                {
                    explicitly_inline = true;
                }
                if token == &Token::ParenOpen {
                    member_name = index
                        .checked_sub(1)
                        .and_then(|previous| self.tokens.get(previous))
                        .and_then(|previous| match previous {
                            Token::Identifier(name) => Some(canonical_inline_member_name(
                                &class,
                                name,
                                index.checked_sub(2).and_then(|before_name| {
                                    self.tokens.get(before_name)
                                }) == Some(&Token::Tilde),
                            )),
                            _ => None,
                        });
                    if member_name.is_none()
                        && self.tokens[member_declaration_start..index].iter().any(
                            |token| matches!(token, Token::Identifier(word) if word == "operator"),
                        )
                    {
                        member_name = Some("operator".to_string());
                    }
                }
            }
            if begins_member {
                member_declaration_start = index;
                let is_access_label = matches!(token, Token::Identifier(access)
                    if matches!(access.as_str(), "public" | "private" | "protected"))
                    && self.tokens.get(index + 1) == Some(&Token::Colon);
                if !is_access_label {
                    if let Some((explicitly_virtual, is_destructor)) =
                        function_declaration_virtuality(&self.tokens, index)
                    {
                        let is_virtual = explicitly_virtual
                            || (is_destructor && inherits_virtual_destructor);
                        if is_virtual {
                            if is_destructor {
                                self.cxx_inline_ordinal_facts
                                    .virtual_destructor_declarations += 1;
                                if inherits_virtual_destructor {
                                    self.cxx_inline_ordinal_facts
                                        .inherited_virtual_destructor_declarations += 1;
                                }
                                self.cxx_virtual_destructor_classes
                                    .insert(class.clone());
                            } else {
                                self.cxx_inline_ordinal_facts
                                    .virtual_method_declarations += 1;
                            }
                        }
                    }
                }
            }
            let nested_class = if !begins_member {
                None
            } else if matches!(token, Token::Identifier(word) if word == "class")
                || token == &Token::KeywordStruct
            {
                Some(index)
            } else if matches!(token, Token::Identifier(word) if word == "typedef")
                && (self.tokens.get(index + 1) == Some(&Token::KeywordStruct)
                    || matches!(self.tokens.get(index + 1), Some(Token::Identifier(word)) if word == "class"))
            {
                Some(index + 1)
            } else {
                None
            };
            let nested_enum =
                begins_member && matches!(token, Token::Identifier(word) if word == "enum");
            match token {
                Token::ParenOpen if brace_depth == 1 => paren_depth += 1,
                Token::ParenClose if brace_depth == 1 && paren_depth > 0 => paren_depth -= 1,
                Token::BraceOpen => {
                    // A brace following a class-scope parameter list is the
                    // method body, hence implicitly inline.
                    if brace_depth == 1 && paren_depth == 0 {
                        if let Some(member) = member_name.take() {
                            self.inline_cxx_members.insert((class.clone(), member));
                            self.cxx_inline_ordinal_facts.inline_definitions += 1;
                            self.cxx_inline_ordinal_facts.inline_definition_parameters +=
                                member_parameter_count.unwrap_or(0);
                            let declaration = &self.tokens[member_declaration_start..index];
                            let is_virtual = declaration.iter().any(
                                |token| matches!(token, Token::Identifier(word) if word == "virtual"),
                            );
                            if declaration.iter().any(|token| token == &Token::Tilde) {
                                if is_virtual {
                                    self.cxx_inline_ordinal_facts.virtual_destructors += 1;
                                } else {
                                    self.cxx_inline_ordinal_facts.nonvirtual_destructors += 1;
                                    self.cxx_nonvirtual_destructor_classes
                                        .insert(source_class.clone());
                                }
                            }
                            inline_body = Some((member_declaration_start, index + 1));
                        }
                    }
                    brace_depth += 1;
                }
                Token::BraceClose => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        return prototypes;
                    }
                    if brace_depth == 1 {
                        if let Some((declaration_start, body_start)) = inline_body.take() {
                            self.cxx_inline_ordinal_facts
                                .inline_definition_local_declarators +=
                                crate::inline_body_analysis::local_declarators(
                                    &self.tokens,
                                    body_start - 1,
                                );
                            self.cxx_inline_ordinal_facts.control_flow_labels +=
                                inline_control_flow_labels(&self.tokens[body_start..index]);
                            self.cxx_inline_ordinal_facts.direct_calls += self.tokens
                                [body_start..index]
                                .windows(2)
                                .filter(|tokens| {
                                    matches!(tokens[0], Token::Identifier(_))
                                        && tokens[1] == Token::ParenOpen
                                })
                                .count();
                            self.cxx_temporary_construction_targets.extend(
                                self.tokens[body_start..index]
                                    .windows(2)
                                    .filter_map(|tokens| match &tokens[0] {
                                        Token::Identifier(target)
                                            if tokens[1] == Token::ParenOpen
                                                && (target == &source_class
                                                    || self.struct_typedefs.contains_key(target)) =>
                                        {
                                            Some(target.clone())
                                        }
                                        _ => None,
                                    }),
                            );
                            self.capture_cxx_inline_definition(
                                declaration_start,
                                index,
                                &class,
                            );
                        }
                        explicitly_inline = false;
                        member_name = None;
                        member_parameter_count = None;
                    }
                }
                Token::Semicolon if brace_depth == 1 && paren_depth == 0 => {
                    if explicitly_inline {
                        if let Some(member) = member_name.take() {
                            self.inline_cxx_members.insert((class.clone(), member));
                        }
                    }
                    explicitly_inline = false;
                    member_name = None;
                    member_parameter_count = None;
                }
                Token::EndOfFile => return prototypes,
                _ => {}
            }
            if let Some(nested_class) = nested_class {
                let nested = nested_explicit_virtual_declarations(
                    &self.tokens,
                    nested_class,
                    &mut self.counted_nested_virtual_positions,
                );
                self.cxx_inline_ordinal_facts.virtual_method_declarations += nested.0;
                self.cxx_inline_ordinal_facts
                    .virtual_destructor_declarations += nested.1;
                self.capture_nested_cxx_inline_facts(nested_class);
                self.capture_nested_cxx_class_layout(nested_class, &class);
            }
            if nested_enum {
                self.capture_nested_cxx_enum(index, &class);
            }
            if begins_member {
                self.capture_cxx_member_template_forwarder(index, &class);
                self.capture_cxx_constructor(index, &class);
                let starts_virtual = matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "virtual");
                match self.capture_cxx_method(index, &class) {
                    Some(Some(prototype)) => prototypes.push(prototype),
                    Some(None) => {}
                    None if starts_virtual => {
                        // A destructor, pointer-to-member signature, or another
                        // unmodeled virtual declaration may consume a slot. Refuse
                        // every virtual call through the class until it is modeled.
                        self.incomplete_cxx_dispatch.insert(class.clone());
                    }
                    None => {}
                }
            }
            index += 1;
        }
        prototypes
    }

    /// Reuse the ordinary class analysis for a nested definition, but merge
    /// only syntax facts. Layout and callable recovery remain owned by the
    /// containing parser, while the frontend's dropped-inline timeline still
    /// observes methods defined inside the nested class.
    fn capture_nested_cxx_inline_facts(&mut self, index: usize) {
        let mut probe = self.clone();
        probe.position = index;
        probe.cxx_inline_ordinal_facts = mwcc_syntax_trees::CxxInlineOrdinalFacts::default();
        probe.cxx_temporary_construction_targets.clear();
        let _ = probe.capture_cxx_class_declarations();
        let facts = probe.cxx_inline_ordinal_facts;
        self.cxx_inline_ordinal_facts.class_definitions += facts.class_definitions;
        self.cxx_inline_ordinal_facts.inline_definitions += facts.inline_definitions;
        self.cxx_inline_ordinal_facts.inline_definition_parameters +=
            facts.inline_definition_parameters;
        self.cxx_inline_ordinal_facts
            .inline_definition_local_declarators += facts.inline_definition_local_declarators;
        self.cxx_inline_ordinal_facts.nonvirtual_destructors += facts.nonvirtual_destructors;
        self.cxx_inline_ordinal_facts.virtual_destructors += facts.virtual_destructors;
        self.cxx_inline_ordinal_facts.direct_calls += facts.direct_calls;
        self.cxx_inline_ordinal_facts.control_flow_labels += facts.control_flow_labels;
        self.cxx_temporary_construction_targets
            .extend(probe.cxx_temporary_construction_targets);
        self.cxx_nonvirtual_destructor_classes
            .extend(probe.cxx_nonvirtual_destructor_classes);
    }

    /// Recover one constructor declaration without requiring the containing
    /// class's complete field/method layout. This is intentionally signature
    /// only; an in-class body is retained separately by
    /// `capture_cxx_inline_definition`.
    fn capture_cxx_constructor(&mut self, declaration_index: usize, class: &str) {
        let Some(source_name) = class.rsplit("::").next() else {
            return;
        };
        if !matches!(self.tokens.get(declaration_index), Some(Token::Identifier(name)) if name == source_name)
            || self.tokens.get(declaration_index + 1) != Some(&Token::ParenOpen)
        {
            return;
        }
        let mut probe = self.clone();
        probe.position = declaration_index + 1;
        let Ok(signature) = probe.parse_class_parameter_types() else {
            return;
        };
        let Ok(is_inline) = probe.skip_class_method_tail() else {
            return;
        };
        self.merge_named_parameter_positions_from(&probe);
        let scopes: Vec<&str> = class.split("::").collect();
        let Ok(mangled) = mangle_qualified_member_function_typed(
            &scopes,
            "__ct",
            &signature.cxx_parameters,
        ) else {
            return;
        };
        let method = RecoveredCxxMethod {
            mangled: mangled.clone(),
            fixed_parameter_count: signature.parameters.len(),
            variadic: false,
            parameters: signature.parameters,
            cxx_parameters: signature.cxx_parameters,
        };
        let methods = self.cxx_constructors.entry(class.to_owned()).or_default();
        if !methods.iter().any(|existing| existing.mangled == mangled) {
            methods.push(method);
        }
        if is_inline {
            self.skipped_inline_names.insert(mangled);
        }
    }

    /// Parse an in-class inline body through the ordinary out-of-class member
    /// definition path on an isolated token stream. The recovered function is
    /// analysis-only: it supplies verified semantics to inline summaries and is
    /// never appended to the translation unit's emitted definitions.
    fn capture_cxx_inline_definition(
        &mut self,
        declaration_start: usize,
        body_end: usize,
        class: &str,
    ) {
        let mut source: Vec<(Token, mwcc_tokens::SourceLocation)> = self.tokens
            [declaration_start..=body_end]
            .iter()
            .cloned()
            .zip(self.locations[declaration_start..=body_end].iter().copied())
            .collect();
        let is_virtual = source.iter().any(
            |(token, _)| matches!(token, Token::Identifier(word) if word == "virtual"),
        );
        while matches!(source.first(), Some((Token::Identifier(word), _)) if matches!(word.as_str(), "virtual" | "explicit" | "inline"))
        {
            source.remove(0);
        }
        let Some(parameter_open) = source
            .iter()
            .position(|(token, _)| *token == Token::ParenOpen)
        else {
            return;
        };
        let Some(mut member_index) = parameter_open.checked_sub(1) else {
            return;
        };
        let Some((Token::Identifier(member_name), _)) = source.get(member_index) else {
            return;
        };
        let member_name = member_name.clone();
        let destructor = member_index > 0
            && source.get(member_index - 1).is_some_and(|(token, _)| *token == Token::Tilde);
        let constructor = !destructor
            && member_index == 0
            && class
                .rsplit("::")
                .next()
                .is_some_and(|name| name == member_name);
        let location = source[member_index].1;
        if destructor {
            source.remove(member_index - 1);
            member_index -= 1;
            source[member_index].0 = Token::Identifier("__dt".to_string());
        }
        let mut qualification = Vec::new();
        for (index, component) in class.split("::").enumerate() {
            if index > 0 {
                qualification.push((Token::Colon, location));
                qualification.push((Token::Colon, location));
            }
            qualification.push((Token::Identifier(component.to_owned()), location));
        }
        qualification.push((Token::Colon, location));
        qualification.push((Token::Colon, location));
        source.splice(member_index..member_index, qualification);
        if constructor || destructor {
            source.insert(0, (Token::KeywordVoid, location));
        }
        let eof_location = source.last().map_or(location, |(_, location)| *location);
        source.push((Token::EndOfFile, eof_location));

        let (tokens, locations): (Vec<_>, Vec<_>) = source.into_iter().unzip();
        let mut probe = self.clone();
        probe.tokens = tokens;
        probe.locations = locations;
        probe.position = 0;
        probe.namespace_stack.clear();
        probe.recover_skipped_inline_definition = true;
        let mut globals = Vec::new();
        let mut functions = Vec::new();
        let mut prototypes = Vec::new();
        let parsed = probe.parse_top_level_item(&mut globals, &mut functions, &mut prototypes);
        if parsed.is_ok() && functions.len() == 1 {
            self.merge_generated_inline_definitions_from(&probe);
            let source = probe.function_sources.pop().flatten();
            let mut function = functions.pop().expect("length checked");
            if matches!(function.return_type, Type::Struct { .. }) {
                let return_tag = self
                    .cxx_classes
                    .get(class)
                    .and_then(|layout| layout.methods.get(&member_name))
                    .into_iter()
                    .flatten()
                    .find_map(|method| method.return_struct_tag.clone());
                if let Some(return_tag) = return_tag {
                    self.function_return_structs
                        .insert(function.name.clone(), return_tag);
                }
            }
            if destructor || is_virtual {
                function.is_weak = true;
                if !self
                    .cxx_inline_materializations
                    .iter()
                    .any(|existing| existing.name == function.name)
                {
                    self.cxx_inline_materializations.push(function.clone());
                    if let Some(source) = source {
                        self.cxx_inline_materialization_sources
                            .insert(function.name.clone(), source);
                    }
                }
            }
            self.skipped_inline_names.insert(function.name.clone());
            if !self
                .skipped_inline_definitions
                .iter()
                .any(|existing| existing.name == function.name)
            {
                self.skipped_inline_definitions.push(function);
            }
        } else if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some()
            || std::env::var("MWCC_CAPTURE_INLINE")
                .ok()
                .is_some_and(|needle| member_name.contains(&needle))
        {
            eprintln!(
                "failed to retain inline definition for {class}::{member_name}: {parsed:?}; recovered functions: {}",
                functions.len()
            );
        }
    }

    /// Keep compiler-generated constructors discovered while parsing an inline
    /// body on an isolated parser. The probe begins as a clone of this parser,
    /// so merge by function identity instead of copying its whole definition
    /// pool back. Without this step a retained outer constructor can call a
    /// synthesized member constructor whose body exists only in the probe.
    pub(crate) fn merge_generated_inline_definitions_from(&mut self, probe: &Parser) {
        for function in &probe.skipped_inline_definitions {
            if self
                .skipped_inline_definitions
                .iter()
                .any(|existing| existing.name == function.name)
            {
                continue;
            }
            self.skipped_inline_names.insert(function.name.clone());
            self.skipped_inline_definitions.push(function.clone());
        }
    }

    /// Recover a namespace-qualified layout without coupling declaration
    /// capture to the main top-level parser. The latter historically stores C
    /// `struct` tags by their terminal spelling; keeping this qualified copy
    /// prevents sibling namespaces' identically named classes from aliasing.
    fn capture_cxx_class_layout(&mut self, declaration_index: usize, qualified: &str) {
        let saved_position = self.position;
        let saved_struct_tag = self.last_struct_tag.clone();
        let saved_enum_tag = self.last_enum_tag.clone();
        let saved_wchar = self.last_type_was_wchar;
        let saved_array_typedef = self.last_array_typedef;
        let saved_type_const = self.last_type_was_const;
        let saved_pointer_const = self.last_pointer_const;
        let saved_cxx_pointer_depth = self.last_cxx_pointer_depth;
        let saved_cxx_pointer_base = self.last_cxx_pointer_base;
        let saved_volatile = self.last_type_was_volatile;

        self.position = declaration_index;
        match self.parse_class_definition() {
            Ok((source_name, layout, class)) => {
                self.struct_typedefs
                    .entry(source_name)
                    .or_insert_with(|| qualified.to_owned());
                self.structs.insert(qualified.to_owned(), layout);
                if !self.cxx_classes.contains_key(qualified) {
                    self.cxx_class_declaration_order
                        .push(qualified.to_owned());
                }
                self.cxx_classes.insert(qualified.to_owned(), class);
            }
            Err(error) if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some() => {
                let start = self.position.saturating_sub(24);
                let end = (self.position + 8).min(self.tokens.len());
                eprintln!(
                    "class layout recovery failed in '{qualified}' at token {}: {error}; context {:?}",
                    self.position,
                    &self.tokens[start..end]
                );
            }
            Err(_) => {}
        }

        self.position = saved_position;
        self.last_struct_tag = saved_struct_tag;
        self.last_enum_tag = saved_enum_tag;
        self.last_type_was_wchar = saved_wchar;
        self.last_array_typedef = saved_array_typedef;
        self.last_type_was_const = saved_type_const;
        self.last_pointer_const = saved_pointer_const;
        self.last_cxx_pointer_depth = saved_cxx_pointer_depth;
        self.last_cxx_pointer_base = saved_cxx_pointer_base;
        self.last_type_was_volatile = saved_volatile;
    }

    /// Recover a directly nested class even when the enclosing class is too
    /// large for the ordinary layout parser. Out-of-class definitions retain
    /// the full `Outer::Inner` scope, so registering that qualified layout lets
    /// unqualified fields in their bodies lower to `this->field` normally.
    fn capture_nested_cxx_class_layout(&mut self, declaration_index: usize, outer: &str) {
        let saved_position = self.position;
        let saved_struct_tag = self.last_struct_tag.clone();
        let saved_enum_tag = self.last_enum_tag.clone();
        let saved_wchar = self.last_type_was_wchar;
        let saved_array_typedef = self.last_array_typedef;
        let saved_type_const = self.last_type_was_const;
        let saved_pointer_const = self.last_pointer_const;
        let saved_cxx_pointer_depth = self.last_cxx_pointer_depth;
        let saved_cxx_pointer_base = self.last_cxx_pointer_base;
        let saved_volatile = self.last_type_was_volatile;

        self.position = declaration_index;
        match self.parse_class_definition() {
            Ok((nested, layout, class)) => {
                let qualified = format!("{outer}::{nested}");
                self.struct_typedefs.insert(nested, qualified.clone());
                self.structs.insert(qualified.clone(), layout);
                if !self.cxx_classes.contains_key(&qualified) {
                    self.cxx_class_declaration_order.push(qualified.clone());
                }
                self.cxx_classes.insert(qualified, class);
            }
            Err(error) if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some() => {
                eprintln!("nested-class layout recovery failed in '{outer}': {error}");
            }
            Err(_) => {}
        }

        self.position = saved_position;
        self.last_struct_tag = saved_struct_tag;
        self.last_enum_tag = saved_enum_tag;
        self.last_type_was_wchar = saved_wchar;
        self.last_array_typedef = saved_array_typedef;
        self.last_type_was_const = saved_type_const;
        self.last_pointer_const = saved_pointer_const;
        self.last_cxx_pointer_depth = saved_cxx_pointer_depth;
        self.last_cxx_pointer_base = saved_cxx_pointer_base;
        self.last_type_was_volatile = saved_volatile;
    }

    /// Register a nested enum independently of the enclosing class's layout.
    /// Qualified method signatures can then retain `Outer::Enum` even when an
    /// unrelated field prevents the outer aggregate from being laid out.
    fn capture_nested_cxx_enum(&mut self, declaration_index: usize, outer: &str) {
        let saved_position = self.position;
        let saved_struct_tag = self.last_struct_tag.clone();
        let saved_enum_tag = self.last_enum_tag.clone();
        let saved_wchar = self.last_type_was_wchar;
        let saved_array_typedef = self.last_array_typedef;
        let saved_type_const = self.last_type_was_const;
        let saved_pointer_const = self.last_pointer_const;
        let saved_cxx_pointer_depth = self.last_cxx_pointer_depth;
        let saved_cxx_pointer_base = self.last_cxx_pointer_base;
        let saved_volatile = self.last_type_was_volatile;

        self.position = declaration_index;
        if let Ok(storage) = self.parse_type() {
            if let Some(name) = self.last_enum_tag.clone() {
                self.enum_types.insert(format!("{outer}::{name}"), storage);
            }
        }

        self.position = saved_position;
        self.last_struct_tag = saved_struct_tag;
        self.last_enum_tag = saved_enum_tag;
        self.last_type_was_wchar = saved_wchar;
        self.last_array_typedef = saved_array_typedef;
        self.last_type_was_const = saved_type_const;
        self.last_pointer_const = saved_pointer_const;
        self.last_cxx_pointer_depth = saved_cxx_pointer_depth;
        self.last_cxx_pointer_base = saved_cxx_pointer_base;
        self.last_type_was_volatile = saved_volatile;
    }

    /// Speculatively reuse the ordinary type/declarator parser on one class
    /// method declaration. The main cursor and transient type side channels are
    /// restored regardless of success; fields, constructors, definitions, and
    /// unsupported reference-valued signatures simply produce no result.
    fn capture_cxx_method(
        &mut self,
        declaration_index: usize,
        class: &str,
    ) -> Option<Option<(String, Type, Vec<Type>)>> {
        let saved_position = self.position;
        let saved_struct_tag = self.last_struct_tag.clone();
        let saved_enum_tag = self.last_enum_tag.clone();
        let saved_wchar = self.last_type_was_wchar;
        let saved_array_typedef = self.last_array_typedef;
        let saved_type_const = self.last_type_was_const;
        let saved_pointer_const = self.last_pointer_const;
        let saved_cxx_pointer_depth = self.last_cxx_pointer_depth;
        let saved_cxx_pointer_base = self.last_cxx_pointer_base;
        let saved_volatile = self.last_type_was_volatile;

        self.position = declaration_index;
        let recovered = (|| -> Compilation<(
            String,
            Type,
            Option<String>,
            Vec<Type>,
            Vec<CxxParameterType>,
            bool,
            bool,
            bool,
            bool,
            bool,
        )> {
            let mut is_static = false;
            let mut is_virtual = false;
            let mut is_inline = false;
            while let Token::Identifier(qualifier) = self.peek() {
                match qualifier.as_str() {
                    "static" => is_static = true,
                    "virtual" => is_virtual = true,
                    "inline" | "__inline" => is_inline = true,
                    _ => break,
                }
                self.advance();
            }
            let return_type = self.parse_type()?;
            let return_struct_tag = self.last_struct_tag.take();
            self.last_enum_tag.take();
            self.last_type_was_wchar = false;
            self.last_array_typedef.take();
            // A reference return uses pointer storage in the IR, while the `&`
            // remains a declarator token so ABI-aware type parsing can observe
            // it. Consume it before the member function name.
            self.eat_keyword(Token::Ampersand);
            let member = self.parse_identifier()?;
            self.expect(Token::ParenOpen)?;
            let mut parameters = Vec::new();
            let mut cxx_parameters = Vec::new();
            let mut variadic = false;
            if *self.peek() == Token::KeywordVoid && *self.peek_at(1) == Token::ParenClose {
                self.advance();
            } else {
                while *self.peek() != Token::ParenClose {
                    if matches!(
                        self.tokens.get(self.position..self.position + 3),
                        Some([Token::Dot, Token::Dot, Token::Dot])
                    ) {
                        self.position += 3;
                        variadic = true;
                        break;
                    }
                    let parameter_start = self.position;
                    let mut parameter_type = match self.parse_type() {
                        Ok(parameter_type) => parameter_type,
                        Err(_) if is_virtual => {
                            // Slot recovery needs declaration order and arity,
                            // not a complete value ABI. Preserve one opaque
                            // aggregate/reference parameter while skipping its
                            // spelling; a call using that overload can still be
                            // selected safely by arity.
                            self.position = parameter_start;
                            while !matches!(self.peek(), Token::Comma | Token::ParenClose | Token::EndOfFile) {
                                self.advance();
                            }
                            parameters.push(Type::StructPointer { element_size: 0 });
                            cxx_parameters.push(CxxParameterType::plain(
                                Type::StructPointer { element_size: 0 },
                            ));
                            if !self.eat_keyword(Token::Comma) {
                                break;
                            }
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    let struct_tag = self.last_struct_tag.take();
                    let enum_tag = self.last_enum_tag.take();
                    let is_wchar = self.last_type_was_wchar;
                    let source_is_aggregate_value = self.last_type_was_aggregate_reference;
                    let qualified_name = enum_tag.or_else(|| {
                        struct_tag.map(|tag| {
                            self.struct_typedefs.get(&tag).cloned().unwrap_or(tag)
                        })
                    });
                    self.last_array_typedef.take();
                    let pointee_const = self.last_type_was_const;
                    let pointer_const = self.last_pointer_const;
                    let pointer_depth = self.last_cxx_pointer_depth;
                    let pointer_base = self.last_cxx_pointer_base;
                    let function_type = self.last_cxx_function_type.take();
                    let is_reference = self.eat_keyword(Token::Ampersand);
                    let cxx_storage_type = parameter_type;
                    if is_reference {
                        parameter_type = Type::StructPointer { element_size: 0 };
                    }
                    if matches!(self.peek(), Token::Identifier(_)) {
                        let name_position = self.position;
                        self.advance();
                        self.record_named_parameter_at(name_position);
                    }
                    self.skip_cxx_default_argument()?;
                    cxx_parameters.push(
                        CxxParameterType::parsed(
                            cxx_storage_type,
                            qualified_name,
                            is_wchar,
                            is_reference,
                            source_is_aggregate_value,
                            pointee_const,
                            pointer_const,
                        )
                        .with_pointer_shape(pointer_depth, pointer_base)
                        .with_function_type(function_type),
                    );
                    parameters.push(parameter_type);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;
            let mut is_const_member = false;
            while matches!(self.peek(), Token::Identifier(word) if matches!(word.as_str(), "const" | "override" | "final"))
            {
                if matches!(self.peek(), Token::Identifier(word) if word == "const") {
                    is_const_member = true;
                }
                self.advance();
            }
            let has_body = *self.peek() == Token::BraceOpen;
            if self.eat_keyword(Token::Equals) {
                // Pure virtual (`= 0`) and deleted/defaulted declarations all
                // still occupy their declaration-selected slot.
                self.advance();
            }
            if *self.peek() != Token::Semicolon && !has_body {
                return Err(Diagnostic::error("not a class method declaration"));
            }
            Ok((
                member,
                return_type,
                return_struct_tag,
                parameters,
                cxx_parameters,
                variadic,
                is_static,
                is_virtual,
                is_inline || has_body,
                is_const_member,
            ))
        })();

        self.position = saved_position;
        self.last_struct_tag = saved_struct_tag;
        self.last_enum_tag = saved_enum_tag;
        self.last_type_was_wchar = saved_wchar;
        self.last_array_typedef = saved_array_typedef;
        self.last_type_was_const = saved_type_const;
        self.last_pointer_const = saved_pointer_const;
        self.last_cxx_pointer_depth = saved_cxx_pointer_depth;
        self.last_cxx_pointer_base = saved_cxx_pointer_base;
        self.last_type_was_volatile = saved_volatile;

        if let Ok((
            member,
            return_type,
            return_struct_tag,
            parameters,
            cxx_parameters,
            variadic,
            is_static,
            is_virtual,
            is_inline,
            is_const_member,
        )) = recovered
        {
            let explicitly_virtual = is_virtual;
            let inherited_virtual = self
                .cxx_dispatch_tables
                .get(class)
                .and_then(|table| table.methods.get(&member))
                .and_then(|methods| {
                    methods.iter().find(|method| {
                        method.parameters == parameters && method.variadic == variadic
                    })
                })
                .cloned();
            if !explicitly_virtual && inherited_virtual.is_some() {
                self.cxx_inline_ordinal_facts.virtual_method_declarations += 1;
            }
            let is_virtual = is_virtual || inherited_virtual.is_some();
            if is_virtual {
                {
                    let table = self.cxx_dispatch_tables.get_mut(class)?;
                    if inherited_virtual.is_none() {
                        let slot_offset = table.next_slot_offset;
                        table.next_slot_offset = table.next_slot_offset.checked_add(4)?;
                        table.methods.entry(member.clone()).or_default().push(
                            RecoveredCxxVirtualMethod {
                                return_type,
                                return_struct_tag: return_struct_tag.clone(),
                                parameters: parameters.clone(),
                                fixed_parameter_count: parameters.len(),
                                variadic,
                                vptr_offset: 0,
                                slot_offset,
                            },
                        );
                    }
                }
                if is_inline {
                    self.capture_constant_virtual_inline(
                        declaration_index,
                        class,
                        &member,
                        return_type,
                        &cxx_parameters,
                        variadic,
                        is_const_member,
                    );
                } else {
                    let scopes: Vec<&str> = class.split("::").collect();
                    let mangled = if is_const_member && !variadic {
                        mangle_qualified_member_function_cv_typed(
                            &scopes,
                            &member,
                            &cxx_parameters,
                            true,
                        )
                        .ok()?
                    } else {
                        mangle_qualified_member_function_variadic_typed(
                            &scopes,
                            &member,
                            &cxx_parameters,
                            variadic,
                        )
                        .ok()?
                    };
                    self.cxx_explicit_instance_methods
                        .entry((class.to_string(), member))
                        .or_default()
                        .push(RecoveredCxxMethod {
                            mangled,
                            fixed_parameter_count: parameters.len(),
                            variadic,
                            parameters: parameters.clone(),
                            cxx_parameters: cxx_parameters.clone(),
                        });
                }
                // A virtual call never references the out-of-line member symbol
                // directly. Recording the slot is the complete result.
                return Some(None);
            }
            let scopes: Vec<&str> = class.split("::").collect();
            let mangled = if is_const_member && !variadic {
                mangle_qualified_member_function_cv_typed(&scopes, &member, &cxx_parameters, true)
                    .ok()?
            } else {
                mangle_qualified_member_function_variadic_typed(
                    &scopes,
                    &member,
                    &cxx_parameters,
                    variadic,
                )
                .ok()?
            };
            let method = RecoveredCxxMethod {
                mangled: mangled.clone(),
                fixed_parameter_count: parameters.len(),
                variadic,
                parameters: parameters.clone(),
                cxx_parameters: cxx_parameters.clone(),
            };
            if let Some(return_struct_tag) = return_struct_tag {
                self.function_return_structs
                    .insert(mangled.clone(), return_struct_tag);
            }
            if is_inline {
                self.skipped_inline_names.insert(mangled.clone());
            }
            let prototype_parameters = if is_static {
                self.cxx_static_methods
                    .entry((class.to_string(), member))
                    .or_default()
                    .push(method);
                parameters
            } else {
                self.cxx_instance_methods
                    .entry((class.to_string(), member.clone()))
                    .or_default()
                    .push(method.clone());
                self.cxx_explicit_instance_methods
                    .entry((class.to_string(), member))
                    .or_default()
                    .push(method);
                if is_inline {
                    return Some(None);
                }
                let mut prototype_parameters = vec![Type::StructPointer {
                    element_size: self.structs.get(class).map_or(0, |layout| layout.size),
                }];
                prototype_parameters.extend(parameters);
                prototype_parameters
            };
            if variadic {
                self.variadic_definitions.insert(mangled.clone());
            }
            return Some(Some((mangled, return_type, prototype_parameters)));
        }
        None
    }

    /// Retain the common vtable-owned inline leaf (`virtual bool f() const {
    /// return false; }`) as a weak out-of-line function. The vtable relocation,
    /// checked when the translation unit closes, decides whether the candidate
    /// is actually emitted.
    fn capture_constant_virtual_inline(
        &mut self,
        declaration_index: usize,
        class: &str,
        member: &str,
        return_type: Type,
        parameters: &[CxxParameterType],
        variadic: bool,
        is_const_member: bool,
    ) {
        let Some(body_open) = (declaration_index..self.tokens.len())
            .find(|&index| self.tokens[index] == Token::BraceOpen)
        else {
            return;
        };
        let value = match self.tokens.get(body_open + 1..body_open + 5) {
            Some(
                [Token::KeywordReturn, Token::Identifier(value), Token::Semicolon, Token::BraceClose],
            ) if value == "false" => 0,
            Some(
                [Token::KeywordReturn, Token::Identifier(value), Token::Semicolon, Token::BraceClose],
            ) if value == "true" => 1,
            Some(
                [Token::KeywordReturn, Token::IntegerLiteral(value), Token::Semicolon, Token::BraceClose],
            ) => *value,
            _ => return,
        };
        let scopes: Vec<&str> = class.split("::").collect();
        let mangled = if is_const_member && !variadic {
            mangle_qualified_member_function_cv_typed(&scopes, member, parameters, true)
        } else {
            mangle_qualified_member_function_variadic_typed(&scopes, member, parameters, variadic)
        };
        let Ok(mangled) = mangled else {
            return;
        };
        if self
            .cxx_inline_materializations
            .iter()
            .any(|function| function.name == mangled)
        {
            return;
        }
        self.cxx_inline_materializations.push(Function {
            return_type,
            name: mangled,
            is_static: false,
            is_weak: true,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer {
                    element_size: self.structs.get(class).map_or(0, |layout| layout.size),
                },
                name: "this".to_string(),
            }],
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(value)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        });
    }

    /// Recognize `inline void operator delete(void* p) { Owner::Free(p); }`.
    /// Deleting destructors are synthesized after this definition is dropped,
    /// so retain the verified forwarding callee for that ABI-generated call.
    pub(crate) fn try_record_inline_delete_forwarder(&mut self) {
        let mut index = self.position;
        while matches!(self.tokens.get(index), Some(Token::Identifier(word))
            if matches!(word.as_str(), "inline" | "__inline" | "static"))
        {
            index += 1;
        }
        if self.tokens.get(index) != Some(&Token::KeywordVoid)
            || !matches!(self.tokens.get(index + 1), Some(Token::Identifier(word)) if word == "operator")
            || !matches!(self.tokens.get(index + 2), Some(Token::Identifier(word)) if word == "delete")
            || self.tokens.get(index + 3) != Some(&Token::ParenOpen)
        {
            return;
        }
        index += 4;
        let Some(parameter_close) = (index..self.tokens.len())
            .find(|&candidate| self.tokens[candidate] == Token::ParenClose)
        else {
            return;
        };
        let Some(parameter) = self.tokens[index..parameter_close]
            .iter()
            .rev()
            .find_map(|token| match token {
                Token::Identifier(name)
                    if !matches!(name.as_str(), "void" | "const" | "volatile") =>
                {
                    Some(name.as_str())
                }
                _ => None,
            })
        else {
            return;
        };
        let body = parameter_close + 1;
        let Some(
            [Token::BraceOpen, Token::Identifier(class), Token::Colon, Token::Colon, Token::Identifier(member), Token::ParenOpen, Token::Identifier(argument), Token::ParenClose, Token::Semicolon, Token::BraceClose],
        ) = self.tokens.get(body..body + 10)
        else {
            return;
        };
        if argument == parameter {
            if let Ok(callee) = self.resolve_static_member_call(class, member, 1) {
                self.cxx_delete_forwarder = Some(callee);
            }
        }
    }

    /// Resolve `Class::member(args)` using recovered static declarations.
    /// Arity is sufficient only when it selects one overload; ambiguity defers.
    pub(crate) fn resolve_static_member_call(
        &self,
        class: &str,
        member: &str,
        argument_count: usize,
    ) -> Compilation<String> {
        let source_class = class;
        let class = self.qualify_cxx_class_name(source_class);
        let candidates: Vec<&RecoveredCxxMethod> = self
            .cxx_static_methods
            .get(&(class.clone(), member.to_string()))
            .or_else(|| {
                self.cxx_static_methods
                    .get(&(source_class.to_string(), member.to_string()))
            })
            .into_iter()
            .flatten()
            .filter(|method| {
                method.fixed_parameter_count == argument_count
                    || (method.variadic && argument_count >= method.fixed_parameter_count)
            })
            .collect();
        if candidates.len() != 1 {
            return Err(Diagnostic::error(format!(
                "static C++ member call '{class}::{member}' is ambiguous or unavailable (roadmap)"
            )));
        }
        Ok(candidates[0].mangled.clone())
    }

    /// Resolve `Base::member(args)` inside a member body. An explicit class
    /// qualifier suppresses virtual dispatch and still passes the current
    /// object as the first EABI argument. Only the declaration-only primary-base
    /// chain is accepted, so no secondary-base `this` adjustment is guessed.
    pub(crate) fn resolve_explicit_instance_member_call(
        &self,
        source_class: &str,
        member: &str,
        argument_count: usize,
    ) -> Compilation<Option<String>> {
        let Some(current_class) = self.current_cxx_member_class.as_deref() else {
            return Ok(None);
        };
        let Some(class) = self.resolve_scoped_cxx_class_name(source_class) else {
            return Ok(None);
        };
        let mut cursor = current_class;
        let mut related = cursor == class;
        let mut visited = std::collections::HashSet::new();
        while !related && visited.insert(cursor) {
            let Some(base) = self.cxx_primary_bases.get(cursor) else {
                break;
            };
            related = base == &class;
            cursor = base;
        }
        if !related {
            return Ok(None);
        }
        let candidates: Vec<&RecoveredCxxMethod> = self
            .cxx_explicit_instance_methods
            .get(&(class.clone(), member.to_string()))
            .into_iter()
            .flatten()
            .filter(|method| {
                method.fixed_parameter_count == argument_count
                    || (method.variadic && argument_count >= method.fixed_parameter_count)
            })
            .collect();
        match candidates.as_slice() {
            [] => Ok(None),
            [method] => Ok(Some(method.mangled.clone())),
            _ => Err(Diagnostic::error(format!(
                "explicit C++ instance call '{class}::{member}' is ambiguous (roadmap)"
            ))),
        }
    }

    /// Recognize `Alias::Nested()` when `Nested` is an empty class declared
    /// inside the aliased template. This is a value construction, not a static
    /// member call. Keeping the query in the C++ declaration registry prevents
    /// expression parsing from guessing based on spelling alone.
    pub(crate) fn is_empty_nested_type_constructor(&self, outer: &str, nested: &str) -> bool {
        let qualified_outer = self.qualify_cxx_class_name(outer);
        let template = self
            .template_aliases
            .get(outer)
            .or_else(|| self.template_aliases.get(&qualified_outer));
        if self.empty_nested_template_types.contains(&(
            template.map_or(outer, String::as_str).to_string(),
            nested.to_string(),
        )) {
            return true;
        }
        let qualified_template = template.map(|name| self.qualify_cxx_class_name(name));
        let suffix = format!("::{nested}");
        self.structs.iter().any(|(name, layout)| {
            if !layout.fields.is_empty() || !name.ends_with(&suffix) {
                return false;
            }
            let owner = &name[..name.len() - suffix.len()];
            owner == outer
                || owner == qualified_outer
                || template.is_some_and(|template| owner == template)
                || qualified_template
                    .as_deref()
                    .is_some_and(|template| owner == template)
        })
    }

    /// A namespace-qualified empty aggregate construction (`N::empty_tag()`)
    /// is likewise a value expression, not a free-function call.
    pub(crate) fn is_empty_qualified_type_constructor(&self, scope: &str, name: &str) -> bool {
        let qualified = format!("{scope}::{name}");
        self.structs
            .get(&qualified)
            .or_else(|| self.structs.get(name))
            .is_some_and(|layout| layout.fields.is_empty())
    }

    pub(crate) fn resolve_instance_member_call(
        &self,
        class: &str,
        member: &str,
        arguments: &[Expression],
    ) -> Compilation<Option<ImplicitMemberCall>> {
        let argument_count = arguments.len();
        let resolved = self
            .resolve_scoped_cxx_class_name(class)
            .unwrap_or_else(|| class.to_owned());
        let candidates: Vec<&RecoveredCxxMethod> = self
            .cxx_instance_methods
            .get(&(resolved.clone(), member.to_string()))
            .or_else(|| {
                self.cxx_instance_methods
                    .get(&(class.to_string(), member.to_string()))
            })
            .into_iter()
            .flatten()
            .filter(|method| {
                method.fixed_parameter_count == argument_count
                    || (method.variadic && argument_count >= method.fixed_parameter_count)
            })
            .collect();
        match candidates.as_slice() {
            [method] => {
                return Ok(Some(ImplicitMemberCall::Direct {
                    name: method.mangled.clone(),
                    is_inline: self.skipped_inline_names.contains(&method.mangled),
                    this_adjustment: 0,
                    parameters: method.parameters.clone(),
                }));
            }
            [] => {}
            _ => {
                let exact = candidates
                    .iter()
                    .filter(|method| {
                        self.cxx_arguments_exactly_match(
                            &method.parameters,
                            &method.cxx_parameters,
                            method.variadic,
                            arguments,
                        )
                    })
                    .collect::<Vec<_>>();
                if let [method] = exact.as_slice() {
                    return Ok(Some(ImplicitMemberCall::Direct {
                        name: method.mangled.clone(),
                        is_inline: self.skipped_inline_names.contains(&method.mangled),
                        this_adjustment: 0,
                        parameters: method.parameters.clone(),
                    }));
                }
                return Err(Diagnostic::error(format!(
                    "C++ member call '{resolved}::{member}' is ambiguous (roadmap)"
                )));
            }
        }
        // The shared resolver uses complete layouts when available and safely
        // falls back to a declaration-only primary-base chain. The latter is
        // important for templates whose concrete layout could not be recovered:
        // their primary base remains at offset zero and can still own members.
        if let Some(call) = self.resolve_member_call_in_class(&resolved, member, arguments)? {
            return Ok(Some(call));
        }
        if let Some((dispatch, return_struct_tag, parameters)) =
            self.resolve_virtual_member_call(&resolved, member, argument_count)?
        {
            return Ok(Some(ImplicitMemberCall::Virtual {
                dispatch,
                return_struct_tag,
                this_adjustment: 0,
                direct_name: None,
                direct_is_inline: false,
                parameters,
            }));
        }
        Ok(None)
    }

    /// Resolve a virtual member by declaration signature and return the ABI
    /// dispatch location. As with direct members, arity is accepted only when it
    /// identifies exactly one overload. Incomplete tables never produce a slot.
    pub(crate) fn resolve_virtual_member_call(
        &self,
        class: &str,
        member: &str,
        argument_count: usize,
    ) -> Compilation<Option<(VirtualDispatch, Option<String>, Vec<Type>)>> {
        let source_class = class;
        let class = self.qualify_cxx_class_name(source_class);
        let resolved_class = if self.cxx_dispatch_tables.contains_key(&class) {
            class
        } else {
            source_class.to_string()
        };
        if self.incomplete_cxx_dispatch.contains(&resolved_class) {
            return Ok(None);
        }
        let candidates: Vec<&RecoveredCxxVirtualMethod> = self
            .cxx_dispatch_tables
            .get(&resolved_class)
            .and_then(|table| table.methods.get(member))
            .into_iter()
            .flatten()
            .filter(|method| {
                method.fixed_parameter_count == argument_count
                    || (method.variadic && argument_count >= method.fixed_parameter_count)
            })
            .collect();
        if candidates.is_empty() {
            let primary = source_class.split('<').next().unwrap_or(source_class);
            let qualified_primary = self.qualify_cxx_class_name(primary);
            let template_candidates: Vec<_> = self
                .cxx_template_virtual_methods
                .get(&(qualified_primary, member.to_string()))
                .or_else(|| {
                    self.cxx_template_virtual_methods
                        .get(&(primary.to_string(), member.to_string()))
                })
                .into_iter()
                .flatten()
                .filter_map(|(arity, dispatch)| (*arity == argument_count).then_some(*dispatch))
                .collect();
            return match template_candidates.as_slice() {
                [] => Ok(None),
                [dispatch] => Ok(Some((*dispatch, None, Vec::new()))),
                _ => Err(Diagnostic::error(format!(
                    "virtual C++ template member call '{primary}::{member}' is ambiguous (roadmap)"
                ))),
            };
        }
        match candidates.as_slice() {
            [] => Ok(None),
            [method] => Ok(Some((
                VirtualDispatch {
                    vptr_offset: method.vptr_offset,
                    slot_offset: method.slot_offset,
                    return_type: method.return_type,
                    variadic: method.variadic,
                },
                method.return_struct_tag.clone(),
                method.parameters.clone(),
            ))),
            _ => Err(Diagnostic::error(format!(
                "virtual C++ member call '{resolved_class}::{member}' is ambiguous (roadmap)"
            ))),
        }
    }

    /// Mangle a member declared in the active namespace scope. Class layouts
    /// remain keyed by their local source name; namespace qualification is an
    /// ABI concern applied only at symbol boundaries.
    pub(crate) fn mangle_member_in_current_namespace(
        &self,
        class: &str,
        function: &str,
        explicit_parameters: &[Type],
    ) -> Compilation<String> {
        let mut scopes = self.named_namespace_scopes();
        scopes.push(class);
        mangle_qualified_member_function(&scopes, function, explicit_parameters)
    }

    /// Typed sibling used by parsed C++ declarators whose aggregate names,
    /// references, and cv-qualifiers must survive into the ABI symbol.
    pub(crate) fn mangle_typed_member_in_current_namespace(
        &self,
        class: &str,
        function: &str,
        explicit_parameters: &[CxxParameterType],
    ) -> Compilation<String> {
        let mut scopes = self.named_namespace_scopes();
        scopes.extend(class.split("::"));
        mangle_qualified_member_function_typed(&scopes, function, explicit_parameters)
    }

    pub(crate) fn mangle_typed_const_member_in_current_namespace(
        &self,
        class: &str,
        function: &str,
        explicit_parameters: &[CxxParameterType],
    ) -> Compilation<String> {
        let mut scopes = self.named_namespace_scopes();
        scopes.extend(class.split("::"));
        mangle_qualified_member_function_cv_typed(&scopes, function, explicit_parameters, true)
    }

    /// Mangle a non-member C++ function. A namespace-qualified free function
    /// uses the qualified member spelling; a global free function has the
    /// compact `name__F<arguments>` form.
    pub(crate) fn mangle_typed_free_function(
        &self,
        function: &str,
        explicit_parameters: &[CxxParameterType],
        variadic: bool,
    ) -> Compilation<String> {
        let arguments = encode_function_arguments(explicit_parameters, variadic)?;
        let scopes = self.named_namespace_scopes();
        if scopes.is_empty() {
            Ok(format!("{function}__F{arguments}"))
        } else {
            let qualified_scope = encode_qualified_scope(&scopes)?;
            Ok(format!("{function}__{qualified_scope}F{arguments}"))
        }
    }

    pub(crate) fn mangle_typed_free_function_in_scope(
        &self,
        scope: &str,
        function: &str,
        explicit_parameters: &[CxxParameterType],
        variadic: bool,
    ) -> Compilation<String> {
        let arguments = encode_function_arguments(explicit_parameters, variadic)?;
        let scopes: Vec<&str> = scope.split("::").collect();
        let qualified_scope = encode_qualified_scope(&scopes)?;
        Ok(format!("{function}__{qualified_scope}F{arguments}"))
    }

    /// Mangle a static data member declared in the active namespace scope.
    /// Data members share the class/namespace encoding used by member functions,
    /// but carry no `F<parameters>` suffix.
    pub(crate) fn mangle_data_member_in_current_namespace(
        &self,
        class: &str,
        member: &str,
    ) -> Compilation<String> {
        let mut scopes = self.named_namespace_scopes();
        scopes.extend(class.split("::"));
        mangle_qualified_data_member(&scopes, member)
    }

    /// Register one namespace-scope data object and return its ABI symbol.
    /// Anonymous namespaces have no scope spelling in this compiler family.
    pub(crate) fn register_cxx_data_object(
        &mut self,
        source_name: &str,
    ) -> Compilation<String> {
        let scopes = self.named_namespace_scopes();
        if scopes.is_empty() {
            return Ok(source_name.to_owned());
        }
        let qualified_source = format!("{}::{source_name}", scopes.join("::"));
        let mangled = mangle_qualified_data_member(&scopes, source_name)?;
        self.cxx_data_objects
            .insert(qualified_source, mangled.clone());
        Ok(mangled)
    }

    /// Resolve an unqualified data-object use from the innermost active
    /// namespace outward, following ordinary C++ lexical lookup.
    pub(crate) fn resolve_cxx_data_object(&self, source_name: &str) -> Option<String> {
        let scopes = self.named_namespace_scopes();
        (1..=scopes.len()).rev().find_map(|depth| {
            let qualified = format!("{}::{source_name}", scopes[..depth].join("::"));
            self.cxx_data_objects.get(&qualified).cloned()
        })
    }

    /// Resolve a bare static-data-member name inside one of its class methods.
    /// The out-of-class definition is already registered under its ABI name;
    /// this lookup keeps ordinary local/instance-member shadowing in the
    /// expression parser while centralizing C++ qualification here.
    pub(crate) fn resolve_implicit_static_data_member(
        &self,
        member: &str,
    ) -> Compilation<Option<String>> {
        let Some(class) = self.current_cxx_member_class.as_deref() else {
            return Ok(None);
        };
        let scopes: Vec<&str> = class.split("::").collect();
        let mangled = mangle_qualified_data_member(&scopes, member)?;
        Ok(self.global_sizes.contains_key(&mangled).then_some(mangled))
    }

    /// Resolve an unqualified call inside a member body. Arity is enough for the
    /// currently modeled overload set; ambiguous same-arity overloads defer.
    pub(crate) fn resolve_implicit_member_call(
        &self,
        function: &str,
        arguments: &[Expression],
    ) -> Compilation<Option<ImplicitMemberCall>> {
        let Some(class_name) = self.current_cxx_member_class.as_deref() else {
            return Ok(None);
        };
        self.resolve_member_call_in_class(class_name, function, arguments)
    }

    /// Resolve ordinary member lookup for a known object class. Both implicit
    /// calls on `this` and explicit `object.member()` calls use this path so
    /// inheritance, virtual dispatch, inline identity, and base adjustment
    /// cannot drift between the two expression spellings.
    fn resolve_member_call_in_class(
        &self,
        class_name: &str,
        function: &str,
        arguments: &[Expression],
    ) -> Compilation<Option<ImplicitMemberCall>> {
        let argument_count = arguments.len();
        if let Some(methods) = self
            .cxx_classes
            .get(class_name)
            .and_then(|class| class.methods.get(function))
        {
            let candidates: Vec<&MemberMethod> = methods
                .iter()
                .filter(|method| method.parameters.len() == argument_count)
                .collect();
            let candidates = if candidates.len() > 1 {
                candidates
                    .into_iter()
                    .filter(|method| {
                        self.cxx_arguments_exactly_match(
                            &method.parameters,
                            &method.cxx_parameters,
                            false,
                            arguments,
                        )
                    })
                    .collect()
            } else {
                candidates
            };
            if candidates.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "member overload resolution for '{class_name}::{function}' is ambiguous or unavailable (roadmap)"
                )));
            }
            let method = candidates[0];
            if let Some(dispatch) = method.virtual_dispatch {
                return Ok(Some(ImplicitMemberCall::Virtual {
                    dispatch,
                    return_struct_tag: method.return_struct_tag.clone(),
                    this_adjustment: 0,
                    direct_name: Some(mangle_qualified_member_function_typed(
                        &class_name.split("::").collect::<Vec<_>>(),
                        function,
                        &method.cxx_parameters,
                    )?),
                    direct_is_inline: method.is_inline,
                    parameters: method.parameters.clone(),
                }));
            }
            return Ok(Some(ImplicitMemberCall::Direct {
                name: mangle_qualified_member_function_typed(
                    &class_name.split("::").collect::<Vec<_>>(),
                    function,
                    &method.cxx_parameters,
                )?,
                is_inline: method.is_inline,
                this_adjustment: 0,
                parameters: method.parameters.clone(),
            }));
        }

        // Search every complete non-virtual base subobject in declaration
        // order. A declaration in a base hides declarations further up that
        // branch; declarations reached through different base subobjects are
        // ambiguous, matching ordinary C++ member lookup. Retaining the byte
        // adjustment here keeps ABI pointer formation out of name mangling.
        let mut inherited = Vec::new();
        let mut pending: Vec<(String, u32)> = self
            .cxx_classes
            .get(class_name)
            .into_iter()
            .flat_map(|class| class.bases.iter().rev())
            .map(|base| (base.name.clone(), base.offset))
            .collect();
        let mut visited = std::collections::HashSet::new();
        while let Some((owner, this_adjustment)) = pending.pop() {
            if !visited.insert((owner.clone(), this_adjustment)) {
                continue;
            }
            let Some(class) = self.cxx_classes.get(&owner) else {
                continue;
            };
            if let Some(methods) = class.methods.get(function) {
                let candidates: Vec<&MemberMethod> = methods
                    .iter()
                    .filter(|method| method.parameters.len() == argument_count)
                    .collect();
                let candidates = if candidates.len() > 1 {
                    candidates
                        .into_iter()
                        .filter(|method| {
                            self.cxx_arguments_exactly_match(
                                &method.parameters,
                                &method.cxx_parameters,
                                false,
                                arguments,
                            )
                        })
                        .collect()
                } else {
                    candidates
                };
                if candidates.len() != 1 {
                    return Err(Diagnostic::error(format!(
                        "member overload resolution for '{owner}::{function}' is ambiguous or unavailable (roadmap)"
                    )));
                }
                inherited.push((owner, candidates[0].clone(), this_adjustment));
                continue;
            }
            for base in class.bases.iter().rev().filter(|base| !base.is_virtual) {
                let adjustment = this_adjustment.checked_add(base.offset).ok_or_else(|| {
                    Diagnostic::error("C++ base-subobject adjustment overflow")
                })?;
                pending.push((base.name.clone(), adjustment));
            }
        }
        if inherited.len() > 1 {
            return Err(Diagnostic::error(format!(
                "member lookup for '{class_name}::{function}' is ambiguous across base subobjects (roadmap)"
            )));
        }
        if let Some((owner, method, this_adjustment)) = inherited.pop() {
            if let Some(dispatch) = method.virtual_dispatch {
                return Ok(Some(ImplicitMemberCall::Virtual {
                    dispatch,
                    return_struct_tag: method.return_struct_tag.clone(),
                    this_adjustment,
                    direct_name: Some(mangle_qualified_member_function_typed(
                        &owner.split("::").collect::<Vec<_>>(),
                        function,
                        &method.cxx_parameters,
                    )?),
                    direct_is_inline: method.is_inline,
                    parameters: method.parameters.clone(),
                }));
            }
            return Ok(Some(ImplicitMemberCall::Direct {
                name: mangle_qualified_member_function_typed(
                    &owner.split("::").collect::<Vec<_>>(),
                    function,
                    &method.cxx_parameters,
                )?,
                is_inline: method.is_inline,
                this_adjustment,
                parameters: method.parameters.clone(),
            }));
        }

        // A declaration-only primary-base chain remains usable when the full
        // executable class layout was too complex to recover. Only that chain
        // is safe: its incoming `this` stays at offset zero.
        let mut owner = class_name;
        let mut visited = std::collections::HashSet::new();
        while visited.insert(owner) {
            let candidates: Vec<&RecoveredCxxMethod> = self
                .cxx_instance_methods
                .get(&(owner.to_string(), function.to_string()))
                .into_iter()
                .flatten()
                .filter(|method| {
                    method.fixed_parameter_count == argument_count
                        || (method.variadic && argument_count >= method.fixed_parameter_count)
                })
                .collect();
            match candidates.as_slice() {
                [method] => return Ok(Some(ImplicitMemberCall::Direct {
                    name: method.mangled.clone(),
                    is_inline: self.skipped_inline_names.contains(&method.mangled),
                    this_adjustment: 0,
                    parameters: method.parameters.clone(),
                })),
                [] => {}
                _ => {
                    let exact = candidates
                        .iter()
                        .filter(|method| {
                            self.cxx_arguments_exactly_match(
                                &method.parameters,
                                &method.cxx_parameters,
                                method.variadic,
                                arguments,
                            )
                        })
                        .collect::<Vec<_>>();
                    if let [method] = exact.as_slice() {
                        return Ok(Some(ImplicitMemberCall::Direct {
                            name: method.mangled.clone(),
                            is_inline: self.skipped_inline_names.contains(&method.mangled),
                            this_adjustment: 0,
                            parameters: method.parameters.clone(),
                        }));
                    }
                    return Err(Diagnostic::error(format!(
                        "member overload resolution for '{owner}::{function}' is ambiguous (roadmap)"
                    )));
                }
            }
            let Some(base) = self.cxx_primary_bases.get(owner) else {
                break;
            };
            owner = base;
        }
        Ok(None)
    }

    /// Parse one class definition and recover its object layout.
    /// Method declarations do not occupy storage and are skipped after recording
    /// constructor signatures. Non-virtual bases are laid out in declaration order.
    /// CodeWarrior inserts a class's own vptr at the declaration position of
    /// its first virtual member, so fields written before `virtual` remain at
    /// the object prefix rather than being shifted.
    pub(crate) fn parse_class_definition(
        &mut self,
    ) -> Compilation<(String, StructLayout, ClassLayout)> {
        self.parse_class_definition_in_scope(None)
    }

    /// Parse a class layout while retaining the lexical owner of directly
    /// nested classes. Nested declarations do not occupy storage themselves,
    /// but their complete layouts may be used by a later value member or as a
    /// qualified base (`Outer::Inner`). Keeping this recursion inside layout
    /// parsing means an unsupported nested declaration cannot be mistaken for
    /// the outer class's first data member.
    fn parse_class_definition_in_scope(
        &mut self,
        enclosing_class: Option<&str>,
    ) -> Compilation<(String, StructLayout, ClassLayout)> {
        let class_keyword = self.eat_word("class");
        if !class_keyword && !self.eat_keyword(Token::KeywordStruct) {
            return Err(Diagnostic::error("expected a C++ class definition"));
        }
        let name = self.parse_identifier()?;
        let qualified_name = enclosing_class.map_or_else(
            || self.qualify_cxx_class_name(&name),
            |outer| format!("{outer}::{name}"),
        );
        let previous_layout_scope = self
            .current_cxx_layout_scope
            .replace(qualified_name.clone());
        let previous_layout_constants = self.cxx_layout_constants.clone();
        let result = self.parse_class_definition_body(name, qualified_name);
        self.cxx_layout_constants = previous_layout_constants;
        self.current_cxx_layout_scope = previous_layout_scope;
        result
    }

    /// Retain a simple in-class integral constant long enough to lay out later
    /// array members (`static const int N = 32; T values[N];`). Static storage
    /// itself still contributes no bytes to the class. Parsing on a clone keeps
    /// failed or non-integral declarations side-effect free.
    fn capture_cxx_layout_integral_constant(&mut self) {
        let mut probe = self.clone();
        probe.eat_word("const");
        let Ok(declared_type) = probe.parse_type() else {
            return;
        };
        if !matches!(
            declared_type,
            Type::Int
                | Type::UnsignedInt
                | Type::Short
                | Type::UnsignedShort
                | Type::Char
                | Type::UnsignedChar
                | Type::LongLong
                | Type::UnsignedLongLong
        ) {
            return;
        }
        let Ok(name) = probe.parse_identifier() else {
            return;
        };
        if !probe.eat_keyword(Token::Equals) {
            return;
        }
        let Ok(value) = probe.parse_integer_constant() else {
            return;
        };
        if *probe.peek() == Token::Semicolon {
            self.cxx_layout_constants.insert(name, value);
        }
    }

    /// Find the callable slot overridden along the zero-offset primary-base
    /// chain. Declaring an override with an explicit `virtual` keyword does not
    /// append a new slot; it replaces the matching base entry in place.
    /// Secondary-base thunks need separate vtable-component modeling and are
    /// deliberately excluded here.
    fn resolve_primary_base_virtual_override(
        &self,
        class: &ClassLayout,
        member: &str,
        parameters: &[CxxParameterType],
        is_const_member: bool,
    ) -> Compilation<Option<VirtualDispatch>> {
        let mut primary = class
            .bases
            .first()
            .filter(|base| !base.is_virtual && base.offset == 0)
            .map(|base| base.name.as_str());
        while let Some(owner) = primary {
            let Some(base) = self.cxx_classes.get(owner) else {
                return Ok(None);
            };
            if let Some(methods) = base.methods.get(member) {
                let candidates = methods
                    .iter()
                    .filter(|method| {
                        // Storage types intentionally erase aggregate identity:
                        // two `const T&` parameters are both one-word struct
                        // pointers. Override resolution must use the retained
                        // C++ source identities or an overload set of unrelated
                        // aggregate references appears ambiguous.
                        method.cxx_parameters.len() == parameters.len()
                            && method
                                .cxx_parameters
                                .iter()
                                .zip(parameters)
                                .all(|(left, right)| left.same_declaration_identity(right))
                            && method.is_const_member == is_const_member
                            && method.virtual_dispatch.is_some()
                    })
                    .collect::<Vec<_>>();
                match candidates.as_slice() {
                    [] => {}
                    [method] => return Ok(method.virtual_dispatch),
                    _ => {
                        return Err(Diagnostic::error(format!(
                            "virtual override '{owner}::{member}' is ambiguous (roadmap)"
                        )))
                    }
                }
            }
            primary = base
                .bases
                .first()
                .filter(|base| !base.is_virtual && base.offset == 0)
                .map(|base| base.name.as_str());
        }
        Ok(None)
    }

    fn parse_class_definition_body(
        &mut self,
        name: String,
        qualified_name: String,
    ) -> Compilation<(String, StructLayout, ClassLayout)> {
        let mut class = ClassLayout::default();
        let mut layout = StructLayout::default();
        let mut offset = 0u32;
        let mut max_align = 1u32;

        if self.eat_keyword(Token::Colon) {
            loop {
                let mut is_virtual_base = false;
                loop {
                    if matches!(self.peek(), Token::Identifier(word)
                        if matches!(word.as_str(), "public" | "private" | "protected"))
                    {
                        self.advance();
                    } else if self.eat_word("virtual") {
                        is_virtual_base = true;
                    } else {
                        break;
                    }
                }
                let source_base_name = self.parse_cxx_qualified_identifier()?;
                let base_name = self
                    .resolve_scoped_cxx_class_name(&source_base_name)
                    .unwrap_or(source_base_name);
                let base_class = self.cxx_classes.get(&base_name).cloned();
                let (base_is_polymorphic, base_vptr_offset, base_virtual_slots) = base_class
                    .as_ref()
                    .map_or((false, None, 0), |base| {
                        (base.is_polymorphic, base.vptr_offset, base.virtual_slots)
                    });
                let base = self
                    .structs
                    .get(&base_name)
                    .cloned()
                    .or_else(|| self.asserted_aggregate_layout(&base_name))
                    .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "base class '{base_name}' must be defined before '{name}'"
                    ))
                })?;
                let inherited_virtual_bases = base_class
                    .as_ref()
                    .map(|base| base.virtual_bases.clone())
                    .unwrap_or_default();
                if is_virtual_base {
                    // CodeWarrior reserves one word in the non-virtual region
                    // for this virtual-base path. The shared base subobject is
                    // materialized after the most-derived class's own fields.
                    offset = offset.div_ceil(4) * 4;
                    offset = offset.checked_add(4).ok_or_else(|| {
                        Diagnostic::error("C++ virtual-base layout exceeds the 32-bit address space")
                    })?;
                    max_align = max_align.max(4);
                    if !class.virtual_bases.contains(&base_name) {
                        class.virtual_bases.push(base_name);
                    }
                    for inherited in inherited_virtual_bases {
                        if !class.virtual_bases.contains(&inherited) {
                            class.virtual_bases.push(inherited);
                        }
                    }
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                let base_align = (base.align as u32).max(1);
                let base_nonvirtual_size = base_class
                    .as_ref()
                    .map(|base| base.nonvirtual_size)
                    .filter(|size| *size != 0)
                    .unwrap_or(base.size);
                offset = offset.div_ceil(base_align) * base_align;
                let base_offset = offset;
                for (field_name, field) in base
                    .fields_in_declaration_order()
                    .into_iter()
                    .filter(|(_, field)| field.offset < base_nonvirtual_size)
                {
                    layout.insert_field(
                        field_name.clone(),
                        StructField {
                            member_type: field.member_type,
                            source_fundamental: field.source_fundamental,
                            offset: base_offset + field.offset,
                            struct_tag: field.struct_tag.clone(),
                            array_element: field.array_element,
                            array_bytes: field.array_bytes,
                            array_stride: field.array_stride,
                            bit_field: field.bit_field,
                        },
                    );
                    if base.function_pointer_fields.contains(field_name) {
                        layout.function_pointer_fields.insert(field_name.clone());
                        if let Some(function_type) = base.function_pointer_types.get(field_name) {
                            layout
                                .function_pointer_types
                                .insert(field_name.clone(), function_type.clone());
                        }
                    }
                }
                let is_primary_base = class.bases.is_empty();
                class.bases.push(BaseClass {
                    name: base_name.clone(),
                    offset: base_offset,
                    is_virtual: false,
                });
                class.is_polymorphic |= base_is_polymorphic;
                if class.vptr_offset.is_none() {
                    class.vptr_offset = base_vptr_offset.map(|offset| base_offset + offset);
                }
                if is_primary_base {
                    class.virtual_slots = base_virtual_slots;
                    if let Some(base_class) = &base_class {
                        class.has_virtual_destructor = base_class.has_virtual_destructor;
                        class.virtual_destructor_slot = base_class.virtual_destructor_slot;
                    }
                }
                if let Some(base_class) = base_class {
                    class.vtable_components.extend(
                        base_class
                            .vtable_components
                            .into_iter()
                            .filter(|component| component.object_offset < base_nonvirtual_size)
                            .map(|component| VtableComponent {
                            object_offset: base_offset + component.object_offset,
                            vptr_offset: base_offset + component.vptr_offset,
                            virtual_slots: component.virtual_slots,
                            virtual_destructor_slot: component.virtual_destructor_slot,
                            }),
                    );
                }
                for inherited in inherited_virtual_bases {
                    if !class.virtual_bases.contains(&inherited) {
                        class.virtual_bases.push(inherited);
                    }
                }
                offset += base_nonvirtual_size;
                max_align = max_align.max(base_align);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }

        self.expect(Token::BraceOpen)?;
        while *self.peek() != Token::BraceClose {
            let nested_definition = (matches!(self.peek(), Token::KeywordStruct)
                || matches!(self.peek(), Token::Identifier(word) if word == "class"))
                && matches!(self.peek_at(1), Token::Identifier(_))
                && matches!(self.peek_at(2), Token::BraceOpen | Token::Colon);
            if nested_definition {
                let (nested_name, nested_layout, nested_class) =
                    self.parse_class_definition_in_scope(Some(&qualified_name))?;
                let nested_qualified = format!("{qualified_name}::{nested_name}");
                self.struct_typedefs
                    .insert(nested_name, nested_qualified.clone());
                self.structs
                    .insert(nested_qualified.clone(), nested_layout);
                if !self.cxx_classes.contains_key(&nested_qualified) {
                    self.cxx_class_declaration_order
                        .push(nested_qualified.clone());
                }
                self.cxx_classes.insert(nested_qualified, nested_class);
                continue;
            }
            // Empty member declarations are valid C++ and commonly survive
            // preprocessing when feature-gated declarations disappear.
            if self.eat_keyword(Token::Semicolon) {
                continue;
            }
            // An anonymous inline struct is a physical subobject (or promoted
            // anonymous member), not a nested named type declaration. Reuse the
            // aggregate layout path so nested anonymous structs compose.
            if *self.peek() == Token::KeywordStruct
                && self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
            {
                class.fields.extend(self.parse_and_place_inline_struct(
                    &mut layout,
                    &mut offset,
                    &mut max_align,
                )?);
                continue;
            }
            // A nested type alias is declaration state, not object storage.
            // Function-pointer aliases are especially common in SDK classes:
            // later data members use the alias as a one-word callback field,
            // while inline methods use its full signature for indirect calls.
            // Retain both facts in the same registries as a top-level typedef
            // before continuing with the class's physical members.
            if self.eat_word("typedef") {
                let aliased = self.parse_type()?;
                let aliased_source = self.take_cxx_type_identity(aliased, false);
                if let Some((alias, _, function_type)) =
                    self.try_cxx_function_pointer_declarator(aliased_source)?
                {
                    if alias.is_empty() {
                        return Err(Diagnostic::error(
                            "a class function-pointer typedef requires a name",
                        ));
                    }
                    self.expect(Token::Semicolon)?;
                    self.typedefs
                        .insert(alias.clone(), Type::Pointer(Pointee::Int));
                    self.function_pointer_typedefs.insert(alias, function_type);
                    continue;
                }

                let alias = self.parse_identifier()?;
                self.expect(Token::Semicolon)?;
                self.typedefs.insert(alias, aliased);
                continue;
            }
            if matches!(self.peek(), Token::Identifier(word)
                if matches!(word.as_str(), "public" | "private" | "protected"))
                && *self.peek_at(1) == Token::Colon
            {
                self.advance();
                self.advance();
                continue;
            }
            // An inline union definition is a complete member type declaration,
            // not the `union Tag` type-id handled by `parse_type`. Keep its
            // storage and promoted-member behavior identical to C aggregates:
            // `union { ... } value;` contributes one named subobject, while
            // `union { ... };` promotes overlapping members into the class.
            if matches!(self.peek(), Token::Identifier(word) if word == "union")
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                class.fields.extend(self.parse_and_place_inline_union(
                    &mut layout,
                    &mut offset,
                    &mut max_align,
                )?);
                continue;
            }
            // Declaration specifiers may be interleaved. Layout recovery does
            // not otherwise care about `inline`, but it must consume it before
            // recognizing an in-class constructor or method declarator.
            let mut is_explicit = false;
            let mut is_virtual = false;
            let mut is_static = false;
            loop {
                if self.eat_word("explicit") {
                    is_explicit = true;
                } else if self.eat_word("virtual") {
                    is_virtual = true;
                } else if self.eat_word("inline") || self.eat_word("__inline") {
                    // In-class definitions are already inline semantically.
                } else if self.eat_word("static") {
                    is_static = true;
                } else {
                    break;
                }
            }
            if is_virtual {
                if !class.is_polymorphic {
                    // Unlike modern Itanium-style layouts, this ABI inserts the
                    // vptr where the first virtual declaration appears. A class
                    // beginning with data therefore keeps that data at offset 0
                    // and receives an aligned vptr after it. Polymorphic bases
                    // already supply the primary vptr and skip this path.
                    offset = offset.div_ceil(4) * 4;
                    class.vptr_offset = Some(offset);
                    class.vtable_components.push(VtableComponent {
                        object_offset: 0,
                        vptr_offset: offset,
                        virtual_slots: 0,
                        virtual_destructor_slot: None,
                    });
                    offset += 4;
                    max_align = max_align.max(4);
                    class.is_polymorphic = true;
                }
            }
            if is_static {
                self.capture_cxx_layout_integral_constant();
                self.skip_class_member()?;
                continue;
            }
            if matches!(self.peek(), Token::Identifier(word) if word == &name)
                && *self.peek_at(1) == Token::ParenOpen
            {
                self.advance();
                class.constructors.push(self.parse_class_parameter_types()?);
                self.skip_class_method_tail()?;
                continue;
            }
            if is_explicit {
                return Err(Diagnostic::error(
                    "'explicit' is only supported on a class constructor",
                ));
            }
            if *self.peek() == Token::Tilde {
                class.declares_destructor = true;
                self.advance();
                let destructor_name = self.parse_identifier()?;
                if destructor_name != name {
                    return Err(Diagnostic::error(format!(
                        "destructor '~{destructor_name}' does not name class '{name}'"
                    )));
                }
                let signature = self.parse_class_parameter_types()?;
                if !signature.parameters.is_empty() {
                    return Err(Diagnostic::error("a destructor cannot have parameters"));
                }
                let is_inline = self.skip_class_method_tail()?;
                let is_virtual_destructor = is_virtual || class.has_virtual_destructor;
                if is_virtual_destructor {
                    if !is_inline && class.vtable_key_function.is_none() {
                        let qualified = self.qualify_cxx_class_name(&name);
                        let scopes: Vec<&str> = qualified.split("::").collect();
                        class.vtable_key_function = Some(
                            mangle_qualified_member_function(&scopes, "__dt", &[])?,
                        );
                    }
                    if class.virtual_destructor_slot.is_none() {
                        class.virtual_destructor_slot = Some(u16::try_from(
                            8usize
                                .checked_add(class.virtual_slots.checked_mul(4).ok_or_else(|| {
                                    Diagnostic::error("C++ virtual destructor slot overflow")
                                })?)
                                .ok_or_else(|| {
                                    Diagnostic::error("C++ virtual destructor slot overflow")
                                })?,
                        )
                        .map_err(|_| Diagnostic::error("C++ virtual destructor slot overflow"))?);
                        class.virtual_slots += 1;
                    }
                    class.has_virtual_destructor = true;
                    if let Some(primary) = class.vtable_components.first_mut() {
                        primary.virtual_slots = class.virtual_slots;
                        primary.virtual_destructor_slot = class.virtual_destructor_slot;
                    }
                }
                // An in-class destructor definition is commonly followed by an
                // optional declaration semicolon (`virtual ~T() { };`). The
                // method-tail skipper stops at `}`, so consume that separator
                // before the next member is interpreted as a fresh type.
                self.eat_keyword(Token::Semicolon);
                continue;
            }

            let declaration_start = self.position;
            let field_is_function_pointer_typedef = matches!(
                self.peek(),
                Token::Identifier(word) if self.function_pointer_typedefs.contains_key(word)
            );
            let field_type = match self.parse_type() {
                Ok(field_type) => field_type,
                Err(error) if !is_virtual => {
                    // An incomplete class cannot yet be parsed as a by-value
                    // return type (`Vector normalized() const`), but such a
                    // declaration contributes no storage. Declaration capture
                    // handles callable semantics independently; layout recovery
                    // only needs to distinguish this from an unsupported field.
                    self.position = declaration_start;
                    if self.cxx_struct_member_is_method() {
                        self.skip_class_member()?;
                        continue;
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            let field_function_type = field_is_function_pointer_typedef
                .then(|| self.last_cxx_function_type.take())
                .flatten();
            if matches!(self.tokens.get(declaration_start), Some(Token::Identifier(word)) if word == "enum")
                && self.eat_keyword(Token::Semicolon)
            {
                // A nested enum definition introduces a type and enumerators,
                // but the declaration itself occupies no object storage. Its
                // body was consumed and registered by `parse_type` above.
                continue;
            }
            // A row-pointer typedef (`typedef T (*Rows)[N]`) is already a
            // pointer and therefore occupies one word as a class member. Keep
            // its byte stride for later indexed-member lowering. A true array
            // typedef (`typedef T Rows[N]`) still declares inline storage and
            // must use the dedicated array layout path before it is admitted.
            let row_pointer_stride = match self.last_array_typedef.take() {
                Some((element, 0, length)) => {
                    Some(type_size(element).saturating_mul(u32::from(length)))
                }
                Some(_) => {
                    return Err(Diagnostic::error(
                        "an array-typedef class member is not supported yet (roadmap)",
                    ));
                }
                None => None,
            };
            let struct_tag = self.last_struct_tag.take();
            let attribute_align = self.skip_attributes()?.unwrap_or(1);
            // Operator overload declarators are methods regardless of the
            // punctuation following `operator` (`[]`, `=`, `+=`, ...). Their
            // bodies and signatures do not contribute storage, so layout
            // recovery can skip them even when their return type is a
            // reference. Ordinary reference-returning methods continue through
            // the method path below after consuming the source-only `&`.
            self.eat_keyword(Token::Ampersand);
            if self.eat_word("operator") {
                self.skip_class_member()?;
                continue;
            }
            if let Some((field_name, _, callback_type)) = self
                .try_cxx_function_pointer_declarator(CxxParameterType::plain(field_type))?
            {
                if *self.peek() != Token::Semicolon {
                    return Err(Diagnostic::error(
                        "a multi-declarator function-pointer class member is not supported yet (roadmap)",
                    ));
                }
                self.advance();
                let field_type = Type::StructPointer { element_size: 0 };
                let align = type_alignment(field_type)
                    .max(u32::from(attribute_align))
                    .max(1);
                offset = offset.div_ceil(align) * align;
                layout.insert_field(
                    field_name.clone(),
                    StructField {
                        member_type: field_type,
                        source_fundamental: None,
                        offset,
                        struct_tag: None,
                        array_element: None,
                        array_bytes: None,
                        array_stride: None,
                        bit_field: None,
                    },
                );
                layout.function_pointer_fields.insert(field_name.clone());
                layout
                    .function_pointer_types
                    .insert(field_name.clone(), callback_type);
                class.fields.push(field_name);
                offset = offset.checked_add(type_size(field_type)).ok_or_else(|| {
                    Diagnostic::error("C++ class layout exceeds the 32-bit address space")
                })?;
                max_align = max_align.max(align);
                continue;
            }
            let field_name = self.parse_identifier()?;
            if *self.peek() == Token::ParenOpen {
                let signature = self.parse_class_parameter_types()?;
                let mut tail = self.position;
                let mut is_const_member = false;
                while matches!(self.tokens.get(tail), Some(Token::Identifier(word))
                    if matches!(word.as_str(), "const" | "override" | "final"))
                {
                    is_const_member |= matches!(self.tokens.get(tail), Some(Token::Identifier(word)) if word == "const");
                    tail += 1;
                }
                let is_pure = self.tokens.get(tail) == Some(&Token::Equals)
                    && self.tokens.get(tail + 1) == Some(&Token::IntegerLiteral(0));
                let is_inline = self.skip_class_method_tail()?;
                // Virtuality is inherited even when the overriding declaration
                // does not repeat the `virtual` keyword. Resolve the primary
                // base slot for every method; an explicit new virtual still
                // allocates a slot when no inherited declaration matches.
                let inherited_virtual = self.resolve_primary_base_virtual_override(
                    &class,
                    &field_name,
                    &signature.cxx_parameters,
                    is_const_member,
                )?;
                let virtual_dispatch = if is_virtual || inherited_virtual.is_some() {
                    let dispatch = if let Some(mut inherited) = inherited_virtual {
                        // Covariant returns retain the inherited slot while the
                        // call expression needs the derived declaration's type.
                        inherited.return_type = field_type;
                        inherited
                    } else {
                        let slot_offset = 8usize
                            .checked_add(class.virtual_slots.checked_mul(4).ok_or_else(|| {
                                Diagnostic::error("C++ primary vtable slot offset overflow")
                            })?)
                            .and_then(|offset| u16::try_from(offset).ok())
                            .ok_or_else(|| {
                                Diagnostic::error("C++ primary vtable slot offset overflow")
                            })?;
                        let vptr_offset = u16::try_from(class.vptr_offset.unwrap_or(0))
                            .map_err(|_| Diagnostic::error("C++ primary vptr offset overflow"))?;
                        class.virtual_slots += 1;
                        if let Some(primary) = class.vtable_components.first_mut() {
                            primary.virtual_slots = class.virtual_slots;
                        }
                        VirtualDispatch {
                            vptr_offset,
                            slot_offset,
                            return_type: field_type,
                            variadic: false,
                        }
                    };
                    if !is_pure {
                        let qualified = self.qualify_cxx_class_name(&name);
                        let scopes: Vec<&str> = qualified.split("::").collect();
                        let mangled = if is_const_member {
                            mangle_qualified_member_function_cv_typed(
                                &scopes,
                                &field_name,
                                &signature.cxx_parameters,
                                true,
                            )?
                        } else {
                            mangle_qualified_member_function_variadic_typed(
                                &scopes,
                                &field_name,
                                &signature.cxx_parameters,
                                false,
                            )?
                        };
                        class
                            .virtual_definitions
                            .push((dispatch.slot_offset, mangled));
                        if !is_inline && class.vtable_key_function.is_none() {
                            class.vtable_key_function = class
                                .virtual_definitions
                                .last()
                                .map(|(_, name)| name.clone());
                        }
                    }
                    Some(dispatch)
                } else {
                    None
                };
                class
                    .methods
                    .entry(field_name)
                    .or_default()
                    .push(MemberMethod {
                        parameters: signature.parameters,
                        cxx_parameters: signature.cxx_parameters,
                        return_struct_tag: struct_tag,
                        is_inline,
                        is_const_member,
                        virtual_dispatch,
                    });
                continue;
            }
            if matches!(self.peek(), Token::Colon) {
                return Err(Diagnostic::error(
                    "a C++ bit-field member is not supported yet (roadmap)",
                ));
            }
            let element_size = type_size(field_type);
            let array_extent = self.parse_array_declarator_extent(element_size)?;
            if *self.peek() != Token::Semicolon {
                return Err(Diagnostic::error(
                    "a multi-declarator class member is not supported yet (roadmap)",
                ));
            }
            self.advance();
            let align = type_alignment(field_type)
                .max(u32::from(attribute_align))
                .max(1);
            offset = offset.div_ceil(align) * align;
            let (field_size, array_element, array_bytes, array_stride) =
                if let Some((total_bytes, first_index_stride)) = array_extent {
                    let element = if matches!(
                        field_type,
                        Type::Struct { .. } | Type::Pointer(_) | Type::StructPointer { .. }
                    ) {
                        None
                    } else {
                        Some(pointee_of(field_type)?)
                    };
                    (
                        total_bytes,
                        element,
                        Some(total_bytes),
                        first_index_stride,
                    )
                } else {
                    (element_size, None, None, row_pointer_stride)
                };
            layout.insert_field(
                field_name.clone(),
                StructField {
                    member_type: field_type,
                    source_fundamental: None,
                    offset,
                    struct_tag,
                    array_element,
                    array_bytes,
                    array_stride,
                    bit_field: None,
                },
            );
            if field_is_function_pointer_typedef {
                layout.function_pointer_fields.insert(field_name.clone());
                if let Some(function_type) = field_function_type {
                    layout
                        .function_pointer_types
                        .insert(field_name.clone(), function_type);
                }
            }
            class.fields.push(field_name);
            offset = offset.checked_add(field_size).ok_or_else(|| {
                Diagnostic::error("C++ class layout exceeds the 32-bit address space")
            })?;
            max_align = max_align.max(align);
        }
        self.expect(Token::BraceClose)?;
        self.expect(Token::Semicolon)?;
        if !class.virtual_bases.is_empty() && class.vptr_offset.is_none() {
            // With no polymorphic non-virtual primary base, CodeWarrior gives
            // the virtual-inheriting region its own dispatch pointer after the
            // written members. The separate virtual-base pointer(s) remain at
            // their declaration positions.
            offset = offset.div_ceil(4) * 4;
            class.vptr_offset = Some(offset);
            if let Some(dispatch_base) = class
                .virtual_bases
                .first()
                .and_then(|base| self.cxx_classes.get(base))
            {
                class.virtual_slots = dispatch_base.virtual_slots;
                class.has_virtual_destructor = dispatch_base.has_virtual_destructor;
                class.virtual_destructor_slot = dispatch_base.virtual_destructor_slot;
            }
            class.vtable_components.push(VtableComponent {
                object_offset: 0,
                vptr_offset: offset,
                virtual_slots: class.virtual_slots,
                virtual_destructor_slot: class.virtual_destructor_slot,
            });
            offset = offset.checked_add(4).ok_or_else(|| {
                Diagnostic::error("C++ virtual-base layout exceeds the 32-bit address space")
            })?;
            max_align = max_align.max(4);
            class.is_polymorphic = true;
        }
        // Direct-base storage and this class's own members form the reusable
        // non-virtual region. A further-derived class starts its fields here,
        // then emits one copy of each inherited virtual base at its own tail.
        class.nonvirtual_size = offset.max(1).div_ceil(max_align) * max_align;
        offset = class.nonvirtual_size;
        let virtual_bases = class.virtual_bases.clone();
        let mut virtual_base_index = 0usize;
        for virtual_base in virtual_bases {
            let base = self
                .structs
                .get(&virtual_base)
                .cloned()
                .or_else(|| self.asserted_aggregate_layout(&virtual_base))
                .ok_or_else(|| {
                Diagnostic::error(format!(
                    "virtual base class '{virtual_base}' must be defined before '{name}'"
                ))
            })?;
            let base_align = u32::from(base.align).max(1);
            offset = offset.div_ceil(base_align) * base_align;
            let base_offset = offset;
            for (field_name, field) in base.fields_in_declaration_order() {
                if layout.fields.contains_key(field_name) {
                    continue;
                }
                layout.insert_field(
                    field_name.clone(),
                    StructField {
                        member_type: field.member_type,
                        source_fundamental: field.source_fundamental,
                        offset: base_offset + field.offset,
                        struct_tag: field.struct_tag.clone(),
                        array_element: field.array_element,
                        array_bytes: field.array_bytes,
                        array_stride: field.array_stride,
                        bit_field: field.bit_field,
                    },
                );
                if base.function_pointer_fields.contains(field_name) {
                    layout.function_pointer_fields.insert(field_name.clone());
                    if let Some(function_type) = base.function_pointer_types.get(field_name) {
                        layout
                            .function_pointer_types
                            .insert(field_name.clone(), function_type.clone());
                    }
                }
            }
            class.bases.insert(
                virtual_base_index,
                BaseClass {
                    name: virtual_base.clone(),
                    offset: base_offset,
                    is_virtual: true,
                },
            );
            virtual_base_index += 1;
            if let Some(base_class) = self.cxx_classes.get(&virtual_base) {
                class.is_polymorphic |= base_class.is_polymorphic;
                class.vtable_components.extend(base_class.vtable_components.iter().map(
                    |component| VtableComponent {
                        object_offset: base_offset + component.object_offset,
                        vptr_offset: base_offset + component.vptr_offset,
                        virtual_slots: component.virtual_slots,
                        virtual_destructor_slot: component.virtual_destructor_slot,
                    },
                ));
            }
            offset = offset.checked_add(base.size).ok_or_else(|| {
                Diagnostic::error("C++ virtual-base layout exceeds the 32-bit address space")
            })?;
            max_align = max_align.max(base_align);
        }
        // C++ gives an otherwise empty class size one. Empty-base optimization is
        // deliberately outside this subset.
        layout.source_tag = Some(name.clone());
        layout.size = offset.max(1).div_ceil(max_align) * max_align;
        layout.align = max_align as u8;
        Ok((name, layout, class))
    }

    /// Consume an ordinary C++ qualified type name. Base-specifiers need this
    /// narrower grammar rather than the expression parser: accepting exactly
    /// `identifier (:: identifier)*` prevents an inheritance colon from being
    /// confused with a declarator while covering nested class bases used by
    /// the reference projects.
    fn parse_cxx_qualified_identifier(&mut self) -> Compilation<String> {
        let mut name = self.parse_identifier()?;
        while *self.peek() == Token::Colon && *self.peek_at(1) == Token::Colon {
            self.advance();
            self.advance();
            name.push_str("::");
            name.push_str(&self.parse_identifier()?);
        }
        Ok(name)
    }

    /// Resolve `delete pointer` for a polymorphic class to the ABI's virtual
    /// deleting-destructor entry. The caller supplies the implicit `-1` destroy
    /// flag and null guard when building the normalized statement.
    pub(crate) fn resolve_virtual_deleting_destructor(
        &self,
        class_name: &str,
    ) -> Compilation<VirtualDispatch> {
        let class = self.cxx_classes.get(class_name).ok_or_else(|| {
            Diagnostic::error(format!("class layout for delete target '{class_name}' was not recovered"))
        })?;
        let slot_offset = class.virtual_destructor_slot.ok_or_else(|| {
            Diagnostic::error(format!(
                "delete of non-polymorphic class '{class_name}' is not supported yet (roadmap)"
            ))
        })?;
        let vptr_offset = u16::try_from(class.vptr_offset.unwrap_or(0))
            .map_err(|_| Diagnostic::error("C++ primary vptr offset overflow"))?;
        Ok(VirtualDispatch {
            vptr_offset,
            slot_offset,
            return_type: Type::Void,
            variadic: false,
        })
    }

    /// Resolve a placement-construction expression by source class and arity.
    /// The returned EABI constructor takes the placement address as its first,
    /// implicit `this` argument and returns that class pointer internally.
    pub(crate) fn resolve_placement_constructor(
        &self,
        class_name: &str,
        arguments: &[Expression],
    ) -> Compilation<String> {
        let resolved_class = self
            .resolve_scoped_cxx_class_name(class_name)
            .unwrap_or_else(|| class_name.to_owned());
        if arguments.is_empty() {
            let scopes = resolved_class.split("::").collect::<Vec<_>>();
            let default_constructor =
                mangle_qualified_member_function_typed(&scopes, "__ct", &[])?;
            if self.skipped_inline_names.contains(&default_constructor) {
                return Ok(default_constructor);
            }
        }
        let local_class = resolved_class
            .rsplit("::")
            .next()
            .unwrap_or(resolved_class.as_str());
        if let Some(class) = self
            .cxx_classes
            .get(&resolved_class)
            .or_else(|| self.cxx_classes.get(class_name))
            .or_else(|| self.cxx_classes.get(local_class))
        {
            let mut candidates = class
                .constructors
                .iter()
                .filter(|signature| signature.parameters.len() == arguments.len())
                .map(|signature| {
                    mangle_qualified_member_function_typed(
                        &resolved_class.split("::").collect::<Vec<_>>(),
                        "__ct",
                        &signature.cxx_parameters,
                    )
                })
                .collect::<Compilation<Vec<_>>>()?;
            candidates.sort();
            candidates.dedup();
            if let [constructor] = candidates.as_slice() {
                return Ok(constructor.clone());
            }
        }
        let qualified = resolved_class.as_str();
        let candidates: Vec<_> = self
            .cxx_constructors
            .get(qualified)
            .or_else(|| self.cxx_constructors.get(class_name))
            .or_else(|| self.cxx_constructors.get(local_class))
            .into_iter()
            .flatten()
            .filter(|method| method.fixed_parameter_count == arguments.len())
            .collect();
        let mut unique = candidates
            .iter()
            .map(|method| method.mangled.as_str())
            .collect::<Vec<_>>();
        unique.sort_unstable();
        unique.dedup();
        if let [constructor] = unique.as_slice() {
            return Ok((*constructor).to_owned());
        }
        if arguments.is_empty() && !unique.is_empty() {
            return Err(Diagnostic::error(format!(
                "constructor overload resolution for '{class_name}' is ambiguous among {unique:?} (roadmap)"
            )));
        }
        match candidates.as_slice() {
            [method] => Ok(method.mangled.clone()),
            _ => self
                .resolve_exact_cxx_overload(qualified, &candidates, arguments)?
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "constructor overload resolution for '{class_name}' is unavailable (roadmap)"
                    ))
                }),
        }
    }

    /// Materialize the compiler-generated default constructor needed by a
    /// placement-new expression. C++ suppresses this constructor when any
    /// source constructor is declared; otherwise its observable work is the
    /// ordered construction of bases and aggregate members followed by this
    /// class's vptr installation. Keeping the generated body in the ordinary
    /// inline-definition pool lets the existing inliner schedule the calls and
    /// stores exactly like a source-written inline constructor.
    pub(crate) fn ensure_implicit_default_constructor(
        &mut self,
        class_name: &str,
    ) -> Compilation<Option<String>> {
        let mut visiting = std::collections::HashSet::new();
        self.ensure_implicit_default_constructor_inner(class_name, &mut visiting)
    }

    fn ensure_implicit_default_constructor_inner(
        &mut self,
        class_name: &str,
        visiting: &mut std::collections::HashSet<String>,
    ) -> Compilation<Option<String>> {
        let resolved = self
            .resolve_scoped_cxx_class_name(class_name)
            .unwrap_or_else(|| class_name.to_owned());
        let scopes = resolved.split("::").collect::<Vec<_>>();
        let mangled = mangle_qualified_member_function_typed(&scopes, "__ct", &[])?;
        if self.skipped_inline_names.contains(&mangled) {
            return Ok(Some(mangled));
        }
        if !visiting.insert(resolved.clone()) {
            return Err(Diagnostic::error(format!(
                "recursive implicit default construction for '{resolved}'"
            )));
        }

        let class = self.cxx_classes.get(&resolved).cloned().ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for implicit constructor '{resolved}' was not recovered"
            ))
        })?;
        let source_constructors = self
            .cxx_constructors
            .get(&resolved)
            .map_or(false, |constructors| !constructors.is_empty());
        if !class.constructors.is_empty() || source_constructors {
            visiting.remove(&resolved);
            return Ok(None);
        }
        let layout = self.structs.get(&resolved).cloned().ok_or_else(|| {
            Diagnostic::error(format!(
                "aggregate layout for implicit constructor '{resolved}' was not recovered"
            ))
        })?;

        let adjusted_this = |offset: u32| {
            if offset == 0 {
                Expression::Variable("this".to_string())
            } else {
                Expression::MemberAddress {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset,
                    element: Pointee::UnsignedChar,
                    index_stride: None,
                }
            }
        };
        let mut statements = Vec::new();
        for base in &class.bases {
            let constructor = if self.has_declared_default_constructor(&base.name) {
                Some(self.resolve_placement_constructor(&base.name, &[])?)
            } else {
                self.ensure_implicit_default_constructor_inner(&base.name, visiting)?
            };
            if let Some(name) = constructor {
                statements.push(Statement::Expression(Expression::Call {
                    name,
                    arguments: vec![adjusted_this(base.offset)],
                }));
            }
        }
        for field_name in &class.fields {
            let Some(field) = layout.fields.get(field_name) else {
                continue;
            };
            let Some(field_class) = field.struct_tag.as_deref() else {
                continue;
            };
            if !self.cxx_classes.contains_key(field_class) {
                continue;
            }
            let constructor = if self.has_declared_default_constructor(field_class) {
                Some(self.resolve_placement_constructor(field_class, &[])?)
            } else {
                self.ensure_implicit_default_constructor_inner(field_class, visiting)?
            };
            if let Some(name) = constructor {
                statements.push(Statement::Expression(Expression::Call {
                    name,
                    arguments: vec![adjusted_this(field.offset)],
                }));
            }
        }

        if !class.vtable_components.is_empty() {
            let vtable = format!("__vt__{}", encode_qualified_scope(&scopes)?);
            let mut table_offset = 0u32;
            for component in &class.vtable_components {
                let address = Expression::AddressOf {
                    operand: Box::new(Expression::Variable(vtable.clone())),
                };
                let value = if table_offset == 0 {
                    address
                } else {
                    Expression::MemberAddress {
                        base: Box::new(address),
                        offset: table_offset,
                        element: Pointee::UnsignedChar,
                        index_stride: None,
                    }
                };
                statements.push(Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("this".to_string())),
                        offset: component.vptr_offset,
                        member_type: Type::UnsignedInt,
                        index_stride: None,
                    },
                    value,
                });
                table_offset += 8 + component.virtual_slots.max(1) as u32 * 4;
            }
        }

        visiting.remove(&resolved);
        if statements.is_empty() {
            return Ok(None);
        }
        let element_size = layout.size;
        self.skipped_inline_names.insert(mangled.clone());
        self.skipped_inline_definitions.push(Function {
            return_type: Type::StructPointer { element_size },
            name: mangled.clone(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer { element_size },
                name: "this".to_string(),
            }],
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("this".to_string())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        });
        Ok(Some(mangled))
    }

    /// Convert concrete aggregate lvalues passed to reference parameters into
    /// their EABI addresses. Source syntax omits `&`, but the syntax tree must
    /// retain the pointer value or codegen will try to load an entire aggregate
    /// into one argument register.
    pub(crate) fn lower_cxx_aggregate_reference_arguments(
        &self,
        parameters: &[Type],
        arguments: Vec<Expression>,
    ) -> Vec<Expression> {
        arguments
            .into_iter()
            .enumerate()
            .map(|(index, argument)| {
                if matches!(parameters.get(index), Some(Type::StructPointer { .. }))
                    && matches!(self.cxx_expression_type(&argument), Some(Type::Struct { .. }))
                {
                    Expression::AddressOf {
                        operand: Box::new(argument),
                    }
                } else {
                    argument
                }
            })
            .collect()
    }

    /// Resolve a placement constructor signature, then apply the same
    /// aggregate-reference conversion used by ordinary C++ calls.
    pub(crate) fn lower_placement_constructor_arguments(
        &self,
        class_name: &str,
        constructor_name: &str,
        arguments: Vec<Expression>,
    ) -> Vec<Expression> {
        let resolved = self
            .resolve_scoped_cxx_class_name(class_name)
            .unwrap_or_else(|| class_name.to_owned());
        let local = resolved.rsplit("::").next().unwrap_or(resolved.as_str());
        let Some(class) = self
            .cxx_classes
            .get(&resolved)
            .or_else(|| self.cxx_classes.get(class_name))
            .or_else(|| self.cxx_classes.get(local))
        else {
            return arguments;
        };
        let signatures = class
            .constructors
            .iter()
            .filter(|signature| signature.parameters.len() == arguments.len())
            .filter(|signature| {
                mangle_qualified_member_function_typed(
                    &resolved.split("::").collect::<Vec<_>>(),
                    "__ct",
                    &signature.cxx_parameters,
                )
                .is_ok_and(|name| name == constructor_name)
            })
            .collect::<Vec<_>>();
        let [signature] = signatures.as_slice() else {
            return arguments;
        };
        self.lower_cxx_aggregate_reference_arguments(&signature.parameters, arguments)
    }

    /// Whether source class metadata declares a zero-argument constructor.
    ///
    /// An automatic class object written without an initializer still invokes
    /// its default constructor when one is declared. POD structs must remain a
    /// storage-only declaration, so statement parsing uses this predicate
    /// before asking overload resolution for the constructor symbol.
    pub(crate) fn has_declared_default_constructor(&self, class_name: &str) -> bool {
        let resolved_class = self
            .resolve_scoped_cxx_class_name(class_name)
            .unwrap_or_else(|| class_name.to_owned());
        let local_class = resolved_class
            .rsplit("::")
            .next()
            .unwrap_or(resolved_class.as_str());
        let scopes = resolved_class.split("::").collect::<Vec<_>>();
        mangle_qualified_member_function_typed(&scopes, "__ct", &[])
            .ok()
            .is_some_and(|constructor| self.skipped_inline_names.contains(&constructor))
            || self.cxx_classes
            .get(&resolved_class)
            .or_else(|| self.cxx_classes.get(class_name))
            .or_else(|| self.cxx_classes.get(local_class))
            .is_some_and(|class| {
                class
                    .constructors
                    .iter()
                    .any(|signature| signature.parameters.is_empty())
            })
            || self
                .cxx_constructors
                .get(resolved_class.as_str())
                .or_else(|| self.cxx_constructors.get(class_name))
                .or_else(|| self.cxx_constructors.get(local_class))
                .is_some_and(|constructors| {
                    constructors
                        .iter()
                        .any(|constructor| constructor.fixed_parameter_count == 0)
                })
    }

    fn parse_class_parameter_types(&mut self) -> Compilation<ClassParameterTypes> {
        self.expect(Token::ParenOpen)?;
        let mut parameters = Vec::new();
        let mut cxx_parameters = Vec::new();
        if *self.peek() == Token::KeywordVoid && *self.peek_at(1) == Token::ParenClose {
            self.advance();
        } else {
            while *self.peek() != Token::ParenClose {
                let mut parameter_type = self.parse_type()?;
                let source_type = parameter_type;
                let is_wchar = self.last_type_was_wchar;
                let source_is_aggregate_value = self.last_type_was_aggregate_reference;
                self.last_array_typedef.take();
                let struct_tag = self.last_struct_tag.take();
                let enum_tag = self.last_enum_tag.take();
                let qualified_name = enum_tag.or_else(|| {
                    struct_tag.map(|tag| self.struct_typedefs.get(&tag).cloned().unwrap_or(tag))
                });
                let pointee_const = self.last_type_was_const;
                let pointer_const = self.last_pointer_const;
                let pointer_depth = self.last_cxx_pointer_depth;
                let pointer_base = self.last_cxx_pointer_base;
                let function_type = self.last_cxx_function_type.take();
                let source_identity = CxxParameterType::parsed(
                    source_type,
                    qualified_name,
                    is_wchar,
                    false,
                    source_is_aggregate_value,
                    pointee_const,
                    pointer_const,
                )
                .with_pointer_shape(pointer_depth, pointer_base)
                .with_function_type(function_type);
                let is_reference = self.eat_keyword(Token::Ampersand);
                if is_reference {
                    parameter_type = Type::StructPointer { element_size: 0 };
                }
                if let Some((_, name_position, callback_type)) = self
                    .try_cxx_function_pointer_declarator(source_identity.clone())?
                {
                    if let Some(name_position) = name_position {
                        self.record_named_parameter_at(name_position);
                    }
                    parameters.push(Type::StructPointer { element_size: 0 });
                    cxx_parameters.push(
                        CxxParameterType::plain(Type::StructPointer { element_size: 0 })
                            .with_pointer_shape(1, None)
                            .with_function_type(Some(callback_type)),
                    );
                } else {
                    if matches!(self.peek(), Token::Identifier(_)) {
                        let name_position = self.position;
                        self.advance();
                        self.record_named_parameter_at(name_position);
                    }
                    self.skip_cxx_default_argument()?;
                    parameters.push(parameter_type);
                    let mut source_identity = source_identity;
                    source_identity.is_reference = is_reference;
                    cxx_parameters.push(source_identity);
                }
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::ParenClose)?;
        Ok(ClassParameterTypes {
            parameters,
            cxx_parameters,
        })
    }

    /// Skip a parameter's default initializer while preserving the comma or
    /// closing parenthesis that terminates the parameter. Nested calls, array
    /// literals, and braced aggregate values may contain their own commas.
    fn skip_cxx_default_argument(&mut self) -> Compilation<()> {
        if !self.eat_keyword(Token::Equals) {
            return Ok(());
        }
        let mut parens = 0usize;
        let mut brackets = 0usize;
        let mut braces = 0usize;
        loop {
            match self.peek() {
                Token::Comma | Token::ParenClose if parens == 0 && brackets == 0 && braces == 0 => {
                    return Ok(());
                }
                Token::ParenOpen => parens += 1,
                Token::ParenClose => parens = parens.saturating_sub(1),
                Token::BracketOpen => brackets += 1,
                Token::BracketClose => brackets = brackets.saturating_sub(1),
                Token::BraceOpen => braces += 1,
                Token::BraceClose => braces = braces.saturating_sub(1),
                Token::EndOfFile => {
                    return Err(Diagnostic::error(
                        "unterminated C++ default parameter initializer",
                    ));
                }
                _ => {}
            }
            self.advance();
        }
    }

    fn skip_class_method_tail(&mut self) -> Compilation<bool> {
        while matches!(self.peek(), Token::Identifier(word)
            if matches!(word.as_str(), "const" | "override" | "final"))
        {
            self.advance();
        }
        if self.eat_keyword(Token::Equals) {
            self.advance();
        }
        if self.eat_keyword(Token::Semicolon) {
            return Ok(false);
        }
        if self.eat_keyword(Token::Colon) {
            // An in-class constructor may carry a member/base initializer
            // list before its body: `C() : x(0), y(1) {}`. Layout recovery
            // needs only the declarations, so consume each designator and its
            // balanced initializer without parsing the initializer values.
            loop {
                let mut saw_designator = false;
                while !matches!(
                    self.peek(),
                    Token::ParenOpen | Token::BraceOpen | Token::EndOfFile
                ) {
                    if *self.peek() == Token::Comma {
                        return Err(Diagnostic::error(
                            "constructor initializer is missing its value",
                        ));
                    }
                    saw_designator = true;
                    self.advance();
                }
                match self.peek() {
                    Token::ParenOpen => {
                        self.skip_balanced(Token::ParenOpen, Token::ParenClose)?;
                    }
                    Token::BraceOpen if saw_designator => {
                        self.skip_balanced(Token::BraceOpen, Token::BraceClose)?;
                    }
                    _ => {
                        return Err(Diagnostic::error(
                            "unterminated C++ constructor initializer list",
                        ));
                    }
                }
                if self.eat_keyword(Token::Comma) {
                    continue;
                }
                if *self.peek() == Token::BraceOpen {
                    self.skip_balanced(Token::BraceOpen, Token::BraceClose)?;
                    return Ok(true);
                }
                return Err(Diagnostic::error(
                    "constructor initializer list must be followed by a body",
                ));
            }
        }
        if *self.peek() == Token::BraceOpen {
            self.skip_balanced(Token::BraceOpen, Token::BraceClose)?;
            return Ok(true);
        }
        Err(Diagnostic::error(format!(
            "unsupported C++ member declaration tail: {}",
            self.peek()
        )))
    }

    pub(crate) fn skip_class_member(&mut self) -> Compilation<()> {
        let mut parens = 0usize;
        while *self.peek() != Token::EndOfFile {
            match self.advance() {
                Token::ParenOpen => parens += 1,
                Token::ParenClose => parens = parens.saturating_sub(1),
                Token::Semicolon if parens == 0 => return Ok(()),
                Token::BraceOpen if parens == 0 => {
                    self.position -= 1;
                    return self.skip_balanced(Token::BraceOpen, Token::BraceClose);
                }
                _ => {}
            }
        }
        Err(Diagnostic::error("unterminated C++ member declaration"))
    }

    fn skip_balanced(&mut self, open: Token, close: Token) -> Compilation<()> {
        self.expect(open.clone())?;
        let mut depth = 1usize;
        while depth > 0 {
            let token = self.advance();
            if token == open {
                depth += 1;
            } else if token == close {
                depth -= 1;
            } else if token == Token::EndOfFile {
                return Err(Diagnostic::error("unterminated C++ declaration"));
            }
        }
        Ok(())
    }

    /// Consume a constructor initializer list and lower it to ordinary IR in
    /// language-mandated order: bases first, then members in declaration order.
    pub(crate) fn parse_constructor_initializers(
        &mut self,
        scope: &str,
        parameters: &[mwcc_syntax_trees::Parameter],
    ) -> Compilation<ConstructorInitialization> {
        let class = self.cxx_classes.get(scope).ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for constructor '{scope}' was not recovered"
            ))
        })?;
        let bases = class.bases.clone();
        let field_names = class.fields.clone();
        let mut initializers = std::collections::HashMap::new();
        if self.eat_keyword(Token::Colon) {
            loop {
                let target = self.parse_identifier()?;
                self.expect(Token::ParenOpen)?;
                let mut arguments = Vec::new();
                if *self.peek() != Token::ParenClose {
                    loop {
                        arguments.push(self.expression()?);
                        if !self.eat_keyword(Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Token::ParenClose)?;
                if initializers.insert(target.clone(), arguments).is_some() {
                    return Err(Diagnostic::error(format!(
                        "duplicate constructor initializer for '{target}'"
                    )));
                }
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }

        let mut statements = Vec::new();
        for base in bases {
            let arguments = initializers
                .remove(&base.name)
                .or_else(|| {
                    let unqualified = base.name.rsplit("::").next()?;
                    (unqualified != base.name)
                        .then(|| initializers.remove(unqualified))
                        .flatten()
                })
                .unwrap_or_default();
            let Some(base_class) = self.cxx_classes.get(&base.name) else {
                // A C aggregate may be used as a C++ base (SDK vector wrappers
                // do this heavily). It has no constructor semantics, so its
                // implicit default initialization is intentionally empty.
                if arguments.is_empty() {
                    continue;
                }
                return Err(Diagnostic::error(format!(
                    "constructor arguments for non-class base '{}' are not supported (roadmap)",
                    base.name
                )));
            };
            let signatures = &base_class.constructors;
            let candidates: Vec<&ClassParameterTypes> = signatures
                .iter()
                .filter(|signature| signature.parameters.len() == arguments.len())
                .collect();
            // A non-polymorphic base with no declared constructor is trivially
            // default-constructed and emits no call. A polymorphic base still
            // has a compiler-generated default constructor whose observable
            // work installs its vptr; materialize that work directly so inline
            // expansion does not silently drop the base construction.
            if candidates.is_empty() && arguments.is_empty() {
                if !base.is_virtual && !base_class.vtable_components.is_empty() {
                    let scopes: Vec<&str> = base.name.split("::").collect();
                    let vtable = format!(
                        "__vt__{}",
                        crate::cxx::encode_qualified_scope(&scopes)?
                    );
                    let mut table_offset = 0u32;
                    for component in &base_class.vtable_components {
                        let address = Expression::AddressOf {
                            operand: Box::new(Expression::Variable(vtable.clone())),
                        };
                        let value = if table_offset == 0 {
                            address
                        } else {
                            Expression::MemberAddress {
                                base: Box::new(address),
                                offset: table_offset,
                                element: Pointee::UnsignedChar,
                                index_stride: None,
                            }
                        };
                        statements.push(Statement::Store {
                            target: Expression::Member {
                                base: Box::new(Expression::Variable("this".to_string())),
                                offset: base.offset + component.vptr_offset,
                                member_type: Type::UnsignedInt,
                                index_stride: None,
                            },
                            value,
                        });
                        table_offset += 8 + component.virtual_slots.max(1) as u32 * 4;
                    }
                }
                continue;
            }
            if candidates.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "constructor overload resolution for '{}' is ambiguous or unavailable (roadmap)",
                    base.name
                )));
            }
            let name = self.mangle_typed_member_in_current_namespace(
                base.name.as_str(),
                "__ct",
                &candidates[0].cxx_parameters,
            )?;
            let this = if base.offset == 0 {
                Expression::Variable("this".to_string())
            } else {
                Expression::MemberAddress {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: base.offset,
                    element: mwcc_syntax_trees::Pointee::UnsignedChar,
                    index_stride: None,
                }
            };
            let mut call_arguments = vec![this];
            call_arguments.extend(arguments);
            statements.push(Statement::Expression(Expression::Call {
                name,
                arguments: call_arguments,
            }));
        }
        let vptr_insertion_index = statements.len();
        let layout = self.structs.get(scope).cloned().ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for constructor '{scope}' was not recovered"
            ))
        })?;
        for field_name in field_names {
            let field = layout.fields.get(&field_name).ok_or_else(|| {
                Diagnostic::error(format!(
                    "member '{field_name}' is absent from class '{scope}'"
                ))
            })?;
            let Some(mut arguments) = initializers.remove(&field_name) else {
                let Some(field_class) = field.struct_tag.as_deref() else {
                    continue;
                };
                // Arrays require one construction per element. Do not mistake
                // their aggregate tag for a single class subobject.
                if field.array_element.is_some() || !self.cxx_classes.contains_key(field_class) {
                    continue;
                }
                let constructor = if self.has_declared_default_constructor(field_class) {
                    Some(self.resolve_placement_constructor(field_class, &[])?)
                } else {
                    self.ensure_implicit_default_constructor(field_class)?
                };
                if let Some(name) = constructor {
                    let this = if field.offset == 0 {
                        Expression::Variable("this".to_string())
                    } else {
                        Expression::MemberAddress {
                            base: Box::new(Expression::Variable("this".to_string())),
                            offset: field.offset,
                            element: Pointee::UnsignedChar,
                            index_stride: None,
                        }
                    };
                    statements.push(Statement::Expression(Expression::Call {
                        name,
                        arguments: vec![this],
                    }));
                }
                continue;
            };
            let aggregate_copy = !matches!(field.member_type, Type::Struct { .. })
                || matches!(
                    arguments.as_slice(),
                    [Expression::Variable(source)]
                        if matches!(
                            parameters
                                .iter()
                                .find(|parameter| parameter.name == *source)
                                .map(|parameter| parameter.parameter_type),
                            Some(Type::Struct { .. } | Type::StructPointer { .. })
                        )
                );
            if arguments.len() != 1 || !aggregate_copy {
                return Err(Diagnostic::error(format!(
                    "non-scalar constructor initialization for '{field_name}' is not supported yet (roadmap)"
                )));
            }
            // Preserve the scalar field types of a three-float aggregate copy.
            // Collapsing `Vector3f` to an opaque 12-byte store makes the backend
            // use integer word copies; MWCC retains the member graph and issues
            // lfs/stfs operations that its constructor scheduler can interleave.
            if let (
                Type::Struct { size: 12, .. },
                Some(struct_tag),
                [Expression::Variable(source)],
            ) = (field.member_type, field.struct_tag.as_ref(), arguments.as_slice())
            {
                let source_matches = parameters.iter().any(|parameter| {
                    parameter.name == *source
                        && matches!(
                            parameter.parameter_type,
                            Type::Struct { size: 12, .. }
                                | Type::StructPointer { .. }
                        )
                });
                let components = self.structs.get(struct_tag).map(|layout| {
                    layout
                        .field_order
                        .iter()
                        .filter_map(|name| layout.fields.get(name))
                        .collect::<Vec<_>>()
                });
                if source_matches
                    && components.as_ref().is_some_and(|components| {
                        components.len() == 3
                            && components.iter().enumerate().all(|(index, component)| {
                                component.offset == index as u32 * 4
                                    && component.member_type == Type::Float
                                    && component.array_element.is_none()
                                    && component.bit_field.is_none()
                            })
                    })
                {
                    for component in components.expect("the shape was checked") {
                        statements.push(Statement::Store {
                            target: Expression::Member {
                                base: Box::new(Expression::Variable("this".to_string())),
                                offset: field.offset + component.offset,
                                member_type: Type::Float,
                                index_stride: None,
                            },
                            value: Expression::Member {
                                base: Box::new(Expression::Variable(source.clone())),
                                offset: component.offset,
                                member_type: Type::Float,
                                index_stride: None,
                            },
                        });
                    }
                    continue;
                }
            }
            statements.push(Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: field.offset,
                    member_type: field.member_type,
                    index_stride: None,
                },
                value: arguments.remove(0),
            });
        }
        if let Some(unknown) = initializers.keys().next() {
            return Err(Diagnostic::error(format!(
                "unknown constructor initializer '{unknown}' in class '{scope}'"
            )));
        }
        Ok(ConstructorInitialization {
            statements,
            vptr_insertion_index,
        })
    }

    /// Synthesize non-virtual base destruction in language-mandated reverse
    /// declaration order. Base destructors receive deleting flag zero: only
    /// the complete-object destructor may invoke operator delete.
    pub(crate) fn synthesize_base_destructor_calls(
        &self,
        scope: &str,
    ) -> Compilation<Vec<Statement>> {
        let class = self.cxx_classes.get(scope).ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for destructor '{scope}' was not recovered"
            ))
        })?;
        let mut statements = Vec::new();
        for base in class.bases.iter().rev() {
            let Some(base_class) = self.cxx_classes.get(&base.name) else {
                continue;
            };
            if !base_class.has_virtual_destructor {
                continue;
            }
            let this = if base.offset == 0 {
                Expression::Variable("this".to_string())
            } else {
                Expression::MemberAddress {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: base.offset,
                    element: mwcc_syntax_trees::Pointee::UnsignedChar,
                    index_stride: None,
                }
            };
            statements.push(Statement::Expression(Expression::Call {
                name: self.mangle_typed_member_in_current_namespace(
                    &base.name,
                    "__dt",
                    &[],
                )?,
                arguments: vec![this, Expression::IntegerLiteral(0)],
            }));
        }
        Ok(statements)
    }
}

/// Mangle an ordinary, singly-qualified member function.
///
/// Examples measured from mwcceppc:
/// `void KartCannon::Init(int)` -> `Init__10KartCannonFi`
/// `void KartCannon::DoKeep()` -> `DoKeep__10KartCannonFv`
pub(crate) fn mangle_member_function(
    scope: &str,
    function: &str,
    explicit_parameters: &[Type],
) -> Compilation<String> {
    mangle_qualified_member_function(&[scope], function, explicit_parameters)
}

/// Mangle a class member qualified by one or more scopes. CodeWarrior encodes
/// one class directly (`7Counter`) and nested namespace/class names as
/// `Q<count><length><name>...` (`Q26sample7Counter`).
pub(crate) fn mangle_qualified_member_function(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[Type],
) -> Compilation<String> {
    let parameters: Vec<CxxParameterType> = explicit_parameters
        .iter()
        .copied()
        .map(CxxParameterType::plain)
        .collect();
    mangle_qualified_member_function_variadic_typed(scopes, function, &parameters, false)
}

/// Mangle a static data member. For example, `Game::Creature::enabled`
/// becomes `enabled__Q24Game8Creature`.
pub(crate) fn mangle_qualified_data_member(scopes: &[&str], member: &str) -> Compilation<String> {
    if member.is_empty() {
        return Err(Diagnostic::error(
            "an empty C++ data-member name is invalid",
        ));
    }
    let qualified_scope = encode_qualified_scope(scopes)?;
    Ok(format!("{member}__{qualified_scope}"))
}

fn mangle_qualified_member_function_typed(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[CxxParameterType],
) -> Compilation<String> {
    mangle_qualified_member_function_cv_typed(scopes, function, explicit_parameters, false)
}

fn mangle_qualified_member_function_cv_typed(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[CxxParameterType],
    is_const: bool,
) -> Compilation<String> {
    if function.is_empty() {
        return Err(Diagnostic::error("an empty C++ member name is invalid"));
    }
    let arguments = encode_function_arguments(explicit_parameters, false)?;
    let qualified_scope = encode_qualified_scope(scopes)?;
    let cv = if is_const { "C" } else { "" };
    Ok(format!("{function}__{qualified_scope}{cv}F{arguments}"))
}

fn mangle_qualified_member_function_variadic_typed(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[CxxParameterType],
    variadic: bool,
) -> Compilation<String> {
    if function.is_empty() {
        return Err(Diagnostic::error("an empty C++ member name is invalid"));
    }
    let arguments = encode_function_arguments(explicit_parameters, variadic)?;
    let qualified_scope = encode_qualified_scope(scopes)?;
    Ok(format!("{function}__{qualified_scope}F{arguments}"))
}

fn encode_function_arguments(
    explicit_parameters: &[CxxParameterType],
    variadic: bool,
) -> Compilation<String> {
    let mut arguments = if explicit_parameters.is_empty() && !variadic {
        "v".to_string()
    } else {
        explicit_parameters
            .iter()
            .map(encode_type)
            .collect::<Compilation<Vec<_>>>()?
            .concat()
    };
    if variadic {
        arguments.push('e');
    }
    Ok(arguments)
}

pub(crate) fn encode_qualified_scope(scopes: &[&str]) -> Compilation<String> {
    if scopes.is_empty() || scopes.iter().any(|scope| scope.is_empty()) {
        return Err(Diagnostic::error("an empty qualified C++ scope is invalid"));
    }
    if scopes.len() == 1 {
        Ok(format!("{}{}", scopes[0].len(), scopes[0]))
    } else {
        let components = scopes
            .iter()
            .map(|scope| format!("{}{scope}", scope.len()))
            .collect::<String>();
        Ok(format!("Q{}{components}", scopes.len()))
    }
}

fn encode_type(parameter: &CxxParameterType) -> Compilation<String> {
    let mut code = String::new();
    if parameter.is_reference {
        code.push('R');
        // A top-level const pointer matters only through a reference (`T* const&`).
        if parameter.pointer_const {
            code.push('C');
        }
    }
    let is_pointer = parameter.pointer_depth != 0;
    for _ in 0..parameter.pointer_depth {
        code.push('P');
    }
    // Leading const binds the referred object or a pointer's pointee. Top-level
    // const on a by-value parameter is absent from the function type.
    if parameter.pointee_const && (parameter.is_reference || is_pointer) {
        code.push('C');
    }
    if parameter.is_wchar {
        code.push('w');
        return Ok(code);
    }
    if let Some(function_type) = parameter.function_type.as_deref() {
        code.push('F');
        code.push_str(&encode_function_arguments(
            &function_type.parameters,
            function_type.variadic,
        )?);
        code.push('_');
        code.push_str(&encode_type(&function_type.return_type)?);
        return Ok(code);
    }
    if let Some(name) = parameter.qualified_name.as_deref() {
        code.push_str(&encode_qualified_type_name(name)?);
        return Ok(code);
    }
    let encoded_source = parameter.pointer_base.unwrap_or(parameter.source_type);
    let source_spelling = match parameter.source_fundamental {
        Some(SourceFundamentalType::PlainChar) => Some("c"),
        Some(SourceFundamentalType::SignedLong) => Some("l"),
        Some(SourceFundamentalType::UnsignedLong) => Some("Ul"),
        _ => None,
    };
    if let Some(spelling) = source_spelling {
        code.push_str(spelling);
        return Ok(code);
    }
    let base = match encoded_source {
        Type::Int => "i".to_string(),
        Type::UnsignedInt => "Ui".to_string(),
        Type::Char => "c".to_string(),
        Type::UnsignedChar => "Uc".to_string(),
        Type::Short => "s".to_string(),
        Type::UnsignedShort => "Us".to_string(),
        Type::Float => "f".to_string(),
        Type::Double => "d".to_string(),
        Type::LongLong => "x".to_string(),
        Type::UnsignedLongLong => "Ux".to_string(),
        Type::Pointer(pointee) if parameter.pointer_base.is_none() => {
            encode_pointee(pointee)?.to_string()
        }
        Type::Pointer(_) => {
            return Err(Diagnostic::error(
                "a nested C++ pointer base is not supported yet (roadmap)",
            ))
        }
        Type::Void => "v".to_string(),
        Type::StructPointer { .. } | Type::Struct { .. } => {
            return Err(Diagnostic::error(
                "a struct-valued C++ member parameter needs qualified type mangling (roadmap)",
            ))
        }
    };
    code.push_str(&base);
    Ok(code)
}

/// Encode a concrete template type argument using the same CodeWarrior ABI
/// alphabet as function parameters. Template layout recovery owns storage;
/// symbol spelling remains centralized here so `Vector3<int>` becomes
/// `Vector3<i>` rather than a parser-specific placeholder.
pub(crate) fn encode_template_argument_type(argument: Type) -> Option<String> {
    encode_type(&CxxParameterType::plain(argument)).ok()
}

pub(crate) fn encode_qualified_type_name(name: &str) -> Compilation<String> {
    let scopes: Vec<&str> = name.split("::").collect();
    encode_qualified_scope(&scopes)
}

fn encode_pointee(pointee: Pointee) -> Compilation<&'static str> {
    match pointee {
        Pointee::Int => Ok("i"),
        Pointee::UnsignedInt => Ok("Ui"),
        Pointee::Char => Ok("c"),
        Pointee::UnsignedChar => Ok("Uc"),
        Pointee::Short => Ok("s"),
        Pointee::UnsignedShort => Ok("Us"),
        Pointee::Float => Ok("f"),
        Pointee::Double => Ok("d"),
        Pointee::LongLong => Ok("x"),
        Pointee::UnsignedLongLong => Ok("Ux"),
        Pointee::Pointer | Pointee::WordPointer => Err(Diagnostic::error(
            "a pointer-to-pointer C++ member parameter needs exact pointee mangling (roadmap)",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_tokens::SourceLocation;

    fn locate(tokens: Vec<Token>) -> Vec<LocatedToken> {
        tokens
            .into_iter()
            .enumerate()
            .map(|(index, token)| LocatedToken {
                token,
                location: SourceLocation {
                    byte_offset: index as u32,
                    line: 1,
                    column: index as u32 + 1,
                },
            })
            .collect()
    }

    fn strip(tokens: Vec<LocatedToken>) -> Vec<Token> {
        tokens.into_iter().map(|located| located.token).collect()
    }

    #[test]
    fn scopes_block_linkage_without_losing_declarations() {
        let tokens = vec![
            Token::Identifier("extern".to_string()),
            Token::StringLiteral(b"C".to_vec()),
            Token::BraceOpen,
            Token::KeywordInt,
            Token::Identifier("value".to_string()),
            Token::Semicolon,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        assert_eq!(
            strip(normalize_linkage_specifications(locate(tokens))),
            vec![
                Token::Pragma("push".to_string()),
                Token::Pragma("cplusplus off".to_string()),
                Token::KeywordInt,
                Token::Identifier("value".to_string()),
                Token::Semicolon,
                Token::Pragma("pop".to_string()),
                Token::EndOfFile,
            ]
        );
    }

    #[test]
    fn mangles_measured_member_shapes() {
        assert_eq!(
            mangle_member_function("KartCannon", "Init", &[Type::Int]).unwrap(),
            "Init__10KartCannonFi"
        );
        assert_eq!(
            mangle_member_function("KartCannon", "DoKeep", &[]).unwrap(),
            "DoKeep__10KartCannonFv"
        );
        assert_eq!(
            mangle_member_function("Counter", "__ct", &[Type::Int, Type::Short]).unwrap(),
            "__ct__7CounterFis"
        );
        assert_eq!(
            mangle_qualified_member_function(
                &["homebutton", "FrameController"],
                "init",
                &[Type::Int, Type::Float, Type::Float, Type::Float],
            )
            .unwrap(),
            "init__Q210homebutton15FrameControllerFifff"
        );
    }

    #[test]
    fn mangles_static_data_members_without_a_function_suffix() {
        assert_eq!(
            mangle_qualified_data_member(&["Game", "Creature"], "usePacketCulling").unwrap(),
            "usePacketCulling__Q24Game8Creature"
        );
        assert_eq!(
            mangle_qualified_data_member(&["Counter"], "value").unwrap(),
            "value__7Counter"
        );
    }

    #[test]
    fn mangles_named_value_pointer_reference_and_cv_layers() {
        let named = |storage_type, is_reference, pointee_const, pointer_const| {
            CxxParameterType::parsed(
                storage_type,
                Some("JUtility::TColor".to_string()),
                false,
                is_reference,
                is_reference && matches!(storage_type, Type::Struct { .. }),
                pointee_const,
                pointer_const,
            )
        };
        let value = named(Type::Struct { size: 4, align: 4 }, false, false, false);
        let pointer = named(Type::StructPointer { element_size: 4 }, false, true, false);
        let reference = named(Type::Struct { size: 4, align: 4 }, true, true, false);
        let const_pointer_reference =
            named(Type::StructPointer { element_size: 4 }, true, true, true);
        assert_eq!(
            mangle_qualified_member_function_typed(&["A"], "v", &[value]).unwrap(),
            "v__1AFQ28JUtility6TColor"
        );
        assert_eq!(
            mangle_qualified_member_function_typed(&["A"], "p", &[pointer]).unwrap(),
            "p__1AFPCQ28JUtility6TColor"
        );
        assert_eq!(
            mangle_qualified_member_function_typed(&["A"], "r", &[reference]).unwrap(),
            "r__1AFRCQ28JUtility6TColor"
        );
        assert_eq!(
            mangle_qualified_member_function_typed(&["A"], "q", &[const_pointer_reference],)
                .unwrap(),
            "q__1AFRCPCQ28JUtility6TColor"
        );
    }

    #[test]
    fn mangles_scalar_pointer_depth_without_widening_the_storage_ir() {
        let char_pointer_pointer = CxxParameterType::parsed(
            Type::Pointer(Pointee::Pointer),
            None,
            false,
            false,
            false,
            false,
            false,
        )
        .with_pointer_shape(2, Some(Type::Char));
        let void_pointer = CxxParameterType::parsed(
            Type::Pointer(Pointee::Int),
            None,
            false,
            false,
            false,
            false,
            false,
        )
        .with_pointer_shape(1, Some(Type::Void));
        assert_eq!(encode_type(&char_pointer_pointer).unwrap(), "PPc");
        assert_eq!(encode_type(&void_pointer).unwrap(), "Pv");
    }

    #[test]
    fn adds_internal_return_type_only_to_out_of_class_constructors() {
        let tokens = vec![
            Token::Identifier("class".to_string()),
            Token::Identifier("Counter".to_string()),
            Token::BraceOpen,
            Token::Identifier("Counter".to_string()),
            Token::ParenOpen,
            Token::KeywordInt,
            Token::ParenClose,
            Token::Semicolon,
            Token::BraceClose,
            Token::Semicolon,
            Token::Identifier("Counter".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("Counter".to_string()),
            Token::ParenOpen,
            Token::KeywordInt,
            Token::ParenClose,
            Token::BraceOpen,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        let normalized = strip(normalize_constructor_declarators(locate(tokens)));
        assert_eq!(
            normalized
                .iter()
                .filter(|token| **token == Token::KeywordVoid)
                .count(),
            1
        );
        let constructor = normalized
            .windows(6)
            .find(|window| {
                window[0] == Token::KeywordVoid
                    && matches!(&window[1], Token::Identifier(name) if name == "Counter")
            })
            .unwrap();
        assert_eq!(constructor[2], Token::Colon);
        assert_eq!(constructor[3], Token::Colon);
    }

    #[test]
    fn adds_internal_return_type_to_namespace_scoped_constructors() {
        let tokens = vec![
            Token::Identifier("namespace".to_string()),
            Token::Identifier("Game".to_string()),
            Token::BraceOpen,
            Token::Identifier("Creature".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("Creature".to_string()),
            Token::ParenOpen,
            Token::ParenClose,
            Token::BraceOpen,
            Token::BraceClose,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        let normalized = strip(normalize_constructor_declarators(locate(tokens)));
        assert!(normalized.windows(6).any(|window| {
            window[0] == Token::KeywordVoid
                && matches!(&window[1], Token::Identifier(name) if name == "Creature")
                && window[2] == Token::Colon
                && window[3] == Token::Colon
        }));
    }

    #[test]
    fn adds_internal_return_type_to_fully_qualified_special_members() {
        let tokens = vec![
            Token::Identifier("zen".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("AlphaWipe".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("AlphaWipe".to_string()),
            Token::ParenOpen,
            Token::ParenClose,
            Token::BraceOpen,
            Token::BraceClose,
            Token::Identifier("zen".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("AlphaWipe".to_string()),
            Token::Colon,
            Token::Colon,
            Token::Tilde,
            Token::Identifier("AlphaWipe".to_string()),
            Token::ParenOpen,
            Token::ParenClose,
            Token::BraceOpen,
            Token::BraceClose,
            Token::EndOfFile,
        ];
        let normalized = strip(normalize_constructor_declarators(locate(tokens)));
        assert_eq!(
            normalized
                .iter()
                .filter(|token| **token == Token::KeywordVoid)
                .count(),
            2
        );
        assert!(normalized
            .iter()
            .any(|token| matches!(token, Token::Identifier(name) if name == "__dt")));
        assert!(!normalized.iter().any(|token| *token == Token::Tilde));
    }
}
