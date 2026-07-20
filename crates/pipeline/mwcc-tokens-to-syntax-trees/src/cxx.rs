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
    pub(crate) constructors: Vec<Vec<Type>>,
    pub(crate) methods: std::collections::HashMap<String, Vec<MemberMethod>>,
    /// The class has a virtual dispatch table pointer. This is layout state,
    /// not merely syntax: a polymorphic primary base already supplies the slot.
    pub(crate) is_polymorphic: bool,
}

pub(crate) struct MemberMethod {
    pub(crate) parameters: Vec<Type>,
    pub(crate) is_inline: bool,
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
    is_reference: bool,
    pointee_const: bool,
    pointer_const: bool,
}

impl CxxParameterType {
    pub(crate) fn parsed(
        source_type: Type,
        qualified_name: Option<String>,
        is_reference: bool,
        pointee_const: bool,
        pointer_const: bool,
    ) -> Self {
        Self {
            source_type,
            qualified_name,
            is_reference,
            pointee_const,
            pointer_const,
        }
    }

    pub(crate) fn plain(source_type: Type) -> Self {
        Self::parsed(source_type, None, false, false, false)
    }
}

pub(crate) struct BaseClass {
    pub(crate) name: String,
}

/// Remove C++ linkage-specification syntax while retaining every enclosed token
/// in source order. `extern "C" { declarations }` becomes `declarations`, and
/// `extern "C" declaration` keeps the `extern` storage class but drops the
/// language string. The latter distinction matters for data declarations.
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
                tokens.remove(close);
                tokens.drain(index..index + 3);
                continue;
            }
        } else {
            // Keep `extern` so an object declaration remains a declaration rather
            // than becoming a tentative definition.
            tokens.remove(index + 1);
        }
        index += 1;
    }
    tokens
}

/// C++ constructors have no written return type. Insert the parser-internal
/// `void` only for a top-level `Class::Class(` declarator, leaving class-body
/// prototypes and expression-level qualified names untouched.
pub(crate) fn normalize_constructor_declarators(
    mut tokens: Vec<LocatedToken>,
) -> Vec<LocatedToken> {
    let mut index = 0usize;
    let mut brace_depth = 0usize;
    while index + 4 < tokens.len() {
        match tokens[index].token {
            Token::BraceOpen => brace_depth += 1,
            Token::BraceClose => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        let constructor = brace_depth == 0
            && matches!((&tokens[index].token, &tokens[index + 3].token),
                (Token::Identifier(scope), Token::Identifier(name)) if scope == name)
            && tokens[index + 1].token == Token::Colon
            && tokens[index + 2].token == Token::Colon
            && tokens[index + 4].token == Token::ParenOpen;
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
        } else {
            index += 1;
        }
    }
    tokens
}

impl Parser {
    pub(crate) fn qualify_cxx_class_name(&self, class: &str) -> String {
        if self.namespace_stack.is_empty() {
            class.to_string()
        } else {
            format!("{}::{class}", self.namespace_stack.join("::"))
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
            match token {
                Token::ParenOpen if brace_depth == 1 => paren_depth += 1,
                Token::ParenClose if brace_depth == 1 && paren_depth > 0 => paren_depth -= 1,
                Token::BraceOpen => {
                    // A brace following a class-scope parameter list is the
                    // method body, hence implicitly inline.
                    if brace_depth == 1 && paren_depth == 0 {
                        if let Some(member) = member_name.take() {
                            self.inline_cxx_members.insert((class.clone(), member));
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
        let saved_array_typedef = self.last_array_typedef;
        let saved_type_const = self.last_type_was_const;
        let saved_pointer_const = self.last_pointer_const;
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
                    let qualified_name = struct_tag.map(|tag| {
                        self.struct_typedefs.get(&tag).cloned().unwrap_or(tag)
                    });
                    self.last_array_typedef.take();
                    let pointee_const = self.last_type_was_const;
                    let pointer_const = self.last_pointer_const;
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
                        is_reference,
                        pointee_const,
                        pointer_const,
                    ));
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
        self.last_array_typedef = saved_array_typedef;
        self.last_type_was_const = saved_type_const;
        self.last_pointer_const = saved_pointer_const;
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
        let mut scopes: Vec<&str> = self.namespace_stack.iter().map(String::as_str).collect();
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
        let mut scopes: Vec<&str> = self.namespace_stack.iter().map(String::as_str).collect();
        scopes.push(class);
        mangle_qualified_member_function_typed(&scopes, function, explicit_parameters)
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
            self.mangle_member_in_current_namespace(class_name, function, &method.parameters)?,
            method.is_inline,
        )))
    }

