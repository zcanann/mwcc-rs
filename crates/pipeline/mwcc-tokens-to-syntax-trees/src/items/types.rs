//! Type parsing and struct/union body layout: `parse_type` (the full declarator
//! grammar — qualifiers, pointers, typedef names, enum/struct/union references) and
//! the struct/union field-layout builders. Part of the `items` module.

use super::*;
use crate::parser::{Parser, StructField, StructLayout};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{
    Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter,
    Pointee, PointerElement, SourceFundamentalType, Statement, SwitchArm, TranslationUnit, Type,
};
use mwcc_tokens::Token;

fn align_layout_offset(offset: u32, alignment: u32) -> Compilation<u32> {
    offset
        .div_ceil(alignment)
        .checked_mul(alignment)
        .ok_or_else(|| Diagnostic::error("aggregate layout exceeds the 32-bit address space"))
}

fn advance_layout_offset(offset: u32, size: u32) -> Compilation<u32> {
    offset
        .checked_add(size)
        .ok_or_else(|| Diagnostic::error("aggregate layout exceeds the 32-bit address space"))
}

fn source_fundamental(declared_type: Type) -> Option<SourceFundamentalType> {
    Some(match declared_type {
        Type::Int => SourceFundamentalType::SignedInteger,
        Type::UnsignedInt => SourceFundamentalType::UnsignedInteger,
        Type::Char => SourceFundamentalType::SignedChar,
        Type::UnsignedChar => SourceFundamentalType::UnsignedChar,
        Type::Short => SourceFundamentalType::SignedShort,
        Type::UnsignedShort => SourceFundamentalType::UnsignedShort,
        Type::Float => SourceFundamentalType::Float,
        Type::Double => SourceFundamentalType::Double,
        Type::Void => SourceFundamentalType::Void,
        Type::LongLong => SourceFundamentalType::SignedLongLong,
        Type::UnsignedLongLong => SourceFundamentalType::UnsignedLongLong,
        Type::Pointer(_) | Type::StructPointer { .. } | Type::Struct { .. } => return None,
    })
}

fn minimum_enum_storage(minimum: i64, maximum: i64) -> Type {
    if minimum >= 0 {
        if maximum <= u8::MAX as i64 {
            Type::UnsignedChar
        } else if maximum <= u16::MAX as i64 {
            Type::UnsignedShort
        } else {
            Type::UnsignedInt
        }
    } else if minimum >= i8::MIN as i64 && maximum <= i8::MAX as i64 {
        Type::Char
    } else if minimum >= i16::MIN as i64 && maximum <= i16::MAX as i64 {
        Type::Short
    } else {
        Type::Int
    }
}

fn scalar_pointee(declared: Type) -> Option<Pointee> {
    Some(match declared {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        _ => return None,
    })
}

fn merged_attribute_alignment(before: Option<u16>, after: Option<u16>) -> u32 {
    u32::from(before.unwrap_or(1).max(after.unwrap_or(1)))
}

fn place_bit_field(
    bit_unit: &mut Option<(Type, u32, u8)>,
    offset: &mut u32,
    alignment_max: &mut u32,
    field_type: Type,
    width: u8,
    requested_alignment: u32,
) -> Compilation<(u32, u8)> {
    let unit_bits = (type_size(field_type) * 8) as u8;
    if width == 0 || width > unit_bits {
        return Err(Diagnostic::error(
            "an unsupported bit-field width (roadmap)",
        ));
    }
    if let Some((unit_type, unit_offset, bits_used)) = *bit_unit {
        if unit_type == field_type && bits_used + width <= unit_bits {
            *bit_unit = Some((field_type, unit_offset, bits_used + width));
            return Ok((unit_offset, bits_used));
        }
        // A new storage-unit type closes the previous unit at the bytes its
        // bits actually occupied. MWCC does not pad a partially used `u16`
        // unit to two bytes before a following `u8` field.
        *offset = unit_offset + u32::from(bits_used).div_ceil(8);
    }
    let alignment = type_alignment(field_type).max(1).max(requested_alignment);
    let unit_offset = align_layout_offset(*offset, alignment)?;
    *offset = advance_layout_offset(unit_offset, type_size(field_type))?;
    *alignment_max = (*alignment_max).max(alignment);
    *bit_unit = Some((field_type, unit_offset, width));
    Ok((unit_offset, 0))
}

impl Parser {
    /// Consume one or more constant array dimensions and return the total byte
    /// extent plus the first-index stride for a multidimensional declaration.
    /// Keeping the arithmetic here gives C structs, unions, and C++ classes one
    /// overflow-checked interpretation of `member[A][B]`.
    pub(crate) fn parse_array_declarator_extent(
        &mut self,
        element_size: u32,
    ) -> Compilation<Option<(u32, Option<u32>)>> {
        if *self.peek() != Token::BracketOpen {
            return Ok(None);
        }

        let mut dimensions = Vec::new();
        while *self.peek() == Token::BracketOpen {
            self.advance();
            let count = u32::try_from(self.parse_integer_constant()?)
                .map_err(|_| Diagnostic::error("array dimension exceeds 32 bits"))?;
            self.expect(Token::BracketClose)?;
            dimensions.push(count);
        }

        let multiply_dimensions = |dimensions: &[u32]| {
            dimensions.iter().try_fold(1u32, |total, &dimension| {
                total.checked_mul(dimension).ok_or_else(|| {
                    Diagnostic::error("array extent exceeds the 32-bit address space")
                })
            })
        };
        let total_elements = multiply_dimensions(&dimensions)?;
        let total_bytes = total_elements.checked_mul(element_size).ok_or_else(|| {
            Diagnostic::error("array extent exceeds the 32-bit address space")
        })?;
        let first_index_stride = if dimensions.len() > 1 {
            Some(
                multiply_dimensions(&dimensions[1..])?
                    .checked_mul(element_size)
                    .ok_or_else(|| {
                        Diagnostic::error("array stride exceeds the 32-bit address space")
                    })?,
            )
        } else {
            None
        };
        Ok(Some((total_bytes, first_index_stride)))
    }

    pub(crate) fn parse_type(&mut self) -> Compilation<Type> {
        let parsed = self.parse_type_base()?;
        // A POSTFIX qualifier — east const: `unsigned char const *jp` (metroid
        // prime's ansi_fp revision) — reads exactly like the prefix form.
        while matches!(self.peek(), Token::Identifier(word) if word == "const") {
            self.advance();
            self.last_type_was_const = true;
        }
        if matches!(self.peek(), Token::Identifier(word) if word == crate::CXX_POINTEE_CONST_MARKER)
        {
            self.advance();
            self.last_type_was_const = true;
        }
        Ok(parsed)
    }

