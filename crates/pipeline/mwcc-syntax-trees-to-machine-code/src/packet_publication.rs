//! Semantic description of a guarded packet read and bitfield publication.
//!
//! The automatic inliner and physical schedules share this proof. It carries
//! source names and masks, without assigning registers or frame slots.

use mwcc_syntax_trees::{
    BinaryOperator as B, Expression as E, Function, LocalDeclaration, Statement as S, Type,
};
use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct Publication<'a> {
    pub index: u16,
    pub status: &'a str,
    pub read: &'a str,
    pub ready: u32,
    pub preserve: u32,
    pub tag_mask: u32,
    pub tag: u32,
    pub field_mask: u32,
    pub whole: &'a str,
    pub field: &'a str,
    pub flag: &'a str,
    pub flag_value: u32,
}

impl Publication<'_> {
    pub fn captures(&self, names: &HashSet<String>) -> bool {
        [self.status, self.read, self.whole, self.field, self.flag]
            .iter()
            .any(|name| names.contains(*name))
    }
}

pub(crate) struct Query<'a> {
    pub publication: Publication<'a>,
    pub acquire: &'a str,
    pub release: &'a str,
    pub token: &'a str,
}

fn plain(function: &Function) -> bool {
    function.parameters.is_empty()
        && function.guards.is_empty()
        && function.asm_body.is_none()
        && function.inline_asm_blocks.is_empty()
}

fn array(local: &LocalDeclaration) -> bool {
    local.declared_type == Type::UnsignedInt
        && local.array_length == Some(2)
        && local.initializer.is_none()
        && !local.is_static
        && !local.is_volatile
        && local.data_bytes.is_none()
        && local.data_relocations.is_empty()
        && local.attribute_alignment.is_none()
        && local.row_bytes.is_none()
}

pub(crate) fn helper(function: &Function) -> Option<Publication<'_>> {
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if !plain(function)
        || function.return_type != Type::Void
        || function.return_expression.is_some()
        || !array(local)
    {
        return None;
    }
    body(&function.statements, &local.name)
}

pub(crate) fn query(function: &Function) -> Option<Query<'_>> {
    if !plain(function)
        || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || function.locals.len() != 2
    {
        return None;
    }
    let packet = function.locals.iter().find(|local| array(local))?;
    let token = function
        .locals
        .iter()
        .find(|local| local.name != packet.name)?;
    if !matches!(token.declared_type, Type::Int | Type::UnsignedInt)
        || token.initializer.is_some()
        || token.array_length.is_some()
        || token.is_static
        || token.is_volatile
    {
        return None;
    }
    let [S::Store {
        target: E::Variable(clear),
        value,
    }, S::If {
        condition,
        then_body,
        else_body,
    }, S::Expression(E::Call {
        name: release,
        arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    if integer(value)? != 0
        || !else_body.is_empty()
        || !matches!(arguments.as_slice(), [E::Variable(name)] if name == &token.name)
    {
        return None;
    }
    let E::Binary {
        operator: B::Equal,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let E::Variable(tested) = left.as_ref() else {
        return None;
    };
    if integer(right)? != 0
        || !matches!(function.return_expression.as_ref(), Some(E::Variable(name)) if name == tested)
    {
        return None;
    }
    let [S::Assign {
        name,
        value: E::Call {
            name: acquire,
            arguments,
        },
    }, statements @ ..] = then_body.as_slice()
    else {
        return None;
    };
    if name != &token.name || !arguments.is_empty() {
        return None;
    }
    let publication = body(statements, &packet.name)?;
    if publication.field != tested
        || publication.flag != clear
        || publication.captures(&HashSet::from([token.name.clone(), packet.name.clone()]))
        || [acquire.as_str(), release.as_str()].contains(&token.name.as_str())
        || [acquire.as_str(), release.as_str()].contains(&packet.name.as_str())
    {
        return None;
    }
    Some(Query {
        publication,
        acquire,
        release,
        token: &token.name,
    })
}

fn body<'a>(statements: &'a [S], array: &'a str) -> Option<Publication<'a>> {
    let [status, S::If {
        condition,
        then_body,
        else_body,
    }] = statements
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let (ready_slot, ready) = masked(condition)?;
    let index = slot(ready_slot, array)?;
    let [read, S::Store { target, value }, S::If {
        condition:
            E::Binary {
                operator: B::Equal,
                left,
                right,
            },
        then_body: published,
        else_body,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() || slot(target, array)? != index {
        return None;
    }
    let value = match value {
        E::IndexedUpdateValue { value } => value.as_ref(),
        other => other,
    };
    let (preserved_slot, preserve) = masked(value)?;
    let (tag_slot, tag_mask) = masked(left)?;
    let tag = integer(right)?;
    let [S::Store {
        target: E::Variable(whole),
        value,
    }, S::Store {
        target: E::Variable(field),
        value: field_value,
    }, S::Store {
        target: E::Variable(flag),
        value: flag_value,
    }] = published.as_slice()
    else {
        return None;
    };
    let (field_slot, field_mask) = masked(field_value)?;
    if [preserved_slot, tag_slot, value, field_slot]
        .iter()
        .any(|value| slot(value, array) != Some(index))
        || [whole.as_str(), field.as_str(), flag.as_str()].contains(&array)
        || whole == field
        || whole == flag
        || field == flag
    {
        return None;
    }
    let status = read_call(status, array)?;
    let read = read_call(read, array)?;
    if [status, read].contains(&array) {
        return None;
    }
    Some(Publication {
        index,
        status,
        read,
        ready,
        preserve,
        tag_mask,
        tag,
        field_mask,
        whole,
        field,
        flag,
        flag_value: u32::try_from(integer(flag_value)?).ok()?,
    })
}

fn read_call<'a>(statement: &'a S, array: &str) -> Option<&'a str> {
    let S::Expression(E::Call { name, arguments }) = statement else {
        return None;
    };
    matches!(arguments.as_slice(), [E::Variable(name)] if name == array).then_some(name)
}

fn slot(value: &E, array: &str) -> Option<u16> {
    let E::Index { base, index } = value else {
        return None;
    };
    let index = integer(index)?;
    (matches!(base.as_ref(), E::Variable(name) if name == array) && index < 2)
        .then_some(index as u16)
}

fn masked(value: &E) -> Option<(&E, u32)> {
    let E::Binary {
        operator: B::BitAnd,
        left,
        right,
    } = value
    else {
        return None;
    };
    Some((left, integer(right)?))
}

// Restrict constants to trees with no runtime operands. Algebraic identities
// accepted by constant_value must not erase volatile reads or calls here.
fn integer(value: &E) -> Option<u32> {
    match value {
        E::IntegerLiteral(_) => {}
        E::Unary { operand, .. } | E::Cast { operand, .. } => {
            integer(operand)?;
        }
        E::Binary { left, right, .. } => {
            integer(left)?;
            integer(right)?;
        }
        _ => return None,
    }
    crate::analysis::constant_value(value).map(|value| value as u32)
}

#[cfg(test)]
mod tests;
