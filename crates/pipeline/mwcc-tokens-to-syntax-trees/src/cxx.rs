//! Metrowerks C++ surface syntax kept out of the general C item parser.
//!
//! Linkage specifications are declaration wrappers, not declarations themselves;
//! normalization removes those wrappers before recursive descent. Symbol names
//! use CodeWarrior's own mangling rather than the Itanium ABI.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Pointee, Statement, Type};
use mwcc_tokens::{LocatedToken, Token};

use crate::items::{type_alignment, type_size};
use crate::parser::{Parser, StructField, StructLayout};

/// The C++-only information that a plain C struct layout cannot retain.
/// Declaration order controls constructor initialization order, while base
/// names distinguish a base initializer from an identically shaped member.
#[derive(Default)]
pub(crate) struct ClassLayout {
    pub(crate) bases: Vec<BaseClass>,
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
    /// Whether the class declares a virtual destructor. Its out-of-line
    /// definition is a key function and owns the primary vtable in the subset
    /// currently materialized by the frontend.
    pub(crate) has_virtual_destructor: bool,
}

pub(crate) struct MemberMethod {
    pub(crate) parameters: Vec<Type>,
    cxx_parameters: Vec<CxxParameterType>,
    pub(crate) is_inline: bool,
}

pub(crate) struct ClassParameterTypes {
    pub(crate) parameters: Vec<Type>,
    pub(crate) cxx_parameters: Vec<CxxParameterType>,
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
}

/// One entry in CodeWarrior's primary virtual table. Slot offsets include the
/// two-word ABI header: the first callable entry is therefore byte offset 8.
#[derive(Clone)]
pub(crate) struct RecoveredCxxVirtualMethod {
    pub(crate) return_type: Type,
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
    pub(crate) methods:
        std::collections::HashMap<String, Vec<RecoveredCxxVirtualMethod>>,
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

/// The C++ ABI identity of one source parameter. The general syntax-tree
/// [`Type`] intentionally describes storage and register class only; it cannot
/// distinguish `A*` from `B*`, or a reference from its pointer-shaped calling
/// convention. Name mangling needs those distinctions, so they live in this
/// declaration-only companion instead of leaking into C code generation.
#[derive(Clone)]
pub(crate) struct CxxParameterType {
    source_type: Type,
    qualified_name: Option<String>,
    is_wchar: bool,
    is_reference: bool,
    pointee_const: bool,
    pointer_const: bool,
    pointer_depth: u8,
    pointer_base: Option<Type>,
}

impl CxxParameterType {
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

    pub(crate) fn plain(source_type: Type) -> Self {
        Self::parsed(source_type, None, false, false, false, false, false)
    }
}

pub(crate) struct BaseClass {
    pub(crate) name: String,
}

/// Normalize C++ linkage specifications into the same scoped language pragmas
/// the top-level parser already understands. The braces are declaration
/// wrappers rather than C++ scopes, while a single-declaration form retains
/// `extern` as its storage class.
pub(crate) fn normalize_linkage_specifications(
    mut tokens: Vec<LocatedToken>,
) -> Vec<LocatedToken> {
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
                    tokens.get(index.wrapping_sub(1)).map(|located| &located.token),
                    Some(Token::Identifier(_))
                ));
                declaration_scopes.push(opens_namespace);
            }
            Token::BraceClose => {
                declaration_scopes.pop();
            }
            _ => {}
        }
        let constructor = declaration_scopes.iter().all(|scope| *scope)
            && matches!((&tokens[index].token, &tokens[index + 3].token),
                (Token::Identifier(scope), Token::Identifier(name)) if scope == name)
            && tokens[index + 1].token == Token::Colon
            && tokens[index + 2].token == Token::Colon
            && tokens[index + 4].token == Token::ParenOpen;
        let destructor = index + 5 < tokens.len()
            && declaration_scopes.iter().all(|scope| *scope)
            && matches!((&tokens[index].token, &tokens[index + 4].token),
                (Token::Identifier(scope), Token::Identifier(name)) if scope == name)
            && tokens[index + 1].token == Token::Colon
            && tokens[index + 2].token == Token::Colon
            && tokens[index + 3].token == Token::Tilde
            && tokens[index + 5].token == Token::ParenOpen;
        if constructor {
            let location = tokens[index].location;
            tokens.insert(
                index,
                LocatedToken {
                    token: Token::KeywordVoid,
                    location,
                },
            );
            index += 6;
        } else if destructor {
            let location = tokens[index].location;
            tokens[index + 3].token = Token::Identifier("__dt".to_string());
            tokens.remove(index + 4);
            tokens.insert(
                index,
                LocatedToken {
                    token: Token::KeywordVoid,
                    location,
                },
            );
            index += 6;
        } else {
            index += 1;
        }
    }
    tokens
}