    fn parse_type_base(&mut self) -> Compilation<Type> {
        self.last_struct_tag = None;
        self.last_enum_tag = None;
        self.last_type_was_wchar = false;
        self.last_source_fundamental = None;
        self.last_type_was_aggregate_reference = false;
        self.last_pointer_const = false;
        self.last_cxx_pointer_depth = 0;
        self.last_cxx_pointer_base = None;
        self.last_cxx_function_type = None;
        // The array-typedef marker is only ever set by the LAST parse_type call, so a
        // consumer that `.take()`s right after its own call can never read a stale one.
        self.last_array_typedef = None;
        // Leading qualifiers: `const`/`register` are transparent to codegen (`const`
        // is noted for the global path, which defers a read-only global); `volatile`
        // changes access semantics (memory accesses can't be elided), so defer it.
        self.skip_type_qualifiers()?;
        // `enum [Tag] [{ … }]`: a body registers its enumerators and, under
        // `-enum min`, selects storage from their complete value range.
        if matches!(self.peek(), Token::Identifier(word) if word == "enum") {
            self.advance();
            let tag = if let Token::Identifier(tag) = self.peek() {
                let tag = tag.clone();
                self.advance();
                Some(tag)
            } else {
                None
            };
            let tagged = tag.is_some();
            // Source identity is needed by both C debug information and C++
            // mangling even though executable storage is scalar.
            self.last_enum_tag = tag.clone();
            let storage = if *self.peek() == Token::BraceOpen {
                // An ANONYMOUS enum definition consumes one anonymous-`@N` number
                // (measured fire 494: `typedef enum {…} E;` shifts the next pool
                // constant by +1; a TAGGED enum adds nothing — pikmin's uart TU
                // carries three such enums between its inlines and its statics).
                // Keyed by token position so a speculative re-parse can't double-count.
                if !tagged && self.counted_enum_positions.insert(self.position) {
                    self.skipped_inline_functions += 1;
                }
                let definition_position = self.position;
                let (minimum, maximum, enumerators) = self.parse_enum_body()?;
                let storage = if self.enum_min {
                    minimum_enum_storage(minimum, maximum)
                } else {
                    Type::Int
                };
                let identity = tag
                    .clone()
                    .unwrap_or_else(|| format!("@enum:{definition_position}"));
                self.enum_types.insert(identity.clone(), storage);
                self.enumeration_definitions.push(
                    mwcc_syntax_trees::EnumerationDefinition {
                        name: identity.clone(),
                        source_name: tag.clone(),
                        byte_size: storage.width().div_ceil(8),
                        enumerators,
                    },
                );
                self.last_enum_tag = Some(identity);
                storage
            } else {
                tag.as_ref()
                    .and_then(|tag| self.enum_types.get(tag).copied())
                    .unwrap_or(Type::Int)
            };
            if *self.peek() == Token::Star {
                self.advance();
                return Ok(Type::Pointer(
                    scalar_pointee(storage).unwrap_or(Pointee::Int),
                ));
            }
            return Ok(storage);
        }
        // `struct Name*` — a pointer to a (already declared) struct. The tag is
        // stashed in `last_struct_tag` for the declarator parser to record.
        // A qualified C++ aggregate name (`JUtility::TColor`) names the same
        // layout as its locally declared tag while retaining the qualified ABI
        // identity in `struct_typedefs`. Recognize an arbitrary namespace/class
        // chain before the ordinary one-token typedef path below.
        if self.cplusplus {
            if let Some(instance_type) = self.parse_template_instance_type() {
                return Ok(instance_type);
            }
            let mut scan = self.position;
            let mut components = Vec::new();
            if let Some(Token::Identifier(first)) = self.tokens.get(scan) {
                components.push(first.clone());
                scan += 1;
                while self.tokens.get(scan) == Some(&Token::Colon)
                    && self.tokens.get(scan + 1) == Some(&Token::Colon)
                {
                    let Some(Token::Identifier(component)) = self.tokens.get(scan + 2) else {
                        break;
                    };
                    components.push(component.clone());
                    scan += 3;
                }
            }
            if components.len() >= 2 {
                let source_qualified = components.join("::");
                let local = components.last().unwrap().clone();
                if let Some(storage) = self
                    .enum_types
                    .get(&source_qualified)
                    .or_else(|| self.enum_types.get(&local))
                    .copied()
                {
                    self.position = scan;
                    self.last_enum_tag = Some(source_qualified);
                    return Ok(storage);
                }
                let qualified = self
                    .resolve_scoped_cxx_class_name(&source_qualified)
                    .unwrap_or(source_qualified);
                let layout_key = if self.structs.contains_key(&qualified) {
                    qualified.clone()
                } else {
                    local.clone()
                };
                let known = self.structs.contains_key(&layout_key)
                    || self
                        .struct_typedefs
                        .values()
                        .any(|mapped| mapped == &qualified);
                // A fully qualified pointer/reference type is unambiguous even
                // when only a forward declaration survived preprocessing. Its
                // pointee may remain opaque (size zero); retaining the name is
                // enough for ABI mangling and later member declaration parsing.
                let opaque_indirection = matches!(
                    self.tokens.get(scan),
                    Some(Token::Star | Token::Ampersand)
                );
                if known || opaque_indirection {
                    self.position = scan;
                    self.struct_typedefs
                        .entry(local)
                        .or_insert_with(|| qualified.clone());
                    if *self.peek() == Token::Star {
                        self.advance();
                        let element_size = self
                            .structs
                            .get(&layout_key)
                            .map_or(0, |layout| layout.size);
                        self.last_struct_tag = Some(layout_key);
                        if *self.peek() == Token::Star {
                            self.advance();
                            return Ok(Type::Pointer(Pointee::Pointer));
                        }
                        return Ok(Type::StructPointer { element_size });
                    }
                    if *self.peek() == Token::Ampersand {
                        let element_size = self
                            .structs
                            .get(&layout_key)
                            .map_or(0, |layout| layout.size);
                        self.last_struct_tag = Some(layout_key);
                        self.last_type_was_aggregate_reference = true;
                        return Ok(Type::StructPointer { element_size });
                    }
                    return match self.struct_value_type(&layout_key) {
                        Some(struct_type) => {
                            self.last_struct_tag = Some(layout_key);
                            Ok(struct_type)
                        }
                        None => Err(Diagnostic::error(format!(
                            "struct '{qualified}' value layout is not declared",
                        ))),
                    };
                }
            }
        }
        // An elaborated C++ class specifier (`class Name value`, `class Name* p`)
        // names the same aggregate type registered by either a prior `class` or
        // `struct` definition. C++ permits a different class-key at a use site;
        // layout and member identity therefore share the ordinary struct path.
        if self.cplusplus && matches!(self.peek(), Token::Identifier(word) if word == "class") {
            self.advance();
            let tag = self.parse_identifier()?;
            if !matches!(self.peek(), Token::Star | Token::Ampersand) {
                return match self.struct_value_type(&tag) {
                    Some(class_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(class_type)
                    }
                    None => Err(Diagnostic::error(format!(
                        "class '{tag}' value layout is not declared",
                    ))),
                };
            }
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
            if *self.peek() == Token::Ampersand {
                self.last_type_was_aggregate_reference = true;
                return Ok(Type::StructPointer { element_size });
            }
            self.advance();
            if *self.peek() == Token::Star {
                self.advance();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
            return Ok(Type::StructPointer { element_size });
        }
        if *self.peek() == Token::KeywordStruct {
            self.advance();
            let tag = self.parse_identifier()?;
            self.consume_trailing_qualifiers();
            if !matches!(self.peek(), Token::Star | Token::Ampersand) {
                // A struct *value*: a known layout becomes a sized struct value
                // (a frame-resident local); an opaque/unknown struct still defers.
                return match self.struct_value_type(&tag) {
                    Some(struct_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(struct_type)
                    }
                    None => Err(Diagnostic::error(format!(
                        "struct '{tag}' value layout is not declared",
                    ))),
                };
            }
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
            if *self.peek() == Token::Ampersand {
                self.last_type_was_aggregate_reference = true;
                return Ok(Type::StructPointer { element_size });
            }
            self.advance();
            if *self.peek() == Token::Star {
                // `S**` — a pointer to a struct pointer: a word-classed
                // pointer whose element is itself a pointer.
                self.advance();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
            return Ok(Type::StructPointer { element_size });
        }
        // `union Name*` / `union Name` — a union is laid out like a struct with every member
        // at offset 0 (overlapping storage), so it reuses the struct machinery once the layout
        // is registered. `union` is lexed as a plain identifier, not a keyword.
        if matches!(self.peek(), Token::Identifier(word) if word == "union") {
            self.advance();
            let tag = self.parse_identifier()?;
            self.consume_trailing_qualifiers();
            if !matches!(self.peek(), Token::Star | Token::Ampersand) {
                return match self.struct_value_type(&tag) {
                    Some(union_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(union_type)
                    }
                    None => Err(Diagnostic::error(
                        "union values are not supported yet — use a union pointer",
                    )),
                };
            }
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
            if *self.peek() == Token::Ampersand {
                self.last_type_was_aggregate_reference = true;
                return Ok(Type::StructPointer { element_size });
            }
            self.advance();
            if *self.peek() == Token::Star {
                // `S**` — a pointer to a struct pointer: a word-classed
                // pointer whose element is itself a pointer.
                self.advance();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
            return Ok(Type::StructPointer { element_size });
        }
        // A named C++ enum may be used without the `enum` prefix. Its configured
        // storage is separate from the source identity retained for mangling.
        if let Token::Identifier(name) = self.peek() {
            if self.cplusplus && self.enum_types.contains_key(name) {
                let name = name.clone();
                let storage = self.enum_types[&name];
                self.advance();
                self.last_enum_tag = Some(name);
                if *self.peek() == Token::Star {
                    self.advance();
                    if matches!(self.peek(), Token::Identifier(word) if word == "const") {
                        self.last_pointer_const = true;
                    }
                    self.consume_trailing_qualifiers();
                    return Ok(Type::Pointer(
                        scalar_pointee(storage).unwrap_or(Pointee::Int),
                    ));
                }
                return Ok(storage);
            }
        }
        // A struct-pointer typedef (`VecPtr`) is itself a pointer to the struct —
        // no trailing `*` — carrying the layout's tag.
        if let Token::Identifier(name) = self.peek() {
            if let Some(tag) = self.struct_pointer_typedefs.get(name).cloned() {
                self.advance();
                self.consume_trailing_qualifiers();
                let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
                self.last_struct_tag = Some(tag);
                if *self.peek() == Token::Star {
                    self.advance();
                    return Ok(Type::Pointer(Pointee::Pointer));
                }
                return Ok(Type::StructPointer { element_size });
            }
        }
        // A struct typedef (`FILE`) behaves like its `struct Tag`: `FILE *` is a
        // struct pointer carrying the layout's tag; a struct value isn't supported.
        if let Token::Identifier(name) = self.peek() {
            // Recovery may conservatively register the final identifier of a
            // malformed header typedef as an opaque aggregate. C++ fundamental
            // types still have lexical precedence over that recovery state: a
            // stray `bool -> bool` entry must not turn `bool C::flag` into an
            // attempted struct-value declaration.
            let cxx_fundamental = self.cplusplus && matches!(name.as_str(), "bool" | "wchar_t");
            if !cxx_fundamental {
                let name = name.clone();
                let tag = self
                    .resolve_scoped_cxx_class_name(&name)
                    .or_else(|| self.struct_typedefs.get(&name).cloned());
                if let Some(tag) = tag {
                    self.advance();
                    // East-const aggregate references (`Node const&`) must be
                    // classified after the qualifier. Requiring a completed
                    // value layout before looking through `const` creates an
                    // impossible cycle for a class's own method signatures.
                    self.consume_trailing_qualifiers();
                    if !matches!(self.peek(), Token::Star | Token::Ampersand) {
                        return match self.struct_value_type(&tag) {
                            Some(struct_type) => {
                                self.last_struct_tag = Some(tag);
                                Ok(struct_type)
                            }
                            None => Err(Diagnostic::error(format!(
                                "struct '{tag}' value layout is not declared",
                            ))),
                        };
                    }
                    let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
                    self.last_struct_tag = Some(tag);
                    if *self.peek() == Token::Ampersand {
                        self.last_type_was_aggregate_reference = true;
                        return Ok(Type::StructPointer { element_size });
                    }
                    self.advance();
                    if *self.peek() == Token::Star {
                        self.advance();
                        return Ok(Type::Pointer(Pointee::Pointer));
                    }
                    return Ok(Type::StructPointer { element_size });
                }
            }
        }
        // A `typedef`-declared alias resolves to its underlying type.
        if let Token::Identifier(name) = self.peek() {
            let name = name.clone();
            if let Some(&aliased) = self.typedefs.get(&name) {
                if let Some(identity) = self.enum_typedefs.get(&name).cloned() {
                    self.last_enum_tag = Some(identity);
                }
                let function_type = self.function_pointer_typedefs.get(&name).cloned();
                self.last_source_fundamental = self
                    .typedef_source_fundamentals
                    .get(&name)
                    .copied()
                    .or_else(|| source_fundamental(aliased));
                self.advance();
                if let Some(function_type) = function_type {
                    self.last_cxx_pointer_depth = 1;
                    self.last_cxx_function_type = Some(function_type);
                }
                if *self.peek() == Token::Star {
                    self.advance();
                    if self.last_cxx_function_type.is_some() {
                        self.last_cxx_pointer_depth = self.last_cxx_pointer_depth.saturating_add(1);
                    }
                    // A star on an already-pointer typedef (`voidfunctionptr*`)
                    // is a pointer-to-pointer: word element, inner untracked.
                    if matches!(aliased, Type::Pointer(_) | Type::StructPointer { .. }) {
                        return Ok(Type::Pointer(Pointee::Pointer));
                    }
                    return Ok(Type::Pointer(pointee_of(aliased)?));
                }
                return Ok(aliased);
            }
        }
        // An array typedef (`Mtx`, `typedef float Mtx[3][4];`) used as a type: the
        // returned scalar type is the DECAYED element pointer (right for a parameter);
        // `last_array_typedef` carries `(element, total, inner)` so the global path can
        // declare the whole array object and the parameter path can record the row
        // stride. A row-pointer typedef (`MtxPtr`, `typedef float (*MtxPtr)[4];`) is
        // already that pointer — it reports `total == 0` (no array object to declare).
        // A trailing `*` (`Mtx*`) is a pointer to the whole array — not modeled; defer.
        if let Token::Identifier(name) = self.peek() {
            if let Some(&(element, total, inner)) = self.array_typedefs.get(name) {
                self.advance();
                if *self.peek() == Token::Star {
                    return Err(Diagnostic::error(
                        "a pointer to an array-typedef value is not supported yet (roadmap)",
                    ));
                }
                self.last_array_typedef = Some((element, total, inner));
                return Ok(Type::Pointer(pointee_of(element)?));
            }
            if let Some(&(element, length)) = self.row_pointer_typedefs.get(name) {
                self.advance();
                if *self.peek() == Token::Star {
                    return Err(Diagnostic::error(
                        "a pointer to a row-pointer-typedef value is not supported yet (roadmap)",
                    ));
                }
                self.last_array_typedef = Some((element, 0, length));
                return Ok(Type::Pointer(pointee_of(element)?));
            }
        }
        let mut source_override = None;
        let base = match self.advance() {
            Token::KeywordInt => Type::Int,
            // Plain `char` follows the build's signedness default: signed on
            // mainline/1.3.2+, UNSIGNED on GC/1.3 (build 53) and `-char unsigned`
            // (no `extsb` on read). `signed char`/`unsigned char` below are explicit.
            Token::KeywordChar => {
                source_override = Some(SourceFundamentalType::PlainChar);
                if self.char_is_signed {
                    Type::Char
                } else {
                    Type::UnsignedChar
                }
            }
            // `short` / `short int`.
            Token::KeywordShort => {
                let _ = self.eat_keyword(Token::KeywordInt);
                Type::Short
            }
            // `unsigned` and its widths, including `unsigned long [long] [int]`.
            Token::KeywordUnsigned => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Type::UnsignedChar
                }
                Token::KeywordShort => {
                    self.advance();
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::UnsignedShort
                }
                Token::KeywordInt => {
                    self.advance();
                    Type::UnsignedInt
                }
                // `unsigned long` / `unsigned long int` — 32-bit unsigned. `unsigned long
                // long` is the 64-bit register-pair type.
                Token::Identifier(word) if word == "long" => {
                    let mut long_count = 0;
                    while self.eat_word("long") {
                        long_count += 1;
                    }
                    let _ = self.eat_keyword(Token::KeywordInt);
                    if long_count >= 2 {
                        Type::UnsignedLongLong
                    } else {
                        source_override = Some(SourceFundamentalType::UnsignedLong);
                        Type::UnsignedInt
                    }
                }
                _ => Type::UnsignedInt,
            },
            Token::KeywordFloat => Type::Float,
            Token::KeywordVoid => Type::Void,
            // PowerPC EABI stores C++ `bool` in one unsigned byte. The current
            // scalar IR has no separate boolean storage type; expression results
            // are already normalized to zero/one, so the unsigned-byte lane keeps
            // field layout and loads/stores exact. C++ parameter mangling will need
            // a distinct IR type before bool-valued parameters can be accepted.
            Token::Identifier(word) if word == "bool" => {
                source_override = Some(SourceFundamentalType::Boolean);
                Type::UnsignedChar
            }
            Token::Identifier(word) if self.cplusplus && word == "wchar_t" => {
                self.last_type_was_wchar = true;
                Type::UnsignedShort
            }
            // `double` (and `long double`, which is also 64-bit here).
            Token::Identifier(word) if word == "double" => Type::Double,
            // `long` / `long int` — 32-bit signed on this target; `long double` is a
            // double. `long long` is the 64-bit register-pair type.
            Token::Identifier(word) if word == "long" => {
                // The outer `match self.advance()` already consumed the first `long`, so
                // seed the count at 1; the loop adds any further `long`s.
                let mut long_count = 1;
                while self.eat_word("long") {
                    long_count += 1;
                }
                if self.eat_word("double") {
                    Type::Double
                } else if long_count >= 2 {
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::LongLong
                } else {
                    source_override = Some(SourceFundamentalType::SignedLong);
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::Int
                }
            }
            // `signed [char|short|int|long]`.
            Token::Identifier(word) if word == "signed" => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Type::Char
                }
                Token::KeywordShort => {
                    self.advance();
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::Short
                }
                // `signed`, `signed int`, and `signed long [int]` are 32-bit on
                // this target; `signed long long [int]` is the 64-bit register-pair
                // type, just like the unprefixed `long long` spelling above.
                _ => {
                    let mut long_count = 0;
                    while self.eat_word("long") {
                        long_count += 1;
                    }
                    let _ = self.eat_keyword(Token::KeywordInt);
                    if long_count >= 2 {
                        Type::LongLong
                    } else {
                        if long_count == 1 {
                            source_override = Some(SourceFundamentalType::SignedLong);
                        }
                        Type::Int
                    }
                }
            },
            other => {
                let token_index = self.position.saturating_sub(1);
                return Err(Diagnostic::error(format!(
                    "expected a type, found {other} at {}",
                    self.diagnostic_position(token_index)
                )));
            }
        };
        self.last_source_fundamental = source_override.or_else(|| source_fundamental(base));
        // East-const/volatile: a qualifier may TRAIL the base type (`float const`, the
        // mirror of the leading `const float` — dolphin/MSL headers use both). Fold
        // `const` into `last_type_was_const` so the global path still sees it as read-only.
        self.consume_trailing_qualifiers();
        // A trailing `*` makes it a pointer to that scalar; a qualifier after the `*`
        // is a const/volatile POINTER (`int *const p`). A `const` here makes the
        // POINTER OBJECT read-only (routes a global to `.sdata2`), unlike a leading
        // `const void*` (pointee-const) — tracked separately in `last_pointer_const`.
        if *self.peek() == Token::Star {
            self.advance();
            self.last_cxx_pointer_depth = 1;
            self.last_cxx_pointer_base = Some(base);
            if matches!(self.peek(), Token::Identifier(word) if word == "const") {
                self.last_pointer_const = true;
            }
            self.consume_trailing_qualifiers();
            // A SECOND `*` is a pointer-to-pointer (`char **end`, `int **pp`):
            // word-sized element. When the inner scalar is a 32-bit integer word
            // (`int **`, `unsigned **`) the double deref `**pp` is a plain `lwz`,
            // so record `WordPointer` to let codegen emit the chained load. The
            // narrow (`char`/`short`), float and long-long inners keep the opaque
            // `Pointer` — their `**pp` would need `lbz`/`lha`/`lfs`, so they defer.
            if *self.peek() == Token::Star {
                self.advance();
                self.last_cxx_pointer_depth = 2;
                self.consume_trailing_qualifiers();
                let inner = match base {
                    Type::Int | Type::UnsignedInt => Pointee::WordPointer,
                    _ => Pointee::Pointer,
                };
                return Ok(Type::Pointer(inner));
            }
            return Ok(Type::Pointer(pointee_of(base)?));
        }
        Ok(base)
    }

    /// Consume a run of `const`/`volatile`/`register` qualifiers that TRAIL a type (east
    /// const), noting `const`/`volatile` in the same flags the leading `skip_type_qualifiers`
    /// sets — but never resetting them, so a leading qualifier already seen is preserved.
    pub(crate) fn consume_trailing_qualifiers(&mut self) {
        loop {
            match self.peek() {
                Token::Identifier(word) if word == "const" => {
                    self.last_type_was_const = true;
                    self.advance();
                }
                Token::Identifier(word) if word == "volatile" => {
                    self.last_type_was_volatile = true;
                    self.advance();
                }
                Token::Identifier(word) if word == "register" => {
                    self.advance();
                }
                _ => return,
            }
        }
    }

    /// Parse `struct Name { type field; ... };`, laying members out with natural
    /// alignment (the `-align powerpc` default) and registering the layout.
    /// Parse a struct body `{ field; … }` (the cursor is at the `{`), returning its
    /// layout. Does not consume any trailing `;` — the caller (a definition or a
    /// typedef) does.
    /// Skip any `__attribute__((...))` specifiers at the cursor — GCC/CodeWarrior
    /// attributes that survive preprocessing (e.g. `ATTRIBUTE_ALIGN(n)` expands to
    /// `__attribute__((aligned(n)))`). Returns the largest `aligned(n)` requested,
    /// if any, so the declarator that follows is laid out with that alignment.
    pub(crate) fn skip_attributes(&mut self) -> Compilation<Option<u16>> {
        let mut align: Option<u16> = None;
        while matches!(self.peek(), Token::Identifier(name) if name == "__attribute__") {
            self.advance();
            self.expect(Token::ParenOpen)?;
            self.expect(Token::ParenOpen)?;
            let mut depth = 2;
            while depth > 0 {
                match self.advance() {
                    Token::ParenOpen => depth += 1,
                    Token::ParenClose => depth -= 1,
                    Token::Identifier(name) if name == "aligned" => {
                        if *self.peek() == Token::ParenOpen {
                            self.advance();
                            depth += 1;
                            let requested = self.parse_integer_constant()? as u16;
                            align = Some(align.unwrap_or(1).max(requested));
                        }
                    }
                    Token::EndOfFile => {
                        return Err(Diagnostic::error("unterminated __attribute__"))
                    }
                    _ => {}
                }
            }
        }
        Ok(align)
    }

    /// Decide whether the declaration at the struct-body cursor is a C++ member
    /// function. The layout pass only needs object fields; callable declarations
    /// are collected by the C++ scanner and must not make a C-compatible `struct`
    /// layout disappear merely because their return type is the injected class
    /// name (`Pixel& operator=(...)` is the common case).
    ///
    /// A function-pointer data member also contains top-level parentheses, so
    /// remember its `(*name)` declarator and leave it to the ordinary field path.
    pub(crate) fn cxx_struct_member_is_method(&self) -> bool {
        if !self.cplusplus {
            return false;
        }
        let mut index = self.position;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut saw_function_pointer_declarator = false;
        let mut saw_initializer = false;
        while let Some(token) = self.tokens.get(index) {
            if paren_depth == 0 && bracket_depth == 0 {
                // Declarator attributes have their own top-level parentheses;
                // they do not make an ordinary data member callable.
                if matches!(token, Token::Identifier(word)
                    if matches!(word.as_str(), "__attribute__" | "__declspec"))
                    && self.tokens.get(index + 1) == Some(&Token::ParenOpen)
                {
                    index += 1;
                    let mut depth = 0usize;
                    while let Some(attribute_token) = self.tokens.get(index) {
                        match attribute_token {
                            Token::ParenOpen => depth += 1,
                            Token::ParenClose => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    index += 1;
                                    break;
                                }
                            }
                            Token::EndOfFile => return false,
                            _ => {}
                        }
                        index += 1;
                    }
                    continue;
                }
                match token {
                    Token::Identifier(word) if word == "operator" => return true,
                    Token::Equals => saw_initializer = true,
                    Token::ParenOpen => {
                        if self.tokens.get(index + 1) == Some(&Token::Star) {
                            saw_function_pointer_declarator = true;
                            paren_depth = 1;
                        } else if !saw_initializer && !saw_function_pointer_declarator {
                            return true;
                        } else {
                            paren_depth = 1;
                        }
                    }
                    Token::BracketOpen => bracket_depth = 1,
                    Token::Semicolon | Token::BraceOpen | Token::BraceClose => return false,
                    Token::EndOfFile => return false,
                    _ => {}
                }
            } else {
                match token {
                    Token::ParenOpen => paren_depth += 1,
                    Token::ParenClose => paren_depth = paren_depth.saturating_sub(1),
                    Token::BracketOpen => bracket_depth += 1,
                    Token::BracketClose => bracket_depth = bracket_depth.saturating_sub(1),
                    Token::EndOfFile => return false,
                    _ => {}
                }
            }
            index += 1;
        }
        false
    }

    pub(crate) fn parse_struct_body(&mut self) -> Compilation<StructLayout> {
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        let mut offset: u32 = 0;
        let mut alignment_max: u32 = 1;
        // The open bit-field allocation unit (its type, byte offset, bits used so
        // far); an ordinary member or a different-typed bit-field closes it.
        let mut bit_unit: Option<(Type, u32, u8)> = None;
        while *self.peek() != Token::BraceClose {
            if self.cxx_struct_member_is_method() {
                self.skip_class_member()?;
                // MWCC headers commonly spell an in-class definition as
                // `return ...; };`; the semicolon is outside the balanced body.
                self.eat_keyword(Token::Semicolon);
                continue;
            }
            // Static C++ members occupy no object storage. Their callable/data
            // declaration semantics are recovered separately by the C++ class
            // scanner; the C-compatible layout pass only needs to advance over
            // the complete declaration without discarding the ordinary fields
            // already laid out around it.
            if self.cplusplus && matches!(self.peek(), Token::Identifier(word) if word == "static")
            {
                self.skip_class_member()?;
                self.eat_keyword(Token::Semicolon);
                continue;
            }
            // An inline struct definition as a member: `struct [Tag] { … } [name];`. An
            // ANONYMOUS one with no member name promotes (flattens) its fields into this
            // struct — C anonymous-struct semantics, and how the game-state structs wrap
            // their bit-fields. A named-tag form registers the tag (and adds a nested
            // struct-value member if a name follows).
            if *self.peek() == Token::KeywordStruct
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + u32::from(bits_used).div_ceil(8);
                }
                self.parse_and_place_inline_struct(
                    &mut layout,
                    &mut offset,
                    &mut alignment_max,
                )?;
                continue;
            }
            // An inline union member `union [Tag] { … } [name];`. An ANONYMOUS one
            // with no member name flattens its members into this struct — every
            // union member shares the union's offset (overlapping storage), and the
            // union occupies its largest member. This is how the game's model
            // structs overlay variant payloads (e.g. HsfObject's data/camera/light).
            if matches!(self.peek(), Token::Identifier(word) if word == "union")
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + u32::from(bits_used).div_ceil(8);
                }
                self.parse_and_place_inline_union(
                    &mut layout,
                    &mut offset,
                    &mut alignment_max,
                )?;
                continue;
            }
            // An array-typedef member (`Mtx unk_F0;` where `Mtx` is `typedef float
            // Mtx[3][4]`) — the typedef gives the element type and base length; a
            // trailing `[N]` multiplies it. Recorded as a flat element array so the
            // member (and those after it) lay out correctly. A 2D element access still
            // defers in codegen — the flat array stops the second `[]` resolving.
            let array_typedef = match self.peek() {
                Token::Identifier(word) => self.array_typedefs.get(word).copied(),
                _ => None,
            };
            if let Some((element, base_len, _inner)) = array_typedef {
                self.advance(); // the array-typedef name
                                // `Mtx *group_mtx;` — a POINTER to the array type is a plain 4-byte
                                // pointer member (subscripts through it defer in codegen); route it
                                // through the ordinary pointer-member layout.
                if *self.peek() == Token::Star {
                    self.advance();
                    let field_name = self.parse_identifier()?;
                    if !matches!(self.peek(), Token::Semicolon) {
                        return Err(Diagnostic::error("an array-typedef-pointer member declarator list is not supported yet (roadmap)"));
                    }
                    self.advance(); // `;`
                    offset = align_layout_offset(offset, 4)?;
                    alignment_max = alignment_max.max(4);
                    layout.insert_field(
                        field_name,
                        StructField {
                            member_type: Type::Pointer(pointee_of(element)?),
                            source_fundamental: source_fundamental(element),
                            offset,
                            struct_tag: None,
                            array_element: None,
                            array_bytes: None,
                            array_stride: None,
                            bit_field: None,
                        },
                    );
                    offset = advance_layout_offset(offset, 4)?;
                    continue;
                }
                let attr_align = self.skip_attributes()?;
                let element_size = type_size(element);
                loop {
                    let field_name = self.parse_identifier()?;
                    let mut count = u32::from(base_len);
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let extra = self.parse_integer_constant()? as u32;
                        self.expect(Token::BracketClose)?;
                        count = count.saturating_mul(extra);
                    }
                    let trailing_attr_align = self.skip_attributes()?;
                    let alignment = type_alignment(element)
                        .max(1)
                        .max(merged_attribute_alignment(attr_align, trailing_attr_align));
                    if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                        // mwcc TRIMS the container to the bytes its bits use
                        // (measured: 4 bits -> next byte member at +1; 9-12 bits
                        // -> +2; the container type still sets the alignment).
                        offset = unit_offset + u32::from(bits_used).div_ceil(8);
                    }
                    alignment_max = alignment_max.max(alignment);
                    offset = align_layout_offset(offset, alignment)?;
                    layout.insert_field(
                        field_name,
                        StructField {
                            member_type: element,
                            source_fundamental: source_fundamental(element),
                            offset,
                            struct_tag: None,
                            array_element: Some(pointee_of(element)?),
                            array_bytes: Some(count.saturating_mul(element_size)),
                            array_stride: None,
                            bit_field: None,
                        },
                    );
                    offset = advance_layout_offset(offset, count.saturating_mul(element_size))?;
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
                continue;
            }
            let field_is_function_pointer_typedef = matches!(self.peek(), Token::Identifier(word) if self.function_pointer_typedefs.contains_key(word));
            let mut field_type = self.parse_type()?;
            let source_fundamental = self.last_source_fundamental;
            while self.eat_keyword(Token::Star) {
                field_type = Type::Pointer(Pointee::Pointer);
                self.last_struct_tag = None;
            }
            // Only a ROW-POINTER typedef member reaches here (an array-typedef member
            // was intercepted above). It occupies one pointer word. Preserve its row
            // byte stride as the same safety marker used by an explicitly spelled
            // `T (*member)[N]`: recovering the containing layout is sound, while an
            // actual member access still defers instead of using scalar stride.
            let row_pointer_stride = match self.last_array_typedef.take() {
                Some((element, 0, length)) => {
                    Some(type_size(element).saturating_mul(u32::from(length)))
                }
                Some(_) => {
                    return Err(Diagnostic::error(
                        "an array-typedef struct member reached the scalar layout path",
                    ))
                }
                None => None,
            };
            let struct_tag = self.last_struct_tag.take();
            // A declarator may carry `__attribute__((aligned(n)))` between the type
            // and the name (e.g. `u8 ATTRIBUTE_ALIGN(4) board_data[32];`); skip it,
            // honouring any requested alignment so subsequent offsets stay exact.
            let attr_align = self.skip_attributes()?;
            // One or more comma-separated declarators share the field type, e.g.
            // `f32 x, y, z;`. Each gets its own naturally-aligned offset.
            loop {
                // A parenthesized pointer member is either a function pointer
                // `RET (*name)(params)` or a pointer to an array `T (*name)[N]`.
                // Both occupy one word. Keep the row byte stride as a marker for the
                // latter: recovering the containing layout is safe, while expression
                // parsing can still defer if code actually accesses the unmodeled row
                // pointer rather than silently using scalar-pointer stride.
                if *self.peek() == Token::ParenOpen
                    && self.tokens.get(self.position + 1) == Some(&Token::Star)
                {
                    self.advance(); // `(`
                    self.advance(); // `*`
                    let pointer_name = self.parse_identifier()?;
                    self.expect(Token::ParenClose)?;
                    let row_stride = if self.eat_keyword(Token::ParenOpen) {
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Token::ParenOpen => depth += 1,
                                Token::ParenClose => depth -= 1,
                                Token::EndOfFile => {
                                    return Err(Diagnostic::error(
                                        "unterminated function-pointer member",
                                    ))
                                }
                                _ => {}
                            }
                        }
                        None
                    } else if *self.peek() == Token::BracketOpen {
                        let (row_bytes, _) = self
                            .parse_array_declarator_extent(type_size(field_type))?
                            .expect("the array opener was checked above");
                        Some(row_bytes)
                    } else {
                        return Err(Diagnostic::error(
                            "a parenthesized pointer member must declare a function or array",
                        ));
                    };
                    if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                        // mwcc TRIMS the container to the bytes its bits use
                        // (measured: 4 bits -> next byte member at +1; 9-12 bits
                        // -> +2; the container type still sets the alignment).
                        offset = unit_offset + u32::from(bits_used).div_ceil(8);
                    }
                    let alignment = 4u32;
                    alignment_max = alignment_max.max(alignment);
                    offset = align_layout_offset(offset, alignment)?;
                    layout.insert_field(
                        pointer_name.clone(),
                        StructField {
                            member_type: if row_stride.is_some() {
                                Type::Pointer(pointee_of(field_type).unwrap_or(Pointee::Pointer))
                            } else {
                                Type::StructPointer { element_size: 0 }
                            },
                            source_fundamental: None,
                            offset,
                            struct_tag: None,
                            array_element: None,
                            array_bytes: None,
                            array_stride: row_stride,
                            bit_field: None,
                        },
                    );
                    if row_stride.is_none() {
                        layout.function_pointer_fields.insert(pointer_name);
                    }
                    offset = advance_layout_offset(offset, 4)?;
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // An anonymous bit-field `type : width;` is padding with no member: a
                // positive width advances the current allocation unit (opening a fresh
                // one if it overflows, same packing as a named bit-field), and a zero
                // width (`int : 0;`) closes the open unit so the next bit-field starts a
                // new one at the next boundary.
                if *self.peek() == Token::Colon {
                    self.advance();
                    let width = self.parse_integer_constant()? as u8;
                    if width == 0 {
                        bit_unit = None;
                    } else {
                        place_bit_field(
                            &mut bit_unit,
                            &mut offset,
                            &mut alignment_max,
                            field_type,
                            width,
                            u32::from(attr_align.unwrap_or(1)),
                        )?;
                    }
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                let field_name = self.parse_identifier()?;
                // A bit-field `type name : width` packs `width` bits (MSB-first) into a
                // `sizeof(type)` storage unit shared by adjacent same-typed bit-fields;
                // it overflows into a fresh unit. (Mixed-type adjacent bit-fields follow
                // CodeWarrior's irregular packing and defer; member access defers too.)
                if self.eat_keyword(Token::Colon) {
                    let width = self.parse_integer_constant()? as u8;
                    let (unit_offset, bit_offset) = place_bit_field(
                        &mut bit_unit,
                        &mut offset,
                        &mut alignment_max,
                        field_type,
                        width,
                        u32::from(attr_align.unwrap_or(1)),
                    )?;
                    layout.insert_field(
                        field_name,
                        StructField {
                            member_type: field_type,
                            source_fundamental,
                            offset: unit_offset,
                            struct_tag: None,
                            array_element: None,
                            array_bytes: None,
                            array_stride: None,
                            bit_field: Some((bit_offset, width)),
                        },
                    );
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // An ordinary member closes any open bit-field unit.
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + u32::from(bits_used).div_ceil(8);
                }
                // An array member `type name[N]` occupies `N` elements; its access
                // yields the array address rather than a loaded value.
                let mut array_element = None;
                let mut array_stride = row_pointer_stride;
                let mut is_array = false;
                let mut size = type_size(field_type);
                let element_size = size;
                if *self.peek() == Token::BracketOpen {
                    if row_pointer_stride.is_some() {
                        return Err(Diagnostic::error(
                            "an array of row-pointer-typedef members is not supported yet (roadmap)",
                        ));
                    }
                    is_array = true;
                    // A scalar array records its element type for indexed access. A
                    // struct-value array (`GXTexRegion TexRegions[8];`) or a pointer
                    // array (`u8 *mess_stack[8];`) has no scalar pointee — its element
                    // size still lays the array out correctly (4 bytes per pointer, the
                    // struct width per struct), so later members resolve; indexed
                    // element access defers in codegen rather than miscomputing.
                    if !matches!(
                        field_type,
                        Type::Struct { .. } | Type::Pointer(_) | Type::StructPointer { .. }
                    ) {
                        array_element = Some(pointee_of(field_type)?);
                    }
                    // Preserve the first index's byte stride as well as total size:
                    // `field[R][C]` advances `C * sizeof(element)` for `field[row]`.
                    let (total_bytes, first_index_stride) = self
                        .parse_array_declarator_extent(element_size)?
                        .expect("the array opener was checked above");
                    size = total_bytes;
                    array_stride = first_index_stride;
                }
                // GCC/CodeWarrior also accepts the attribute on the declarator,
                // after its name or array dimensions. It aligns this member and
                // the containing aggregate without changing the member's size.
                let trailing_attr_align = self.skip_attributes()?;
                // Natural alignment: to the element's alignment (a struct value to its
                // own alignment, every other type to its size — for an array, that
                // element's).
                let alignment = type_alignment(field_type)
                    .max(1)
                    .max(merged_attribute_alignment(attr_align, trailing_attr_align));
                alignment_max = alignment_max.max(alignment);
                offset = align_layout_offset(offset, alignment)?;
                layout.insert_field(
                    field_name.clone(),
                    StructField {
                        member_type: field_type,
                        source_fundamental,
                        offset,
                        struct_tag: struct_tag.clone(),
                        array_element,
                        array_bytes: is_array.then_some(size),
                        array_stride,
                        bit_field: None,
                    },
                );
                if field_is_function_pointer_typedef && !is_array {
                    layout.function_pointer_fields.insert(field_name);
                }
                offset = advance_layout_offset(offset, size)?;
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Semicolon)?;
        }
        self.expect(Token::BraceClose)?;
        // The struct size includes trailing padding to its own alignment.
        layout.size = align_layout_offset(offset, alignment_max)?;
        layout.align = alignment_max as u8;
        Ok(layout)
    }

    pub(crate) fn parse_inline_union_declarator(
        &mut self,
    ) -> Compilation<(Option<String>, StructLayout, usize, Option<String>)> {
        self.advance(); // `union`
        let tag = if matches!(self.peek(), Token::Identifier(_)) {
            Some(self.parse_identifier()?)
        } else {
            None
        };
        let union_name = tag.as_deref().unwrap_or("<anonymous>");
        let inner = self.parse_union_body().map_err(|error| {
            Diagnostic::error(format!(
                "union layout '{union_name}' was not recovered: {error}"
            ))
        })?;
        let mut pointer_depth = 0usize;
        while self.eat_keyword(Token::Star) {
            pointer_depth += 1;
        }
        let member_name = if matches!(self.peek(), Token::Identifier(_)) {
            Some(self.parse_identifier()?)
        } else {
            None
        };
        Ok((tag, inner, pointer_depth, member_name))
    }

    /// Parse and place one inline union declaration in an enclosing aggregate.
    /// C structs and C++ classes use exactly the same storage rules; returning
    /// the visible field names lets the C++ declaration model retain its member
    /// order without duplicating union layout arithmetic in `cxx.rs`.
    pub(crate) fn parse_and_place_inline_union(
        &mut self,
        layout: &mut StructLayout,
        offset: &mut u32,
        alignment_max: &mut u32,
    ) -> Compilation<Vec<String>> {
        let (tag, inner, pointer_depth, member_name) =
            self.parse_inline_union_declarator()?;
        self.expect(Token::Semicolon)?;
        let inner_size = inner.size;
        let inner_align = u32::from(inner.align).max(1);
        match (tag, pointer_depth, member_name) {
            (tag, depth, Some(name)) => {
                let variant_tag =
                    tag.unwrap_or_else(|| format!("@anon{}", self.structs.len()));
                self.structs.insert(variant_tag.clone(), inner);
                let (member_type, member_align, member_size, struct_tag) = if depth > 0 {
                    (
                        if depth == 1 {
                            Type::StructPointer {
                                element_size: inner_size,
                            }
                        } else {
                            Type::Pointer(Pointee::Pointer)
                        },
                        4,
                        4,
                        (depth == 1).then_some(variant_tag),
                    )
                } else {
                    (
                        Type::Struct {
                            size: inner_size,
                            align: inner_align as u8,
                        },
                        inner_align,
                        inner_size,
                        Some(variant_tag),
                    )
                };
                *alignment_max = (*alignment_max).max(member_align);
                *offset = align_layout_offset(*offset, member_align)?;
                layout.insert_field(
                    name.clone(),
                    StructField {
                        member_type,
                        source_fundamental: None,
                        offset: *offset,
                        struct_tag,
                        array_element: None,
                        array_bytes: None,
                        array_stride: None,
                        bit_field: None,
                    },
                );
                *offset = advance_layout_offset(*offset, member_size)?;
                Ok(vec![name])
            }
            // `union Tag { … };` registers its tag but contributes no member.
            (Some(tag), 0, None) => {
                self.structs.insert(tag, inner);
                Ok(Vec::new())
            }
            // `union { … };` promotes every overlapping member into its owner.
            (None, 0, None) => {
                *alignment_max = (*alignment_max).max(inner_align);
                *offset = align_layout_offset(*offset, inner_align)?;
                let mut names = Vec::new();
                for (field_name, field) in inner.fields_in_declaration_order() {
                    layout.insert_field(
                        field_name.clone(),
                        StructField {
                            member_type: field.member_type,
                            source_fundamental: field.source_fundamental,
                            offset: *offset + field.offset,
                            struct_tag: field.struct_tag.clone(),
                            array_element: field.array_element,
                            array_bytes: field.array_bytes,
                            array_stride: field.array_stride,
                            bit_field: field.bit_field,
                        },
                    );
                    names.push(field_name.clone());
                }
                layout
                    .function_pointer_fields
                    .extend(inner.function_pointer_fields.iter().cloned());
                *offset = advance_layout_offset(*offset, inner_size)?;
                Ok(names)
            }
            (_, _, None) => Err(Diagnostic::error(
                "an inline-union pointer member needs a name",
            )),
        }
    }

    /// Parse and place one inline struct declaration in an enclosing aggregate.
    /// C aggregates and C++ classes share these nested-storage and anonymous
    /// member-promotion rules.
    pub(crate) fn parse_and_place_inline_struct(
        &mut self,
        layout: &mut StructLayout,
        offset: &mut u32,
        alignment_max: &mut u32,
    ) -> Compilation<Vec<String>> {
        self.expect(Token::KeywordStruct)?;
        let tag = if matches!(self.peek(), Token::Identifier(_)) {
            Some(self.parse_identifier()?)
        } else {
            None
        };
        let inner = self.parse_struct_body()?;
        let inner_size = inner.size;
        let inner_align = u32::from(inner.align).max(1);
        let mut pointer_depth = 0usize;
        while self.eat_keyword(Token::Star) {
            pointer_depth += 1;
        }
        let mut member_names = Vec::new();
        if matches!(self.peek(), Token::Identifier(_)) {
            member_names.push(self.parse_identifier()?);
            while self.eat_keyword(Token::Comma) {
                member_names.push(self.parse_identifier()?);
            }
        }
        if pointer_depth > 0 {
            let [name] = member_names.as_slice() else {
                return Err(Diagnostic::error(
                    "an inline-struct pointer declaration requires exactly one member",
                ));
            };
            self.expect(Token::Semicolon)?;
            let layout_tag = tag.unwrap_or_else(|| format!("@anon{}", self.structs.len()));
            self.structs.insert(layout_tag.clone(), inner);
            *alignment_max = (*alignment_max).max(4);
            *offset = align_layout_offset(*offset, 4)?;
            layout.insert_field(
                name.clone(),
                StructField {
                    member_type: if pointer_depth == 1 {
                        Type::StructPointer {
                            element_size: inner_size,
                        }
                    } else {
                        Type::Pointer(Pointee::Pointer)
                    },
                    source_fundamental: None,
                    offset: *offset,
                    struct_tag: (pointer_depth == 1).then_some(layout_tag),
                    array_element: None,
                    array_bytes: None,
                    array_stride: None,
                    bit_field: None,
                },
            );
            *offset = advance_layout_offset(*offset, 4)?;
            return Ok(vec![name.clone()]);
        }

        let mut array_count: Option<u32> = None;
        while *self.peek() == Token::BracketOpen {
            self.advance();
            let dimension = self.parse_integer_constant()? as u32;
            array_count = Some(array_count.unwrap_or(1).saturating_mul(dimension));
            self.expect(Token::BracketClose)?;
        }
        let member_bytes = array_count.map_or(inner_size, |count| count.saturating_mul(inner_size));
        let array_bytes = array_count.map(|count| count.saturating_mul(inner_size));
        let names = match (tag, member_names.is_empty()) {
            (Some(tag), false) => {
                self.structs.insert(tag.clone(), inner);
                *alignment_max = (*alignment_max).max(inner_align);
                for name in &member_names {
                    *offset = align_layout_offset(*offset, inner_align)?;
                    layout.insert_field(
                        name.clone(),
                        StructField {
                            member_type: Type::Struct {
                                size: inner_size,
                                align: inner_align as u8,
                            },
                            source_fundamental: None,
                            offset: *offset,
                            struct_tag: Some(tag.clone()),
                            array_element: None,
                            array_bytes,
                            array_stride: None,
                            bit_field: None,
                        },
                    );
                    *offset = advance_layout_offset(*offset, member_bytes)?;
                }
                member_names
            }
            (Some(tag), true) => {
                self.structs.insert(tag, inner);
                Vec::new()
            }
            (None, false) => {
                let synthetic = format!("@anon{}", self.structs.len());
                self.structs.insert(synthetic.clone(), inner);
                *alignment_max = (*alignment_max).max(inner_align);
                for name in &member_names {
                    *offset = align_layout_offset(*offset, inner_align)?;
                    layout.insert_field(
                        name.clone(),
                        StructField {
                            member_type: Type::Struct {
                                size: inner_size,
                                align: inner_align as u8,
                            },
                            source_fundamental: None,
                            offset: *offset,
                            struct_tag: Some(synthetic.clone()),
                            array_element: None,
                            array_bytes,
                            array_stride: None,
                            bit_field: None,
                        },
                    );
                    *offset = advance_layout_offset(*offset, member_bytes)?;
                }
                member_names
            }
            (None, true) => {
                *alignment_max = (*alignment_max).max(inner_align);
                *offset = align_layout_offset(*offset, inner_align)?;
                let mut names = Vec::new();
                for (field_name, field) in inner.fields_in_declaration_order() {
                    layout.insert_field(
                        field_name.clone(),
                        StructField {
                            member_type: field.member_type,
                            source_fundamental: field.source_fundamental,
                            offset: *offset + field.offset,
                            struct_tag: field.struct_tag.clone(),
                            array_element: field.array_element,
                            array_bytes: field.array_bytes,
                            array_stride: field.array_stride,
                            bit_field: field.bit_field,
                        },
                    );
                    names.push(field_name.clone());
                }
                layout
                    .function_pointer_fields
                    .extend(inner.function_pointer_fields.iter().cloned());
                *offset = advance_layout_offset(*offset, inner_size)?;
                names
            }
        };
        self.expect(Token::Semicolon)?;
        Ok(names)
    }

    /// Parse a `union { … }` body: every member starts at offset 0, so the union's
    /// size is its largest member and its alignment the strictest. Supports the
    /// common shape — a scalar, pointer, or struct-value member per line (each
    /// keeps its `struct_tag` so `u.member.field` still chains). The irregular
    /// cases — bit-fields, arrays, multiple declarators, nested inline aggregates —
    /// defer rather than risk a wrong offset.
    pub(crate) fn parse_union_body(&mut self) -> Compilation<StructLayout> {
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        layout.is_union = true;
        let mut max_size: u32 = 0;
        let mut max_align: u32 = 1;
        while *self.peek() != Token::BraceClose {
            if matches!(self.peek(), Token::Identifier(word) if word == "union")
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                let (tag, inner, pointer_depth, member_name) =
                    self.parse_inline_union_declarator()?;
                let inner_size = inner.size;
                let inner_align = (inner.align as u32).max(1);
                self.expect(Token::Semicolon)?;
                let variant_tag =
                    tag.unwrap_or_else(|| format!("@anon{}", self.structs.len()));
                self.structs.insert(variant_tag.clone(), inner.clone());
                match (pointer_depth, member_name) {
                    (depth, Some(name)) if depth > 0 => {
                        layout.insert_field(
                            name,
                            StructField {
                                member_type: if depth == 1 {
                                    Type::StructPointer {
                                        element_size: inner_size,
                                    }
                                } else {
                                    Type::Pointer(Pointee::Pointer)
                                },
                                source_fundamental: None,
                                offset: 0,
                                struct_tag: (depth == 1).then_some(variant_tag),
                                array_element: None,
                                array_bytes: None,
                                array_stride: None,
                                bit_field: None,
                            },
                        );
                        max_size = max_size.max(4);
                        max_align = max_align.max(4);
                    }
                    (0, Some(name)) => {
                        layout.insert_field(
                            name,
                            StructField {
                                member_type: Type::Struct {
                                    size: inner_size,
                                    align: inner_align as u8,
                                },
                                source_fundamental: None,
                                offset: 0,
                                struct_tag: Some(variant_tag),
                                array_element: None,
                                array_bytes: None,
                                array_stride: None,
                                bit_field: None,
                            },
                        );
                        max_size = max_size.max(inner_size);
                        max_align = max_align.max(inner_align);
                    }
                    (0, None) => {
                        for (field_name, field) in inner.fields_in_declaration_order() {
                            layout.insert_field(field_name.clone(), field.clone());
                        }
                        max_size = max_size.max(inner_size);
                        max_align = max_align.max(inner_align);
                    }
                    (_, None) => {
                        return Err(Diagnostic::error(
                            "an inline-union pointer member needs a name",
                        ));
                    }
                    _ => unreachable!(),
                }
                continue;
            }
            // An inline struct *variant* of the union (`struct [Tag] { … } name;`),
            // e.g. HsfObjectData's `mesh`. Register its layout under a tag so
            // `u.name.field` chains, then add it as a struct-value variant at offset 0.
            if *self.peek() == Token::KeywordStruct
                && (self.tokens.get(self.position + 1) == Some(&Token::BraceOpen)
                    || self.tokens.get(self.position + 2) == Some(&Token::BraceOpen))
            {
                self.advance(); // `struct`
                let tag = if matches!(self.peek(), Token::Identifier(_)) {
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                let inner = self.parse_struct_body()?;
                let inner_size = inner.size;
                let inner_align = (inner.align as u32).max(1);
                // A NAMED inline struct variant (`struct {…} name;`) registers as a struct-value
                // field so `u.name.field` chains. An ANONYMOUS one (`struct {…};`) flattens its
                // fields into the union at the union base (offset 0), each keeping its struct-relative
                // offset — C anonymous-member promotion. This is GXData's fog/z overlay
                // (`union { struct { u8 fgRange; …; f32 fgSideX; }; struct { f32 zOffset; f32 zScale; }; }`),
                // whose members are accessed directly on the enclosing struct.
                if matches!(self.peek(), Token::Identifier(_)) {
                    let name = self.parse_identifier()?;
                    let variant_tag = tag.unwrap_or_else(|| format!("@anon{}", self.structs.len()));
                    self.structs.insert(variant_tag.clone(), inner);
                    layout.insert_field(
                        name,
                        StructField {
                            member_type: Type::Struct {
                                size: inner_size,
                                align: inner_align as u8,
                            },
                            source_fundamental: None,
                            offset: 0,
                            struct_tag: Some(variant_tag),
                            array_element: None,
                            array_bytes: None,
                            array_stride: None,
                            bit_field: None,
                        },
                    );
                } else {
                    for (field_name, field) in inner.fields_in_declaration_order() {
                        layout.insert_field(
                            field_name.clone(),
                            StructField {
                                member_type: field.member_type,
                                source_fundamental: field.source_fundamental,
                                offset: field.offset,
                                struct_tag: field.struct_tag.clone(),
                                array_element: field.array_element,
                                array_bytes: field.array_bytes,
                                array_stride: field.array_stride,
                                bit_field: field.bit_field,
                            },
                        );
                    }
                    layout
                        .function_pointer_fields
                        .extend(inner.function_pointer_fields.iter().cloned());
                }
                max_size = max_size.max(inner_size);
                max_align = max_align.max(inner_align);
                self.expect(Token::Semicolon)?;
                continue;
            }
            let mut field_type = self.parse_type()?;
            let source_fundamental = self.last_source_fundamental;
            while self.eat_keyword(Token::Star) {
                field_type = Type::Pointer(Pointee::Pointer);
                self.last_struct_tag = None;
            }
            let array_typedef = self.last_array_typedef.take();
            let struct_tag = self.last_struct_tag.take();
            let attr_align = self.skip_attributes()?;
            let name = self.parse_identifier()?;
            if self.eat_keyword(Token::Colon) {
                let width = self.parse_integer_constant()? as u8;
                if width == 0 || width > field_type.width() {
                    return Err(Diagnostic::error(
                        "a union bit-field width must fit its declared type",
                    ));
                }
                let size = type_size(field_type);
                let align = type_alignment(field_type)
                    .max(1)
                    .max(u32::from(attr_align.unwrap_or(1)));
                layout.insert_field(
                    name,
                    StructField {
                        member_type: field_type,
                        source_fundamental,
                        offset: 0,
                        struct_tag: None,
                        array_element: None,
                        array_bytes: None,
                        array_stride: None,
                        bit_field: Some((0, width)),
                    },
                );
                max_size = max_size.max(size);
                max_align = max_align.max(align);
                self.expect(Token::Semicolon)?;
                continue;
            }
            let mut names = vec![name];
            while self.eat_keyword(Token::Comma) {
                names.push(self.parse_identifier()?);
            }
            // An array member occupies the product of its dimensions; it still
            // starts at offset 0, so it only widens the union.
            let mut array_element = array_typedef
                .map(|(element, _, _)| pointee_of(element))
                .transpose()?;
            let mut array_stride = array_typedef.and_then(|(element, total, inner)| {
                (total != 0 && inner != total)
                    .then(|| u32::from(inner).saturating_mul(type_size(element)))
            });
            let mut is_array = array_typedef.is_some_and(|(_, total, _)| total != 0);
            let mut size = match array_typedef {
                Some((element, total, _)) if total != 0 => u32::from(total) * type_size(element),
                Some(_) => 4,
                None => type_size(field_type),
            };
            if *self.peek() == Token::BracketOpen {
                is_array = true;
                if array_element.is_none()
                    && !matches!(
                        field_type,
                        Type::Struct { .. } | Type::Pointer(_) | Type::StructPointer { .. }
                    )
                {
                    array_element = Some(pointee_of(field_type)?);
                }
                let (total_bytes, first_index_stride) = self
                    .parse_array_declarator_extent(size)?
                    .expect("the array opener was checked above");
                size = total_bytes;
                array_stride = first_index_stride;
            }
            let storage_type = array_typedef.map_or(field_type, |(element, total, _)| {
                if total == 0 {
                    Type::Pointer(Pointee::Pointer)
                } else {
                    element
                }
            });
            let align = type_alignment(storage_type)
                .max(1)
                .max(u32::from(attr_align.unwrap_or(1)));
            for name in names {
                layout.insert_field(
                    name,
                    StructField {
                        member_type: storage_type,
                        source_fundamental,
                        offset: 0,
                        struct_tag: struct_tag.clone(),
                        array_element,
                        array_bytes: is_array.then_some(size),
                        array_stride,
                        bit_field: None,
                    },
                );
            }
            max_size = max_size.max(size);
            max_align = max_align.max(align);
            self.expect(Token::Semicolon)?;
        }
        self.expect(Token::BraceClose)?;
        layout.size = max_size.div_ceil(max_align) * max_align;
        layout.align = max_align as u8;
        Ok(layout)
    }
}
