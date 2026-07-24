//! Shared structural recovery for body-bearing variadic buffer frames.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct VariadicBufferFrame<'a> {
    pub(super) format_parameter: &'a str,
    pub(super) va_list: &'a str,
    pub(super) buffer: &'a str,
    pub(super) buffer_bytes: i16,
}

impl<'a> VariadicBufferFrame<'a> {
    /// Recover the common signature, two-local frame, and expanded `va_start`
    /// prefix. A specialized owner must still prove every remaining statement
    /// before it emits a complete function.
    pub(super) fn recognize(function: &'a Function) -> Option<(Self, &'a [Statement])> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return None;
        }
        let [format_parameter] = function.parameters.as_slice() else {
            return None;
        };
        if !matches!(
            format_parameter.parameter_type,
            Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
        ) {
            return None;
        }
        let [va_list, buffer] = function.locals.as_slice() else {
            return None;
        };
        if !matches!(va_list.declared_type, Type::Struct { size: 12, align: 4 })
            || va_list.array_length.is_some()
            || va_list.initializer.is_some()
            || va_list.is_static
            || va_list.data_bytes.is_some()
            || !matches!(buffer.declared_type, Type::Char | Type::UnsignedChar)
            || buffer.initializer.is_some()
            || buffer.is_static
            || buffer.data_bytes.is_some()
        {
            return None;
        }
        let [Statement::Expression(Expression::Comma { left, right }), remaining @ ..] =
            function.statements.as_slice()
        else {
            return None;
        };
        if !matches!(
            left.as_ref(),
            Expression::Cast {
                target_type: Type::Void,
                operand,
            } if matches!(
                operand.as_ref(),
                Expression::Variable(name) if name == &format_parameter.name
            )
        ) || !matches!(
            right.as_ref(),
            Expression::Call { name, arguments }
                if name == "__builtin_va_info"
                    && matches!(
                        arguments.as_slice(),
                        [Expression::AddressOf { operand }]
                            if matches!(
                                operand.as_ref(),
                                Expression::Variable(name) if name == &va_list.name
                            )
                    )
        ) {
            return None;
        }
        Some((
            Self {
                format_parameter: &format_parameter.name,
                va_list: &va_list.name,
                buffer: &buffer.name,
                buffer_bytes: i16::try_from(buffer.array_length?).ok()?,
            },
            remaining,
        ))
    }
}