    /// Parse one class definition and recover its object layout.
    /// Method declarations do not occupy storage and are skipped after recording
    /// constructor signatures. A single non-virtual base is laid out first;
    /// simple polymorphic classes reserve their implicit vptr at offset zero.
    pub(crate) fn parse_class_definition(
        &mut self,
    ) -> Compilation<(String, StructLayout, ClassLayout)> {
        if !self.eat_word("class") {
            return Err(Diagnostic::error("expected a C++ class definition"));
        }
        let name = self.parse_identifier()?;
        let mut class = ClassLayout::default();
        let mut layout = StructLayout::default();
        let mut offset = 0u16;
        let mut max_align = 1u16;

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
                let base_is_polymorphic = self
                    .cxx_classes
                    .get(&base_name)
                    .is_some_and(|base| base.is_polymorphic);
                let base = self.structs.get(&base_name).ok_or_else(|| {
                    Diagnostic::error(format!(
                        "base class '{base_name}' must be defined before '{name}'"
                    ))
                })?;
                let base_align = (base.align as u16).max(1);
                offset = offset.div_ceil(base_align) * base_align;
                let base_offset = offset;
                for (field_name, field) in &base.fields {
                    layout.fields.insert(
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
                class.bases.push(BaseClass { name: base_name });
                class.is_polymorphic |= base_is_polymorphic;
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
            if self.eat_word("virtual") {
                if !class.is_polymorphic {
                    // A new vptr is the primary object component. We can place it
                    // exactly while no base data has already occupied the prefix.
                    // Polymorphic derivation reuses the primary base's vptr above.
                    if offset != 0 {
                        return Err(Diagnostic::error(
                            "a polymorphic class with a non-polymorphic base is not supported yet (roadmap)",
                        ));
                    }
                    offset = 4;
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
            if *self.peek() == Token::Tilde {
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
                let parameters = self.parse_class_parameter_types()?;
                let is_inline = self.skip_class_method_tail()?;
                class
                    .methods
                    .entry(field_name)
                    .or_default()
                    .push(MemberMethod {
                        parameters,
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
            let align = type_alignment(field_type).max(attribute_align).max(1);
            offset = offset.div_ceil(align) * align;
            layout.fields.insert(
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

    fn parse_class_parameter_types(&mut self) -> Compilation<Vec<Type>> {
        self.expect(Token::ParenOpen)?;
        let mut parameters = Vec::new();
        if *self.peek() == Token::KeywordVoid && *self.peek_at(1) == Token::ParenClose {
            self.advance();
        } else {
            while *self.peek() != Token::ParenClose {
                let parameter_type = self.parse_type()?;
                self.last_array_typedef.take();
                self.last_struct_tag.take();
                if matches!(self.peek(), Token::Identifier(_)) {
                    self.advance();
                }
                parameters.push(parameter_type);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::ParenClose)?;
        Ok(parameters)
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

    fn skip_class_member(&mut self) -> Compilation<()> {
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
            let candidates: Vec<&Vec<Type>> = signatures
                .iter()
                .filter(|signature| signature.len() == arguments.len())
                .collect();
            if candidates.len() != 1 {
                return Err(Diagnostic::error(format!(
                    "constructor overload resolution for '{base_name}' is ambiguous or unavailable (roadmap)"
                )));
            }
            let name =
                self.mangle_member_in_current_namespace(base_name.as_str(), "__ct", candidates[0])?;
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

fn mangle_qualified_member_function_typed(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[CxxParameterType],
) -> Compilation<String> {
    mangle_qualified_member_function_variadic_typed(
        scopes,
        function,
        explicit_parameters,
        false,
    )
}

fn mangle_qualified_member_function_variadic_typed(
    scopes: &[&str],
    function: &str,
    explicit_parameters: &[CxxParameterType],
    variadic: bool,
) -> Compilation<String> {
    if scopes.is_empty() || scopes.iter().any(|scope| scope.is_empty()) || function.is_empty() {
        return Err(Diagnostic::error("an empty C++ member name is invalid"));
    }
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
    let qualified_scope = if scopes.len() == 1 {
        format!("{}{}", scopes[0].len(), scopes[0])
    } else {
        let components = scopes
            .iter()
            .map(|scope| format!("{}{scope}", scope.len()))
            .collect::<String>();
        format!("Q{}{components}", scopes.len())
    };
    Ok(format!("{function}__{qualified_scope}F{arguments}"))
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
    let is_pointer = matches!(
        parameter.source_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    );
    if is_pointer {
        code.push('P');
    }
    // Leading const binds the referred object or a pointer's pointee. Top-level
    // const on a by-value parameter is absent from the function type.
    if parameter.pointee_const && (parameter.is_reference || is_pointer) {
        code.push('C');
    }
    if let Some(name) = parameter.qualified_name.as_deref() {
        code.push_str(&encode_qualified_type_name(name)?);
        return Ok(code);
    }
    let base = match parameter.source_type {
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
        Type::Pointer(pointee) => encode_pointee(pointee)?.to_string(),
        Type::Void => {
            return Err(Diagnostic::error(
                "a named void C++ parameter is not supported",
            ))
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
    if scopes.is_empty() || scopes.iter().any(|scope| scope.is_empty()) {
        return Err(Diagnostic::error("an empty qualified C++ type name is invalid"));
    }
    if scopes.len() == 1 {
        Ok(format!("{}{name}", name.len()))
    } else {
        let components = scopes
            .iter()
            .map(|scope| format!("{}{scope}", scope.len()))
            .collect::<String>();
        Ok(format!("Q{}{components}", scopes.len()))
    }
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
    fn strips_block_linkage_without_losing_declarations() {
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
                Token::KeywordInt,
                Token::Identifier("value".to_string()),
                Token::Semicolon,
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
    fn mangles_named_value_pointer_reference_and_cv_layers() {
        let named = |storage_type, is_reference, pointee_const, pointer_const| {
            CxxParameterType::parsed(
                storage_type,
                Some("JUtility::TColor".to_string()),
                is_reference,
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
}