impl Parser {
    fn named_namespace_scopes(&self) -> Vec<&str> {
        self.namespace_stack
            .iter()
            .map(String::as_str)
            .filter(|scope| !scope.is_empty())
            .collect()
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
            });
        }
    }

    pub(crate) fn register_qualified_free_cxx_function(
        &mut self,
        scope: &str,
        source_name: &str,
        mangled: &str,
        parameters: &[Type],
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
            });
        }
    }

    pub(crate) fn resolve_free_cxx_call(
        &self,
        source_name: &str,
        arguments: &[Expression],
    ) -> Compilation<Option<String>> {
        let key = self.free_cxx_source_name(source_name);
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
        let Some(argument_types) = arguments
            .iter()
            .map(|argument| self.cxx_expression_type(argument))
            .collect::<Option<Vec<_>>>()
        else {
            return Err(Diagnostic::error(format!(
                "C++ function call '{key}' is ambiguous (roadmap)"
            )));
        };
        let exact: Vec<_> = candidates
            .iter()
            .filter(|method| method.parameters == argument_types)
            .collect();
        match exact.as_slice() {
            [method] => Ok(Some(method.mangled.clone())),
            _ => Err(Diagnostic::error(format!(
                "C++ function call '{key}' is ambiguous (roadmap)"
            ))),
        }
    }

    fn cxx_expression_type(&self, expression: &Expression) -> Option<Type> {
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
            Expression::Dereference { pointer } | Expression::Index { base: pointer, .. } => match self.cxx_expression_type(pointer)? {
                Type::Pointer(pointee) => Some(pointee.element()),
                Type::StructPointer { element_size } => Some(Type::Struct {
                    size: element_size,
                    align: 1,
                }),
                _ => None,
            },
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

    /// Recover declaration semantics from a C++ aggregate independently of
    /// layout parsing. Methods defined in a class body are implicitly inline;
    /// declarations carrying `inline` remain inline when a later out-of-class
    /// definition omits the keyword. Static method declarations supply callable
    /// signatures and their source-order prototype symbols.
    ///
    /// This deliberately never infers layout. Calls to skipped inline names still
    /// defer, while a static call is admitted only when one recovered overload
    /// matches its arity.
    pub(crate) fn capture_cxx_class_declarations(
        &mut self,
    ) -> Vec<(String, Type, Vec<Type>)> {
        if !self.cplusplus {
            return Vec::new();
        }
        let start = self.position;
        let is_aggregate = matches!(self.tokens.get(start), Some(Token::KeywordStruct))
            || matches!(self.tokens.get(start), Some(Token::Identifier(word)) if word == "class");
        if !is_aggregate {
            return Vec::new();
        }
        let Some(Token::Identifier(source_class)) = self.tokens.get(start + 1) else {
            return Vec::new();
        };
        let source_class = source_class.clone();
        let class = self.qualify_cxx_class_name(&source_class);
        // In C++, the class tag is also an ordinary type name. Preserve that
        // fact even when layout recovery later rejects the body, so pointers to
        // the class retain their semantic tag.
        self.struct_typedefs
            .entry(source_class)
            .or_insert_with(|| class.clone());
        let mut index = start + 2;
        while !matches!(
            self.tokens.get(index),
            Some(Token::BraceOpen | Token::Semicolon | Token::EndOfFile) | None
        ) {
            index += 1;
        }
        if self.tokens.get(index) != Some(&Token::BraceOpen) {
            return Vec::new();
        }
        self.cxx_inline_ordinal_facts.class_definitions += 1;

        // Seed the primary dispatch table from the one supported base. A base
        // declared in the current namespace is preferred, with the written
        // name as a fallback for already-qualified/preprocessed declarations.
        // Multiple or virtual inheritance stays an honest defer: secondary
        // vptrs require this-adjusting thunks and cannot share this model.
        let header = &self.tokens[start + 2..index];
        let mut dispatch = RecoveredCxxDispatchTable::default();
        if let Some(colon) = header.iter().position(|token| *token == Token::Colon) {
            let inheritance = &header[colon + 1..];
            let unsupported = inheritance.iter().any(|token| {
                token == &Token::Comma
                    || matches!(token, Token::Identifier(word) if word == "virtual")
            });
            let base = inheritance.iter().find_map(|token| match token {
                Token::Identifier(word)
                    if !matches!(word.as_str(), "public" | "private" | "protected") =>
                {
                    Some(word.as_str())
                }
                _ => None,
            });
            if unsupported {
                self.incomplete_cxx_dispatch.insert(class.clone());
            } else if let Some(base) = base {
                let qualified_base = self.qualify_cxx_class_name(base);
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
        self.cxx_dispatch_tables.insert(class.clone(), dispatch);

        index += 1;
        let body_start = index;
        let mut prototypes = Vec::new();
        let mut brace_depth = 1i32;
        let mut paren_depth = 0i32;
        let mut explicitly_inline = false;
        let mut member_name: Option<String> = None;
        let mut member_declaration_start = body_start;
        let mut inline_body_start = None;
        while let Some(token) = self.tokens.get(index) {
            let begins_member = brace_depth == 1
                && paren_depth == 0
                && (index == body_start
                    || matches!(self.tokens.get(index.wrapping_sub(1)), Some(Token::Semicolon | Token::BraceClose))
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
                            Token::Identifier(name) => Some(name.clone()),
                            _ => None,
                        });
                }
            }
            if begins_member {
                member_declaration_start = index;
            }
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
                            let declaration = &self.tokens[member_declaration_start..index];
                            if declaration.iter().any(
                                |token| matches!(token, Token::Identifier(word) if word == "virtual"),
                            ) && declaration.iter().any(|token| token == &Token::Tilde)
                            {
                                self.cxx_inline_ordinal_facts.virtual_destructors += 1;
                            }
                            inline_body_start = Some(index + 1);
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
                        if let Some(body_start) = inline_body_start.take() {
                            self.cxx_inline_ordinal_facts.direct_calls += self.tokens
                                [body_start..index]
                                .windows(2)
                                .filter(|tokens| {
                                    matches!(tokens[0], Token::Identifier(_))
                                        && tokens[1] == Token::ParenOpen
                                })
                                .count();
                        }
                        explicitly_inline = false;
                        member_name = None;
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
                }
                Token::EndOfFile => return prototypes,
                _ => {}
            }
            if begins_member {
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
            Vec<Type>,
            Vec<CxxParameterType>,
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
            self.last_struct_tag.take();
            self.last_enum_tag.take();
            self.last_type_was_wchar = false;
            self.last_array_typedef.take();
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
                    let is_reference = self.eat_keyword(Token::Ampersand);
                    let cxx_storage_type = parameter_type;
                    if is_reference {
                        parameter_type = Type::StructPointer { element_size: 0 };
                    }
                    if matches!(self.peek(), Token::Identifier(_)) {
                        self.advance();
                    }
                    cxx_parameters.push(CxxParameterType::parsed(
                        cxx_storage_type,
                        qualified_name,
                        is_wchar,
                        is_reference,
                        source_is_aggregate_value,
                        pointee_const,
                        pointer_const,
                    ).with_pointer_shape(pointer_depth, pointer_base));
                    parameters.push(parameter_type);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;
            while matches!(self.peek(), Token::Identifier(word) if matches!(word.as_str(), "const" | "override" | "final"))
            {
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
                parameters,
                cxx_parameters,
                variadic,
                is_static,
                is_virtual,
                is_inline || has_body,
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
            parameters,
            cxx_parameters,
            variadic,
            is_static,
            is_virtual,
            is_inline,
        )) = recovered
        {
            let inherited_virtual = self
                .cxx_dispatch_tables
                .get(class)
                .and_then(|table| table.methods.get(&member))
                .and_then(|methods| {
                    methods
                        .iter()
                        .find(|method| method.parameters == parameters && method.variadic == variadic)
                })
                .cloned();
            let is_virtual = is_virtual || inherited_virtual.is_some();
            if is_virtual {
                let table = self.cxx_dispatch_tables.get_mut(class)?;
                if inherited_virtual.is_none() {
                    let slot_offset = table.next_slot_offset;
                    table.next_slot_offset = table.next_slot_offset.checked_add(4)?;
                    table
                        .methods
                        .entry(member)
                        .or_default()
                        .push(RecoveredCxxVirtualMethod {
                            return_type,
                            parameters: parameters.clone(),
                            fixed_parameter_count: parameters.len(),
                            variadic,
                            vptr_offset: 0,
                            slot_offset,
                        });
                }
                // A virtual call never references the out-of-line member symbol
                // directly. Recording the slot is the complete result.
                return Some(None);
            }
            let scopes: Vec<&str> = class.split("::").collect();
            let mangled = mangle_qualified_member_function_variadic_typed(
                &scopes,
                &member,
                &cxx_parameters,
                variadic,
            )
            .ok()?;
            let method = RecoveredCxxMethod {
                mangled: mangled.clone(),
                fixed_parameter_count: parameters.len(),
                variadic,
                parameters: parameters.clone(),
            };
            let prototype_parameters = if is_static {
                self.cxx_static_methods
                    .entry((class.to_string(), member))
                    .or_default()
                    .push(method);
                parameters
            } else if !is_inline {
                self.cxx_instance_methods
                    .entry((class.to_string(), member))
                    .or_default()
                    .push(method);
                let mut prototype_parameters = vec![Type::StructPointer {
                    element_size: self.structs.get(class).map_or(0, |layout| layout.size),
                }];
                prototype_parameters.extend(parameters);
                prototype_parameters
            } else {
                return Some(None);
            };
            if variadic {
                self.variadic_definitions.insert(mangled.clone());
            }
            return Some(Some((mangled, return_type, prototype_parameters)));
        }
        None
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

    pub(crate) fn resolve_instance_member_call(
        &self,
        class: &str,
        member: &str,
        argument_count: usize,
    ) -> Compilation<Option<String>> {
        let source_class = class;
        let class = self.qualify_cxx_class_name(source_class);
        let candidates: Vec<&RecoveredCxxMethod> = self
            .cxx_instance_methods
            .get(&(class.clone(), member.to_string()))
            .or_else(|| {
                self.cxx_instance_methods
                    .get(&(source_class.to_string(), member.to_string()))
            })
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
                "C++ member call '{class}::{member}' is ambiguous (roadmap)"
            ))),
        }
    }

    /// Resolve a virtual member by declaration signature and return the ABI
    /// dispatch location. As with direct members, arity is accepted only when it
    /// identifies exactly one overload. Incomplete tables never produce a slot.
    pub(crate) fn resolve_virtual_member_call(
        &self,
        class: &str,
        member: &str,
        argument_count: usize,
    ) -> Compilation<Option<VirtualDispatch>> {
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
        match candidates.as_slice() {
            [] => Ok(None),
            [method] => Ok(Some(VirtualDispatch {
                vptr_offset: method.vptr_offset,
                slot_offset: method.slot_offset,
                return_type: method.return_type,
                variadic: method.variadic,
            })),
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
        mangle_qualified_member_function_cv_typed(
            &scopes,
            function,
            explicit_parameters,
            true,
        )
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

    /// Resolve a bare static-data-member name inside one of its class methods.
    /// The out-of-class definition is already registered under its ABI name;
    /// this lookup keeps ordinary local/instance-member shadowing in the
    /// expression parser while centralizing C++ qualification here.
    pub(crate) fn resolve_implicit_static_data_member(
        &self,
        member: &str,
    ) -> Compilation<Option<String>> {
        let Some(class) = self.current_member_scope.as_deref() else {
            return Ok(None);
        };
        let mangled = self.mangle_data_member_in_current_namespace(class, member)?;
        Ok(self.global_sizes.contains_key(&mangled).then_some(mangled))
    }

    /// Resolve an unqualified call inside a member body. Arity is enough for the
    /// currently modeled overload set; ambiguous same-arity overloads defer.
    pub(crate) fn resolve_implicit_member_call(
        &self,
        function: &str,
        argument_count: usize,
    ) -> Compilation<Option<(String, bool)>> {
        let Some(class_name) = self.current_member_scope.as_deref() else {
            return Ok(None);
        };
        let Some(methods) = self
            .cxx_classes
            .get(class_name)
            .and_then(|class| class.methods.get(function))
        else {
            return Ok(None);
        };
        let candidates: Vec<&MemberMethod> = methods
            .iter()
            .filter(|method| method.parameters.len() == argument_count)
            .collect();
        if candidates.len() != 1 {
            return Err(Diagnostic::error(format!(
                "member overload resolution for '{class_name}::{function}' is ambiguous or unavailable (roadmap)"
            )));
        }
        let method = candidates[0];
        Ok(Some((
            self.mangle_typed_member_in_current_namespace(
                class_name,
                function,
                &method.cxx_parameters,
            )?,
            method.is_inline,
        )))
    }

    /// Parse one class definition and recover its object layout.
    /// Method declarations do not occupy storage and are skipped after recording
    /// constructor signatures. A single non-virtual base is laid out first.
    /// CodeWarrior inserts a class's own vptr at the declaration position of
    /// its first virtual member, so fields written before `virtual` remain at
    /// the object prefix rather than being shifted.
    pub(crate) fn parse_class_definition(
        &mut self,
    ) -> Compilation<(String, StructLayout, ClassLayout)> {
        if !self.eat_word("class") {
            return Err(Diagnostic::error("expected a C++ class definition"));
        }
        let name = self.parse_identifier()?;
        let mut class = ClassLayout::default();
        let mut layout = StructLayout::default();
        let mut offset = 0u32;
        let mut max_align = 1u32;

        if self.eat_keyword(Token::Colon) {
            loop {
                while matches!(self.peek(), Token::Identifier(word)
                    if matches!(word.as_str(), "public" | "private" | "protected"))
                {
                    self.advance();
                }
                if self.eat_word("virtual") {
                    return Err(Diagnostic::error(
                        "virtual base-class layout is not supported yet (roadmap)",
                    ));
                }
                let base_name = self.parse_identifier()?;
                let (base_is_polymorphic, base_vptr_offset, base_virtual_slots) = self
                    .cxx_classes
                    .get(&base_name)
                    .map_or((false, None, 0), |base| {
                        (base.is_polymorphic, base.vptr_offset, base.virtual_slots)
                    });
                let base = self.structs.get(&base_name).ok_or_else(|| {
                    Diagnostic::error(format!(
                        "base class '{base_name}' must be defined before '{name}'"
                    ))
                })?;
                let base_align = (base.align as u32).max(1);
                offset = offset.div_ceil(base_align) * base_align;
                let base_offset = offset;
                for (field_name, field) in base.fields_in_declaration_order() {
                    layout.insert_field(
                        field_name.clone(),
                        StructField {
                            member_type: field.member_type,
                            offset: base_offset + field.offset,
                            struct_tag: field.struct_tag.clone(),
                            array_element: field.array_element,
                            array_bytes: field.array_bytes,
                            bit_field: field.bit_field,
                        },
                    );
                }
                layout
                    .function_pointer_fields
                    .extend(base.function_pointer_fields.iter().cloned());
                class.bases.push(BaseClass { name: base_name });
                class.is_polymorphic |= base_is_polymorphic;
                if class.vptr_offset.is_none() {
                    class.vptr_offset = base_vptr_offset.map(|offset| base_offset + offset);
                }
                class.virtual_slots = class.virtual_slots.max(base_virtual_slots);
                offset += base.size;
                max_align = max_align.max(base_align);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
                return Err(Diagnostic::error(
                    "multiple inheritance is not supported yet (roadmap)",
                ));
            }
        }

        self.expect(Token::BraceOpen)?;
        while *self.peek() != Token::BraceClose {
            if matches!(self.peek(), Token::Identifier(word)
                if matches!(word.as_str(), "public" | "private" | "protected"))
                && *self.peek_at(1) == Token::Colon
            {
                self.advance();
                self.advance();
                continue;
            }
            let is_explicit = self.eat_word("explicit");
            let is_virtual = self.eat_word("virtual");
            if is_virtual {
                if !class.is_polymorphic {
                    // Unlike modern Itanium-style layouts, this ABI inserts the
                    // vptr where the first virtual declaration appears. A class
                    // beginning with data therefore keeps that data at offset 0
                    // and receives an aligned vptr after it. Polymorphic bases
                    // already supply the primary vptr and skip this path.
                    offset = offset.div_ceil(4) * 4;
                    class.vptr_offset = Some(offset);
                    offset += 4;
                    max_align = max_align.max(4);
                    class.is_polymorphic = true;
                }
            }
            if self.eat_word("static") {
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
                if is_virtual {
                    class.virtual_slots += 1;
                    class.has_virtual_destructor = true;
                }
                self.skip_class_member()?;
                continue;
            }

            let field_type = self.parse_type()?;
            if self.last_array_typedef.take().is_some() {
                return Err(Diagnostic::error(
                    "an array-typedef class member is not supported yet (roadmap)",
                ));
            }
            let struct_tag = self.last_struct_tag.take();
            let attribute_align = self.skip_attributes()?.unwrap_or(1);
            let field_name = self.parse_identifier()?;
            if *self.peek() == Token::ParenOpen {
                let signature = self.parse_class_parameter_types()?;
                let is_inline = self.skip_class_method_tail()?;
                if is_virtual {
                    class.virtual_slots += 1;
                }
                class
                    .methods
                    .entry(field_name)
                    .or_default()
                    .push(MemberMethod {
                        parameters: signature.parameters,
                        cxx_parameters: signature.cxx_parameters,
                        is_inline,
                    });
                continue;
            }
            if matches!(self.peek(), Token::Colon) {
                return Err(Diagnostic::error(
                    "a C++ bit-field member is not supported yet (roadmap)",
                ));
            }
            if matches!(self.peek(), Token::BracketOpen) {
                return Err(Diagnostic::error(
                    "a C++ array member is not supported yet (roadmap)",
                ));
            }
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
            layout.insert_field(
                field_name.clone(),
                StructField {
                    member_type: field_type,
                    offset,
                    struct_tag,
                    array_element: None,
                    array_bytes: None,
                    bit_field: None,
                },
            );
            class.fields.push(field_name);
            offset += type_size(field_type);
            max_align = max_align.max(align);
        }
        self.expect(Token::BraceClose)?;
        self.expect(Token::Semicolon)?;
        // C++ gives an otherwise empty class size one. Empty-base optimization is
        // deliberately outside this subset.
        layout.size = offset.max(1).div_ceil(max_align) * max_align;
        layout.align = max_align as u8;
        Ok((name, layout, class))
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
                    struct_tag.map(|tag| {
                        self.struct_typedefs.get(&tag).cloned().unwrap_or(tag)
                    })
                });
                let pointee_const = self.last_type_was_const;
                let pointer_const = self.last_pointer_const;
                let pointer_depth = self.last_cxx_pointer_depth;
                let pointer_base = self.last_cxx_pointer_base;
                let is_reference = self.eat_keyword(Token::Ampersand);
                if is_reference {
                    parameter_type = Type::StructPointer { element_size: 0 };
                }
                if matches!(self.peek(), Token::Identifier(_)) {
                    self.advance();
                }
                parameters.push(parameter_type);
                cxx_parameters.push(CxxParameterType::parsed(
                    source_type,
                    qualified_name,
                    is_wchar,
                    is_reference,
                    source_is_aggregate_value,
                    pointee_const,
                    pointer_const,
                ).with_pointer_shape(pointer_depth, pointer_base));
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
    ) -> Compilation<Vec<Statement>> {
        if !self.eat_keyword(Token::Colon) {
            return Ok(Vec::new());
        }
        let class = self.cxx_classes.get(scope).ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for constructor '{scope}' was not recovered"
            ))
        })?;
        let base_names: Vec<String> = class.bases.iter().map(|base| base.name.clone()).collect();
        let field_names = class.fields.clone();
        let mut initializers = std::collections::HashMap::new();
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

        let mut statements = Vec::new();
        for base_name in base_names {
            let Some(arguments) = initializers.remove(&base_name) else {
                return Err(Diagnostic::error(format!(
                    "implicit base construction for '{base_name}' is not supported yet (roadmap)"
                )));
            };
            let signatures = &self
                .cxx_classes
                .get(&base_name)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "base class layout for '{base_name}' was not recovered"
                    ))
                })?
                .constructors;
            let candidates: Vec<&ClassParameterTypes> = signatures
                .iter()
                .filter(|signature| signature.parameters.len() == arguments.len())
                .collect();
            if candidates.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "constructor overload resolution for '{base_name}' is ambiguous or unavailable (roadmap)"
                )));
            }
            let name = self.mangle_typed_member_in_current_namespace(
                base_name.as_str(),
                "__ct",
                &candidates[0].cxx_parameters,
            )?;
            let mut call_arguments = vec![Expression::Variable("this".to_string())];
            call_arguments.extend(arguments);
            statements.push(Statement::Expression(Expression::Call {
                name,
                arguments: call_arguments,
            }));
        }
        let layout = self.structs.get(scope).ok_or_else(|| {
            Diagnostic::error(format!(
                "class layout for constructor '{scope}' was not recovered"
            ))
        })?;
        for field_name in field_names {
            let Some(mut arguments) = initializers.remove(&field_name) else {
                continue;
            };
            let field = layout.fields.get(&field_name).ok_or_else(|| {
                Diagnostic::error(format!(
                    "member '{field_name}' is absent from class '{scope}'"
                ))
            })?;
            if field.struct_tag.is_some() || arguments.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "non-scalar constructor initialization for '{field_name}' is not supported yet (roadmap)"
                )));
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
pub(crate) fn mangle_qualified_data_member(
    scopes: &[&str],
    member: &str,
) -> Compilation<String> {
    if member.is_empty() {
        return Err(Diagnostic::error("an empty C++ data-member name is invalid"));
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

fn encode_qualified_scope(scopes: &[&str]) -> Compilation<String> {
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
    if let Some(name) = parameter.qualified_name.as_deref() {
        code.push_str(&encode_qualified_type_name(name)?);
        return Ok(code);
    }
    let encoded_source = parameter.pointer_base.unwrap_or(parameter.source_type);
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
        Type::Void => {
            if is_pointer {
                "v".to_string()
            } else {
                return Err(Diagnostic::error(
                    "a named void C++ parameter is not supported",
                ));
            }
        }
        Type::StructPointer { .. } | Type::Struct { .. } => {
            return Err(Diagnostic::error(
                "a struct-valued C++ member parameter needs qualified type mangling (roadmap)",
            ))
        }
    };
    code.push_str(&base);
    Ok(code)
}

fn encode_qualified_type_name(name: &str) -> Compilation<String> {
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
        let pointer = named(
            Type::StructPointer { element_size: 4 },
            false,
            true,
            false,
        );
        let reference = named(Type::Struct { size: 4, align: 4 }, true, true, false);
        let const_pointer_reference = named(
            Type::StructPointer { element_size: 4 },
            true,
            true,
            true,
        );
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
            mangle_qualified_member_function_typed(
                &["A"],
                "q",
                &[const_pointer_reference],
            )
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
}
