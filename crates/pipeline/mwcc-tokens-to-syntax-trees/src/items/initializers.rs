//! Global initializer and static-data parsing: scalar/aggregate/pointer
//! initializers, struct and struct-array constant images, and the struct-field
//! layout helpers that fill them (with relocations). Part of the `items` module.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter, Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type};
use mwcc_tokens::Token;
use crate::parser::{Parser, StructField, StructLayout};
use super::*;

impl Parser {
    /// Parse a global's constant initializer: a scalar `<const>` (one element) or
    /// an aggregate `{ <const>, ... }` (several, with an optional trailing comma).
    /// A pointer global's initializer: a single address (`int *p = &g;`) or a brace
    /// list of them (`int *t[] = {&a, &b};`), each element a target symbol, string,
    /// or null.
    pub(crate) fn parse_address_initializer(&mut self) -> Compilation<Vec<PointerElement>> {
        if self.eat_keyword(Token::BraceOpen) {
            let mut elements = Vec::new();
            while *self.peek() != Token::BraceClose {
                elements.push(self.parse_pointer_init_element()?);
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::BraceClose)?;
            Ok(elements)
        } else {
            Ok(vec![self.parse_pointer_init_element()?])
        }
    }

    /// A struct `tag`'s fields in layout (offset) order, but only when every field is
    /// exactly 4 bytes (a pointer, int, or float) and at least one is a pointer — the
    /// shape of a `{ "name", id }` string-table entry. Returns `None` otherwise, so
    /// the caller falls through to the scalar/defer paths (sub-word or 8-byte fields
    /// break the flat 4-byte-slot model).
    pub(crate) fn struct_pointer_table_fields(&self, tag: &str) -> Option<Vec<Type>> {
        let layout = self.structs.get(tag)?;
        let mut fields: Vec<&crate::parser::StructField> = layout.fields.values().collect();
        fields.sort_by_key(|field| field.offset);
        let types: Vec<Type> = fields.iter().map(|field| field.member_type).collect();
        let all_word = types.iter().all(|field_type| field_type.width() == 32);
        let any_pointer = types.iter().any(|field_type| matches!(field_type, Type::Pointer(_) | Type::StructPointer { .. }));
        (all_word && any_pointer).then_some(types)
    }

