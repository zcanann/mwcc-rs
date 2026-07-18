//! Type parsing and struct/union body layout: `parse_type` (the full declarator
//! grammar — qualifiers, pointers, typedef names, enum/struct/union references) and
//! the struct/union field-layout builders. Part of the `items` module.

use super::*;
use crate::parser::{Parser, StructField, StructLayout};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{
    Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter,
    Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type,
};
use mwcc_tokens::Token;

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Compilation<Type> {
        let parsed = self.parse_type_base()?;
        // A POSTFIX qualifier — east const: `unsigned char const *jp` (metroid
        // prime's ansi_fp revision) — reads exactly like the prefix form.
        while matches!(self.peek(), Token::Identifier(word) if word == "const") {
            self.advance();
            self.last_type_was_const = true;
        }
        Ok(parsed)
    }

    fn parse_type_base(&mut self) -> Compilation<Type> {
        self.last_struct_tag = None;
        self.last_pointer_const = false;
        // The array-typedef marker is only ever set by the LAST parse_type call, so a
        // consumer that `.take()`s right after its own call can never read a stale one.
        self.last_array_typedef = None;
        // Leading qualifiers: `const`/`register` are transparent to codegen (`const`
        // is noted for the global path, which defers a read-only global); `volatile`
        // changes access semantics (memory accesses can't be elided), so defer it.
        self.skip_type_qualifiers()?;
        // `enum [Tag] [{ … }]` — an `int` (`-enum int`); a `{ … }` body registers
        // its enumerators so a bare enumerator resolves to its value.
        if matches!(self.peek(), Token::Identifier(word) if word == "enum") {
            self.advance();
            let tagged = matches!(self.peek(), Token::Identifier(_));
            if tagged {
                self.advance(); // the tag
            }
            if *self.peek() == Token::BraceOpen {
                // An ANONYMOUS enum definition consumes one anonymous-`@N` number
                // (measured fire 494: `typedef enum {…} E;` shifts the next pool
                // constant by +1; a TAGGED enum adds nothing — pikmin's uart TU
                // carries three such enums between its inlines and its statics).
                // Keyed by token position so a speculative re-parse can't double-count.
                if !tagged && self.counted_enum_positions.insert(self.position) {
                    self.skipped_inline_functions += 1;
                }
                self.parse_enum_body()?;
            }
            return Ok(Type::Int);
        }
        // `struct Name*` — a pointer to a (already declared) struct. The tag is
        // stashed in `last_struct_tag` for the declarator parser to record.
        if *self.peek() == Token::KeywordStruct {
            self.advance();
            let tag = self.parse_identifier()?;
            if *self.peek() != Token::Star {
                // A struct *value*: a known layout becomes a sized struct value
                // (a frame-resident local); an opaque/unknown struct still defers.
                return match self.struct_value_type(&tag) {
                    Some(struct_type) => {
                        self.last_struct_tag = Some(tag);
                        Ok(struct_type)
                    }
                    None => Err(Diagnostic::error(
                        "struct values are not supported yet — use a struct pointer",
                    )),
                };
            }
            self.advance();
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
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
            if *self.peek() != Token::Star {
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
            self.advance();
            let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
            self.last_struct_tag = Some(tag);
            if *self.peek() == Token::Star {
                // `S**` — a pointer to a struct pointer: a word-classed
                // pointer whose element is itself a pointer.
                self.advance();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
            return Ok(Type::StructPointer { element_size });
        }
        // A struct-pointer typedef (`VecPtr`) is itself a pointer to the struct —
        // no trailing `*` — carrying the layout's tag.
        if let Token::Identifier(name) = self.peek() {
            if let Some(tag) = self.struct_pointer_typedefs.get(name).cloned() {
                self.advance();
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
            if let Some(tag) = self.struct_typedefs.get(name).cloned() {
                self.advance();
                if *self.peek() != Token::Star {
                    return match self.struct_value_type(&tag) {
                        Some(struct_type) => {
                            self.last_struct_tag = Some(tag);
                            Ok(struct_type)
                        }
                        None => Err(Diagnostic::error(
                            "struct values are not supported yet — use a struct pointer",
                        )),
                    };
                }
                self.advance();
                let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
                self.last_struct_tag = Some(tag);
                if *self.peek() == Token::Star {
                    self.advance();
                    return Ok(Type::Pointer(Pointee::Pointer));
                }
                return Ok(Type::StructPointer { element_size });
            }
        }
        // A `typedef`-declared alias resolves to its underlying type.
        if let Token::Identifier(name) = self.peek() {
            if let Some(&aliased) = self.typedefs.get(name) {
                self.advance();
                if *self.peek() == Token::Star {
                    self.advance();
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
        let base = match self.advance() {
            Token::KeywordInt => Type::Int,
            // Plain `char` follows the build's signedness default: signed on
            // mainline/1.3.2+, UNSIGNED on GC/1.3 (build 53) and `-char unsigned`
            // (no `extsb` on read). `signed char`/`unsigned char` below are explicit.
            Token::KeywordChar => {
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
                        Type::UnsignedInt
                    }
                }
                _ => Type::UnsignedInt,
            },
            Token::KeywordFloat => Type::Float,
            Token::KeywordVoid => Type::Void,
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
                // `signed`, `signed int`, `signed long [long] [int]` — all 32-bit
                // signed on this target.
                _ => {
                    while self.eat_word("long") {}
                    let _ = self.eat_keyword(Token::KeywordInt);
                    Type::Int
                }
            },
            other => return Err(Diagnostic::error(format!("expected a type, found {other}"))),
        };
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

    pub(crate) fn parse_struct_body(&mut self) -> Compilation<StructLayout> {
        self.expect(Token::BraceOpen)?;
        let mut layout = StructLayout::default();
        let mut offset: u16 = 0;
        let mut alignment_max: u16 = 1;
        // The open bit-field allocation unit (its type, byte offset, bits used so
        // far); an ordinary member or a different-typed bit-field closes it.
        let mut bit_unit: Option<(Type, u16, u8)> = None;
        while *self.peek() != Token::BraceClose {
            // An inline struct definition as a member: `struct [Tag] { … } [name];`. An
            // ANONYMOUS one with no member name promotes (flattens) its fields into this
            // struct — C anonymous-struct semantics, and how the game-state structs wrap
            // their bit-fields. A named-tag form registers the tag (and adds a nested
            // struct-value member if a name follows).
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
                let inner_align = (inner.align as u16).max(1);
                let member_name = if matches!(self.peek(), Token::Identifier(_)) {
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                // An inline struct member may be an ARRAY — `struct { … } queue[3];` (EXIControl's
                // callback queue). Parse the dimension(s); `array_bytes` is the total so the fields
                // after it lay out correctly (`count * inner_size`), `None` for a scalar member.
                let mut array_count: Option<u16> = None;
                while *self.peek() == Token::BracketOpen {
                    self.advance();
                    let dimension = self.parse_integer_constant()? as u16;
                    array_count = Some(array_count.unwrap_or(1).saturating_mul(dimension));
                    self.expect(Token::BracketClose)?;
                }
                let member_bytes =
                    array_count.map_or(inner_size, |count| count.saturating_mul(inner_size));
                let array_bytes = array_count.map(|count| count.saturating_mul(inner_size));
                match (tag, member_name) {
                    (Some(tag), Some(name)) => {
                        self.structs.insert(tag.clone(), inner);
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        layout.fields.insert(
                            name,
                            StructField {
                                member_type: Type::Struct {
                                    size: inner_size,
                                    align: inner_align as u8,
                                },
                                offset,
                                struct_tag: Some(tag),
                                array_element: None,
                                array_bytes,
                                bit_field: None,
                            },
                        );
                        offset += member_bytes;
                    }
                    (Some(tag), None) => {
                        // A named struct type registered inside this one (no member).
                        self.structs.insert(tag, inner);
                    }
                    (None, Some(name)) => {
                        // An anonymous inline struct *named* as a member (`struct {…}
                        // mesh;`). Register its layout under a synthetic tag (unique —
                        // generated after the inner parse, so nested anon structs don't
                        // collide) so `parent.mesh.field` chains, then add it as an
                        // ordinary struct-value member.
                        let synthetic = format!("@anon{}", self.structs.len());
                        self.structs.insert(synthetic.clone(), inner);
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        layout.fields.insert(
                            name,
                            StructField {
                                member_type: Type::Struct {
                                    size: inner_size,
                                    align: inner_align as u8,
                                },
                                offset,
                                struct_tag: Some(synthetic),
                                array_element: None,
                                array_bytes,
                                bit_field: None,
                            },
                        );
                        offset += member_bytes;
                    }
                    (None, None) => {
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        for (field_name, field) in &inner.fields {
                            layout.fields.insert(
                                field_name.clone(),
                                StructField {
                                    member_type: field.member_type,
                                    offset: offset + field.offset,
                                    struct_tag: field.struct_tag.clone(),
                                    array_element: field.array_element,
                                    array_bytes: field.array_bytes,
                                    bit_field: field.bit_field,
                                },
                            );
                        }
                        offset += inner_size;
                    }
                }
                self.expect(Token::Semicolon)?;
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
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
                self.advance(); // `union`
                let tag = if matches!(self.peek(), Token::Identifier(_)) {
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                let inner = self.parse_union_body()?;
                let inner_size = inner.size;
                let inner_align = (inner.align as u16).max(1);
                let member_name = if matches!(self.peek(), Token::Identifier(_)) {
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                self.expect(Token::Semicolon)?;
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
                match (tag, member_name) {
                    // A named WORD-SIZED union member (`union {…} f_data;` — the ptmf
                    // function-pointer payload): lay it out as a field at the aligned
                    // offset occupying the union's size. A 4-byte union reads/writes as
                    // its word representation (a union value copy is a word copy), so it
                    // registers as UnsignedInt; member-of-member access (`.f_addr`) does
                    // not resolve through it and defers. Other sizes keep deferring.
                    (_, Some(name)) => {
                        if inner_size != 4 {
                            return Err(Diagnostic::error(
                                "a named union member of this size is not supported yet (roadmap)",
                            ));
                        }
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        layout.fields.insert(
                            name,
                            StructField {
                                member_type: Type::UnsignedInt,
                                offset,
                                struct_tag: None,
                                array_element: None,
                                array_bytes: None,
                                bit_field: None,
                            },
                        );
                        offset += inner_size;
                    }
                    // `union Tag { … };` — register the tag, no member contributed.
                    (Some(tag), None) => {
                        self.structs.insert(tag, inner);
                    }
                    // `union { … };` — flatten every member at the union's offset.
                    (None, None) => {
                        alignment_max = alignment_max.max(inner_align);
                        offset = offset.div_ceil(inner_align) * inner_align;
                        for (field_name, field) in &inner.fields {
                            layout.fields.insert(
                                field_name.clone(),
                                StructField {
                                    member_type: field.member_type,
                                    offset: offset + field.offset,
                                    struct_tag: field.struct_tag.clone(),
                                    array_element: field.array_element,
                                    array_bytes: field.array_bytes,
                                    bit_field: field.bit_field,
                                },
                            );
                        }
                        offset += inner_size;
                    }
                }
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
                    offset = offset.div_ceil(4) * 4;
                    alignment_max = alignment_max.max(4);
                    layout.fields.insert(
                        field_name,
                        StructField {
                            member_type: Type::Pointer(pointee_of(element)?),
                            offset,
                            struct_tag: None,
                            array_element: None,
                            array_bytes: None,
                            bit_field: None,
                        },
                    );
                    offset += 4;
                    continue;
                }
                let attr_align = self.skip_attributes()?;
                let element_size = type_size(element);
                let alignment = type_alignment(element).max(1).max(attr_align.unwrap_or(1));
                loop {
                    let field_name = self.parse_identifier()?;
                    let mut count = base_len;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let extra = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        count = count.saturating_mul(extra);
                    }
                    if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                        // mwcc TRIMS the container to the bytes its bits use
                        // (measured: 4 bits -> next byte member at +1; 9-12 bits
                        // -> +2; the container type still sets the alignment).
                        offset = unit_offset + (bits_used as u16).div_ceil(8);
                    }
                    alignment_max = alignment_max.max(alignment);
                    offset = offset.div_ceil(alignment) * alignment;
                    layout.fields.insert(
                        field_name,
                        StructField {
                            member_type: element,
                            offset,
                            struct_tag: None,
                            array_element: Some(pointee_of(element)?),
                            array_bytes: Some(count.saturating_mul(element_size)),
                            bit_field: None,
                        },
                    );
                    offset += count.saturating_mul(element_size);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
                continue;
            }
            let field_type = self.parse_type()?;
            // Only a ROW-POINTER typedef member reaches here (an array-typedef member
            // was intercepted above); its subscript stride isn't carried through the
            // member model yet, so defer rather than lay it out as a plain pointer
            // that a later `s->m[i][j]` would stride wrongly through.
            if self.last_array_typedef.take().is_some() {
                return Err(Diagnostic::error(
                    "a row-pointer-typedef struct member is not supported yet (roadmap)",
                ));
            }
            let struct_tag = self.last_struct_tag.take();
            // A declarator may carry `__attribute__((aligned(n)))` between the type
            // and the name (e.g. `u8 ATTRIBUTE_ALIGN(4) board_data[32];`); skip it,
            // honouring any requested alignment so subsequent offsets stay exact.
            let attr_align = self.skip_attributes()?;
            // One or more comma-separated declarators share the field type, e.g.
            // `f32 x, y, z;`. Each gets its own naturally-aligned offset.
            loop {
                // A function-pointer member `RET (*name)(params)` is a 4-byte pointer
                // (the `field_type` parsed above is just the return type). Consume the
                // declarator and record a pointer-typed member so `p->name` resolves.
                if *self.peek() == Token::ParenOpen
                    && self.tokens.get(self.position + 1) == Some(&Token::Star)
                {
                    self.advance(); // `(`
                    self.advance(); // `*`
                    let pointer_name = self.parse_identifier()?;
                    self.expect(Token::ParenClose)?;
                    self.expect(Token::ParenOpen)?;
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
                    if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                        // mwcc TRIMS the container to the bytes its bits use
                        // (measured: 4 bits -> next byte member at +1; 9-12 bits
                        // -> +2; the container type still sets the alignment).
                        offset = unit_offset + (bits_used as u16).div_ceil(8);
                    }
                    let alignment = 4u16;
                    alignment_max = alignment_max.max(alignment);
                    offset = offset.div_ceil(alignment) * alignment;
                    layout.fields.insert(
                        pointer_name,
                        StructField {
                            member_type: Type::StructPointer { element_size: 0 },
                            offset,
                            struct_tag: None,
                            array_element: None,
                            array_bytes: None,
                            bit_field: None,
                        },
                    );
                    offset += 4;
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
                    let unit_bits = (type_size(field_type) * 8) as u8;
                    if width == 0 {
                        bit_unit = None;
                    } else if width > unit_bits {
                        return Err(Diagnostic::error(
                            "an unsupported anonymous bit-field width (roadmap)",
                        ));
                    } else {
                        match bit_unit {
                            Some((unit_type, unit_offset, bits_used))
                                if unit_type == field_type && bits_used + width <= unit_bits =>
                            {
                                bit_unit = Some((field_type, unit_offset, bits_used + width));
                            }
                            Some((unit_type, ..)) if unit_type != field_type => {
                                return Err(Diagnostic::error("a struct mixing adjacent bit-field types is not supported yet (roadmap)"));
                            }
                            _ => {
                                let alignment = type_alignment(field_type)
                                    .max(1)
                                    .max(attr_align.unwrap_or(1));
                                let unit_offset = offset.div_ceil(alignment) * alignment;
                                offset = unit_offset + type_size(field_type);
                                alignment_max = alignment_max.max(alignment);
                                bit_unit = Some((field_type, unit_offset, width));
                            }
                        }
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
                    let unit_bits = (type_size(field_type) * 8) as u8;
                    if width == 0 || width > unit_bits {
                        return Err(Diagnostic::error(
                            "an unsupported bit-field width (roadmap)",
                        ));
                    }
                    let (unit_offset, bit_offset) = match bit_unit {
                        Some((unit_type, unit_offset, bits_used))
                            if unit_type == field_type && bits_used + width <= unit_bits =>
                        {
                            bit_unit = Some((field_type, unit_offset, bits_used + width));
                            (unit_offset, bits_used)
                        }
                        Some((unit_type, ..)) if unit_type != field_type => {
                            return Err(Diagnostic::error("a struct mixing adjacent bit-field types is not supported yet (roadmap)"));
                        }
                        _ => {
                            let alignment = type_alignment(field_type)
                                .max(1)
                                .max(attr_align.unwrap_or(1));
                            let unit_offset = offset.div_ceil(alignment) * alignment;
                            offset = unit_offset + type_size(field_type);
                            alignment_max = alignment_max.max(alignment);
                            bit_unit = Some((field_type, unit_offset, width));
                            (unit_offset, 0)
                        }
                    };
                    layout.fields.insert(
                        field_name,
                        StructField {
                            member_type: field_type,
                            offset: unit_offset,
                            struct_tag: None,
                            array_element: None,
                            array_bytes: None,
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
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
                // An array member `type name[N]` occupies `N` elements; its access
                // yields the array address rather than a loaded value.
                let mut array_element = None;
                let mut is_array = false;
                let mut size = type_size(field_type);
                let element_size = size;
                if *self.peek() == Token::BracketOpen {
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
                    // One or more dimensions — `field[N]`, `field[R][C]`, … — occupy the
                    // product of the (constant-expression) lengths times the element
                    // size. (Member *access* of a multi-dimensional field still defers in
                    // codegen; the layout is needed so the rest of the struct registers.)
                    let mut total: u16 = 1;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let count = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        total = total.saturating_mul(count);
                    }
                    size = total * element_size;
                }
                // Natural alignment: to the element's alignment (a struct value to its
                // own alignment, every other type to its size — for an array, that
                // element's).
                let alignment = type_alignment(field_type)
                    .max(1)
                    .max(attr_align.unwrap_or(1));
                alignment_max = alignment_max.max(alignment);
                offset = offset.div_ceil(alignment) * alignment;
                layout.fields.insert(
                    field_name,
                    StructField {
                        member_type: field_type,
                        offset,
                        struct_tag: struct_tag.clone(),
                        array_element,
                        array_bytes: is_array.then_some(size),
                        bit_field: None,
                    },
                );
                offset += size;
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Semicolon)?;
        }
        self.expect(Token::BraceClose)?;
        // The struct size includes trailing padding to its own alignment.
        layout.size = offset.div_ceil(alignment_max) * alignment_max;
        layout.align = alignment_max as u8;
        Ok(layout)
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
        let mut max_size: u16 = 0;
        let mut max_align: u16 = 1;
        while *self.peek() != Token::BraceClose {
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
                let inner_align = (inner.align as u16).max(1);
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
                    layout.fields.insert(
                        name,
                        StructField {
                            member_type: Type::Struct {
                                size: inner_size,
                                align: inner_align as u8,
                            },
                            offset: 0,
                            struct_tag: Some(variant_tag),
                            array_element: None,
                            array_bytes: None,
                            bit_field: None,
                        },
                    );
                } else {
                    for (field_name, field) in &inner.fields {
                        layout.fields.insert(
                            field_name.clone(),
                            StructField {
                                member_type: field.member_type,
                                offset: field.offset,
                                struct_tag: field.struct_tag.clone(),
                                array_element: field.array_element,
                                array_bytes: field.array_bytes,
                                bit_field: field.bit_field,
                            },
                        );
                    }
                }
                max_size = max_size.max(inner_size);
                max_align = max_align.max(inner_align);
                self.expect(Token::Semicolon)?;
                continue;
            }
            let field_type = self.parse_type()?;
            // An array-typedef (or row-pointer-typedef) union member would lay out at
            // the decayed pointer's size (4) instead of the array's — defer.
            if self.last_array_typedef.take().is_some() {
                return Err(Diagnostic::error(
                    "an array-typedef union member is not supported yet (roadmap)",
                ));
            }
            let struct_tag = self.last_struct_tag.take();
            let attr_align = self.skip_attributes()?;
            let name = self.parse_identifier()?;
            // Bit-fields and multiple declarators in a union are uncommon and defer.
            if matches!(self.peek(), Token::Colon | Token::Comma) {
                return Err(Diagnostic::error(
                    "an irregular union member shape is not supported yet (roadmap)",
                ));
            }
            // An array member occupies the product of its dimensions; it still
            // starts at offset 0, so it only widens the union.
            let mut array_element = None;
            let mut is_array = false;
            let mut size = type_size(field_type);
            if *self.peek() == Token::BracketOpen {
                is_array = true;
                array_element = Some(pointee_of(field_type)?);
                let mut total: u16 = 1;
                while *self.peek() == Token::BracketOpen {
                    self.advance();
                    total = total.saturating_mul(self.parse_integer_constant()? as u16);
                    self.expect(Token::BracketClose)?;
                }
                size = total * type_size(field_type);
            }
            let align = type_alignment(field_type)
                .max(1)
                .max(attr_align.unwrap_or(1));
            layout.fields.insert(
                name,
                StructField {
                    member_type: field_type,
                    offset: 0,
                    struct_tag,
                    array_element,
                    array_bytes: is_array.then_some(size),
                    bit_field: None,
                },
            );
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