    /// Parse `{ { field0, field1, … }, … }` for an array of word-field structs: each
    /// element's fields are flattened, in order, to a `PointerElement` sequence — a
    /// pointer field to `Str`/`Symbol`/`Null`, a scalar field to `Scalar`. The shape
    /// of a `{ "path", id }` overlay/string table.
    pub(crate) fn parse_struct_pointer_table(&mut self, field_types: &[Type]) -> Compilation<Vec<PointerElement>> {
        self.expect(Token::BraceOpen)?;
        let mut elements = Vec::new();
        while *self.peek() != Token::BraceClose {
            self.expect(Token::BraceOpen)?;
            for (index, field_type) in field_types.iter().enumerate() {
                if matches!(field_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                    let element = self.parse_pointer_init_element()?;
                    // A string element pools an anonymous `@N` object, whose NUMBER in a
                    // real translation unit is offset by phantom `@N` mwcc consumes while
                    // processing preceding (header inline) functions — not modeled yet, so
                    // defer string tables. `&symbol`/`&global`/`0`/scalar tables have no
                    // `@N` and stay byte-exact.
                    if matches!(element, PointerElement::Str(_)) {
                        return Err(Diagnostic::error("a struct-table with string literals needs the anonymous @N base (roadmap)"));
                    }
                    elements.push(element);
                } else {
                    elements.push(PointerElement::Scalar(self.parse_integer_constant()?));
                }
                if index + 1 < field_types.len() {
                    self.expect(Token::Comma)?;
                }
            }
            self.eat_keyword(Token::Comma); // optional trailing comma inside the element
            self.expect(Token::BraceClose)?;
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(elements)
    }

    /// One element of a pointer global's address initializer: a string literal
    /// (pooled), `&name` or a bare `name` (a function pointer) is that symbol; `0` is
    /// a null pointer. `&a[i]`, `&s.f`, casts, and arithmetic defer (they need an
    /// addend not yet modeled).
    pub(crate) fn parse_pointer_init_element(&mut self) -> Compilation<PointerElement> {
        // A cast is transparent for an address: `(SomeType *)&x` is just `&x`. Skip
        // the parenthesised type and parse the operand after it.
        if *self.peek() == Token::ParenOpen && self.token_starts_type(self.peek_at(1)) {
            self.advance();
            let mut depth = 1;
            while depth > 0 {
                match self.advance() {
                    Token::ParenOpen => depth += 1,
                    Token::ParenClose => depth -= 1,
                    Token::EndOfFile => return Err(Diagnostic::error("unterminated cast in a pointer initializer")),
                    _ => {}
                }
            }
            return self.parse_pointer_init_element();
        }
        // A grouping paren that is not a cast: `((void *)0)` (the common `NULL` macro
        // expansion) or `(&x)`. Parse the inner element and consume the closing paren.
        if *self.peek() == Token::ParenOpen {
            self.advance();
            let element = self.parse_pointer_init_element()?;
            self.expect(Token::ParenClose)?;
            return Ok(element);
        }
        if let Token::StringLiteral(bytes) = self.peek() {
            let bytes = bytes.clone();
            self.advance();
            return Ok(PointerElement::Str(bytes));
        }
        if *self.peek() == Token::Ampersand {
            self.advance();
            let name = self.parse_identifier()?;
            // `&g[0]` addresses the first element — the same relocation as `&g`
            // (ansi_files' `FILE* p = &__files[0];`). A nonzero index needs an
            // addend model on PointerElement; defer until measured.
            if *self.peek() == Token::BracketOpen && self.peek_at(1) == &Token::IntegerLiteral(0) && self.peek_at(2) == &Token::BracketClose {
                self.advance();
                self.advance();
                self.advance();
            }
            if matches!(self.peek(), Token::BracketOpen | Token::Dot | Token::Arrow) {
                return Err(Diagnostic::error("a pointer initializer with an offset is not supported yet (roadmap)"));
            }
            return Ok(PointerElement::Symbol(name));
        }
        if matches!(self.peek(), Token::IntegerLiteral(0)) {
            self.advance();
            return Ok(PointerElement::Null);
        }
        if let Token::Identifier(name) = self.peek() {
            let name = name.clone();
            self.advance();
            return Ok(PointerElement::Symbol(name));
        }
        Err(Diagnostic::error("a pointer global initializer must be a string, &symbol, a symbol, or 0 (roadmap)"))
    }

    pub(crate) fn parse_constant_initializer(&mut self, element_type: Type) -> Compilation<Vec<i64>> {
        // A string literal initializes a `char` array with its bytes plus a NUL
        // terminator (the store truncates if the array is shorter). A string for a
        // non-char type is a char-pointer initializer that needs a data relocation —
        // deferred. (A bare string, no braces; an array of strings would brace it.)
        if let Token::StringLiteral(bytes) = self.peek() {
            if !matches!(element_type, Type::Char | Type::UnsignedChar) {
                return Err(Diagnostic::error("a string initializer is only supported for a char array yet (roadmap)"));
            }
            let mut values: Vec<i64> = bytes.iter().map(|&byte| byte as i64).collect();
            self.advance();
            values.push(0);
            return Ok(values);
        }
        if self.eat_keyword(Token::BraceOpen) {
            let mut values = Vec::new();
            while *self.peek() != Token::BraceClose {
                // A nested brace (`{{1,2},{3,4}}` for a multi-dimensional array)
                // flattens in row-major order into the same flat element list.
                if *self.peek() == Token::BraceOpen {
                    values.extend(self.parse_constant_initializer(element_type)?);
                } else {
                    values.push(self.parse_scalar_constant(element_type)?);
                }
                if !self.eat_keyword(Token::Comma) {
                    break;
                }
            }
            self.expect(Token::BraceClose)?;
            Ok(values)
        } else {
            Ok(vec![self.parse_scalar_constant(element_type)?])
        }
    }

    /// One scalar element of a global initializer, encoded to the raw bits the
    /// object stores for `element_type`. An integer element is a full constant
    /// *expression* (`((dir)+(file))`, `1 << 3`, `(u8)0xFF`), folded to its value;
    /// the store later truncates to the element width. A `float`/`double` element
    /// is a single literal (or integer, converted) as its IEEE-754 bit pattern.
    pub(crate) fn parse_scalar_constant(&mut self, element_type: Type) -> Compilation<i64> {
        match element_type {
            Type::Float | Type::Double => {
                // A float/double initializer is a constant EXPRESSION evaluated in double
                // (`M_PI / 180`, `1.0f / 3.0f`, a bare literal, `-1.5f`), then narrowed to
                // the element width. Parse and fold it rather than accepting only a single
                // literal.
                let expression = self.expression()?;
                let value = crate::expressions::fold_constant_float(&expression)?;
                Ok(if element_type == Type::Float { (value as f32).to_bits() as i64 } else { value.to_bits() as i64 })
            }
            _ => {
                let expression = self.expression()?;
                crate::expressions::fold_constant_expression(&expression)
            }
        }
    }

    /// Parse one struct value `{ f0, f1, ... }` for the layout `tag`, folding each
    /// field with its own type (the flat `parse_constant_initializer` cannot, since a
    /// struct mixes field types — notably `float`). Fields are taken in offset order;
    /// a nested struct field recurses. Every field must be word-width (4 bytes) so the
    /// flat value list lays out contiguously with no padding — a sub-word/`double`
    /// field, or an array field, defers (those need a width-aware data emitter).
    pub(crate) fn parse_one_struct(&mut self, tag: &str) -> Compilation<Vec<u8>> {
        let mut relocations = Vec::new();
        let bytes = self.parse_one_struct_relocated(tag, 0, &mut relocations)?;
        if !relocations.is_empty() {
            return Err(Diagnostic::error("an address element in this initializer position is not supported yet (roadmap)"));
        }
        Ok(bytes)
    }

    /// One BRACED struct value for layout `tag`, written into a fresh byte image.
    /// `base_offset` positions the value inside the enclosing data object so
    /// ADDRESS elements (`__read_console`, `(char*)&(&__files[0])->field`) record
    /// `(object offset, target, addend)` relocations. Values map to fields with
    /// C89 FLAT DESCENT: an unbraced nested struct/array consumes values for its
    /// own members from the same list; bit-fields pack into their storage units.
    pub(crate) fn parse_one_struct_relocated(&mut self, tag: &str, base_offset: u32, relocations: &mut Vec<(u32, String, i32)>) -> Compilation<Vec<u8>> {
        let struct_size = {
            let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
            layout.size
        };
        let mut bytes = vec![0u8; struct_size as usize];
        self.expect(Token::BraceOpen)?;
        self.fill_struct_fields(tag, &mut bytes, 0, base_offset, relocations)?;
        self.eat_keyword(Token::Comma);
        self.expect(Token::BraceClose)?;
        Ok(bytes)
    }

    /// The declaration-ordered field list for `tag`: (offset, type, nested tag,
    /// element pointee, total array bytes, bit-field). Bit-fields sharing a
    /// storage unit order by their bit offset.
    pub(crate) fn ordered_struct_fields(&self, tag: &str) -> Compilation<Vec<(u16, Type, Option<String>, Option<Pointee>, Option<u16>, Option<(u8, u8)>)>> {
        let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
        let mut ordered: Vec<_> = layout
            .fields
            .values()
            .map(|field| (field.offset, field.member_type, field.struct_tag.clone(), field.array_element, field.array_bytes, field.bit_field))
            .collect();
        ordered.sort_by_key(|(offset, _, _, _, _, bit_field)| (*offset, bit_field.map_or(0, |(bit, _)| bit)));
        Ok(ordered)
    }

    /// Fill `tag`'s fields from the value list at the cursor into `image`
    /// starting at `struct_base`, stopping early at `}` (remaining fields stay
    /// zero). Consumes one trailing comma after each value; consumes NO braces
    /// itself (the caller owns the enclosing pair; a BRACED sub-aggregate is
    /// detected per field).
    pub(crate) fn fill_struct_fields(&mut self, tag: &str, image: &mut [u8], struct_base: usize, absolute_base: u32, relocations: &mut Vec<(u32, String, i32)>) -> Compilation<()> {
        let fields = self.ordered_struct_fields(tag)?;
        for (offset, member_type, nested_tag, array_element, array_bytes, bit_field) in fields {
            if *self.peek() == Token::BraceClose {
                break;
            }
            let field_base = struct_base + offset as usize;
            let absolute_field = absolute_base + offset as u32;
            if let Some((bit_offset, width)) = bit_field {
                let value = self.parse_scalar_constant(Type::UnsignedInt)?;
                pack_bit_field(image, field_base, bit_offset, width, value as u64);
            } else if let (Some(nested), Type::Struct { .. }) = (nested_tag.as_ref(), member_type) {
                // Only a struct VALUE field descends — a struct POINTER carries
                // the tag too (for member resolution) but is a 4-byte address.
                let nested = nested.clone();
                if *self.peek() == Token::BraceOpen {
                    self.advance();
                    self.fill_struct_fields(&nested, image, field_base, absolute_field, relocations)?;
                    self.eat_keyword(Token::Comma);
                    self.expect(Token::BraceClose)?;
                } else {
                    // FLAT descent: the nested struct's fields consume from this
                    // list — including each value's trailing comma, so the outer
                    // loop must NOT eat another separator.
                    self.fill_struct_fields(&nested, image, field_base, absolute_field, relocations)?;
                    continue;
                }
            } else if let Some(total) = array_bytes {
                let element_width = array_element.map_or(4, |element| element.size() as usize).max(1);
                let count = (total as usize / element_width).max(1);
                let element_type = array_element.map_or(Type::UnsignedInt, |element| element.element());
                let braced = self.eat_keyword(Token::BraceOpen);
                for index in 0..count {
                    if *self.peek() == Token::BraceClose {
                        break;
                    }
                    let value = self.parse_scalar_constant(element_type)?;
                    let at = field_base + index * element_width;
                    let encoded = (value as u64).to_be_bytes();
                    image[at..at + element_width].copy_from_slice(&encoded[8 - element_width..]);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                if braced {
                    self.expect(Token::BraceClose)?;
                    self.eat_keyword(Token::Comma);
                }
                continue; // the comma after an unbraced array's last element is consumed above
            } else if matches!(member_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                // A pointer field initialized with a STRING LITERAL pools the
                // string as an anonymous `@N` `.sdata` object (locale's lconv:
                // `{".", "", ...}` — main.rs assigns the numbers in first-
                // appearance order, deduplicated under `-str reuse`). The
                // marker prefix routes it through the symbol-relocation tuple.
                if let Token::StringLiteral(string_bytes) = self.peek() {
                    let marker = format!("\u{1}{}", String::from_utf8_lossy(string_bytes));
                    self.advance();
                    relocations.push((absolute_field, marker, 0));
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // A pointer field: a relocated address (`__read_console`, a cast
                // `&member` chain into a struct array) or a constant (NULL).
                if let Some((target, addend)) = self.parse_address_element(tag)? {
                    relocations.push((absolute_field, target, addend));
                    // the image bytes stay zero — the relocation fills them at link time
                } else {
                    let value = self.parse_scalar_constant(Type::UnsignedInt)?;
                    let encoded = (value as u64).to_be_bytes();
                    image[field_base..field_base + 4].copy_from_slice(&encoded[4..]);
                }
            } else {
                let value = self.parse_scalar_constant(member_type)?;
                let width = type_size(member_type) as usize;
                let encoded = (value as u64).to_be_bytes();
                image[field_base..field_base + width].copy_from_slice(&encoded[8 - width..]);
            }
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        Ok(())
    }

    /// Classify a pointer-field initializer element at the cursor. Returns
    /// `Some((symbol, addend))` for the relocated-address forms — a bare
    /// function/global name, or a (possibly cast) `&<lvalue>` chain resolving
    /// into a struct-array global (`(char*)&((&__files[0]))->field` — measured:
    /// ansi_files' self-referential FILE table). Returns `None` (cursor
    /// unmoved) when the element is an ordinary constant expression.
    pub(crate) fn parse_address_element(&mut self, tag: &str) -> Compilation<Option<(String, i32)>> {
        let start = self.position;
        // optional cast(s): `(char*)`, `(void*)` ... skip `( type * )` groups.
        loop {
            if *self.peek() == Token::ParenOpen && self.token_starts_type(self.peek_at(1)) {
                // find the matching close; a cast group is short — scan it
                let mut index = self.position + 1;
                let mut depth = 1;
                while depth > 0 {
                    match self.tokens.get(index) {
                        Some(Token::ParenOpen) => depth += 1,
                        Some(Token::ParenClose) => depth -= 1,
                        None => break,
                        _ => {}
                    }
                    index += 1;
                }
                // Only skip when a cast is followed by MORE expression (not `(void*)0`'s
                // constant fold — that path returns None below and re-parses).
                if matches!(self.tokens.get(index), Some(Token::Ampersand) | Some(Token::Identifier(_))) {
                    self.position = index;
                    continue;
                }
            }
            break;
        }
        // Bare `name` followed by `,` or `}` — a function/global address.
        if let (Token::Identifier(name), Some(Token::Comma) | Some(Token::BraceClose)) = (self.peek(), self.tokens.get(self.position + 1)) {
            if !self.enum_constants.contains_key(name) {
                let name = name.clone();
                self.advance();
                return Ok(Some((name, 0)));
            }
        }
        // `name + K` — an address with a byte addend into a known global array
        // (pikmin's locale: `stringBase0 + 2` indexes the packed string table).
        if let (Token::Identifier(name), Some(Token::Plus), Some(Token::IntegerLiteral(addend))) =
            (self.peek(), self.tokens.get(self.position + 1), self.tokens.get(self.position + 2))
        {
            if self.global_sizes.contains_key(name)
                && matches!(self.tokens.get(self.position + 3), Some(Token::Comma) | Some(Token::BraceClose))
            {
                let name = name.clone();
                let addend = *addend as i32;
                self.position += 3;
                return Ok(Some((name, addend)));
            }
        }
        // `&name` — an explicit address-of a function/global (mp4's
        // `&__read_console`); same relocation as the bare-name form.
        if *self.peek() == Token::Ampersand {
            if let Some(Token::Identifier(name)) = self.tokens.get(self.position + 1) {
                if matches!(self.tokens.get(self.position + 2), Some(Token::Comma) | Some(Token::BraceClose)) {
                    let name = name.clone();
                    self.position += 2;
                    return Ok(Some((name, 0)));
                }
            }
        }
        // `&global.member[.member…]` — a (possibly nested) member address in a
        // STRUCT-typed global (`&__files._stdout`, `&__files._stdin.char_buffer`);
        // each step's offset comes from the successive layouts.
        if *self.peek() == Token::Ampersand {
            if let (Some(Token::Identifier(global)), Some(Token::Dot)) =
                (self.tokens.get(self.position + 1), self.tokens.get(self.position + 2))
            {
                if let Some(outer_tag) = self.global_structs.get(global) {
                    let global = global.clone();
                    let mut current_tag = outer_tag.clone();
                    let mut addend: i32 = 0;
                    let mut cursor = self.position + 2;
                    loop {
                        if self.tokens.get(cursor) != Some(&Token::Dot) {
                            break;
                        }
                        let Some(Token::Identifier(member)) = self.tokens.get(cursor + 1) else {
                            break;
                        };
                        let layout = self.structs.get(&current_tag).ok_or_else(|| Diagnostic::error(format!("struct '{current_tag}' is not declared")))?;
                        let Some(field) = layout.fields.get(member) else {
                            break;
                        };
                        addend += field.offset as i32;
                        let next_tag = field.struct_tag.clone();
                        cursor += 2;
                        if matches!(self.tokens.get(cursor), Some(Token::Comma) | Some(Token::BraceClose)) {
                            self.position = cursor;
                            return Ok(Some((global, addend)));
                        }
                        match next_tag {
                            Some(tag) => current_tag = tag,
                            None => break,
                        }
                    }
                }
            }
        }
        // `&global[i]` — the address of a whole array element (ansi_files'
        // mNextFile chain in the mp4/AC variants).
        if *self.peek() == Token::Ampersand {
            if let Some(Token::Identifier(global)) = self.tokens.get(self.position + 1) {
                if self.tokens.get(self.position + 2) == Some(&Token::BracketOpen) {
                    if let (Some(Token::IntegerLiteral(element)), Some(Token::BracketClose)) =
                        (self.tokens.get(self.position + 3), self.tokens.get(self.position + 4))
                    {
                        if matches!(self.tokens.get(self.position + 5), Some(Token::Comma) | Some(Token::BraceClose)) {
                            let global = global.clone();
                            let element = *element;
                            let layout_size = self
                                .structs
                                .get(tag)
                                .ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?
                                .size;
                            self.position += 5;
                            return Ok(Some((global, element as i32 * layout_size as i32)));
                        }
                    }
                }
            }
        }
        // `&( (&global[i]) )->field` — resolve through the CURRENT tag's layout.
        if *self.peek() == Token::Ampersand {
            let mut index = self.position + 1;
            while self.tokens.get(index) == Some(&Token::ParenOpen) {
                index += 1;
            }
            if self.tokens.get(index) == Some(&Token::Ampersand) {
                if let Some(Token::Identifier(global)) = self.tokens.get(index + 1) {
                    let global = global.clone();
                    if self.tokens.get(index + 2) == Some(&Token::BracketOpen) {
                        if let Some(Token::IntegerLiteral(element)) = self.tokens.get(index + 3) {
                            let element = *element;
                            let mut cursor = index + 4;
                            if self.tokens.get(cursor) == Some(&Token::BracketClose) {
                                cursor += 1;
                                while self.tokens.get(cursor) == Some(&Token::ParenClose) {
                                    cursor += 1;
                                }
                                if matches!(self.tokens.get(cursor), Some(Token::Arrow) | Some(Token::Dot)) {
                                    if let Some(Token::Identifier(field)) = self.tokens.get(cursor + 1) {
                                        let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
                                        let field_offset = layout
                                            .fields
                                            .get(field)
                                            .map(|entry| entry.offset)
                                            .ok_or_else(|| Diagnostic::error(format!("no member '{field}' in struct '{tag}'")))?;
                                        let addend = element as i32 * layout.size as i32 + field_offset as i32;
                                        self.position = cursor + 2;
                                        return Ok(Some((global, addend)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = start;
            return Err(Diagnostic::error("this address-of initializer shape is not supported yet (roadmap)"));
        }
        Ok(None)
    }

    /// Parse a `{ s0, s1, ... }` array of struct values for the layout `tag`, each
    /// element parsed by [`Self::parse_one_struct`] and concatenated (the array stride
    /// is the struct size, which each element's image already fills).
    pub(crate) fn parse_struct_array_initializer(&mut self, tag: &str, relocations: &mut Vec<(u32, String, i32)>) -> Compilation<Vec<u8>> {
        self.expect(Token::BraceOpen)?;
        let mut bytes = Vec::new();
        while *self.peek() != Token::BraceClose {
            let element = self.parse_one_struct_relocated(tag, bytes.len() as u32, relocations)?;
            bytes.extend(element);
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(bytes)
    }

    /// The struct-value [`Type`] for a known struct layout (size + alignment), or
    /// `None` for an opaque/undeclared struct (whose value cannot be laid out).
    /// Drives `struct S v;` value support.
    pub(crate) fn struct_value_type(&self, tag: &str) -> Option<Type> {
        self.structs
            .get(tag)
            .filter(|layout| layout.size > 0)
            .map(|layout| Type::Struct { size: layout.size, align: layout.align.max(1) })
    }
}
