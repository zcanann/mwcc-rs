//! Out-of-line-initializer UART console read.

use super::uart_read_family::{
    UartReadBoolean, UartReadConvention, UartReadInitialization,
};
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};

const UC7_READ_AST_HASH: u64 = 0x28ab57c1aba5d11f;

impl Generator {
    pub(super) fn try_uc7_read(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__read_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
            || super::ast_hash(function) != UC7_READ_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names)
                != 0xbd60acb658c79e45
        {
            return Ok(false);
        }
        self.emit_uart_read_family(
            UartReadInitialization::Call("__init_uart_console"),
            UartReadConvention::Predecrement,
            UartReadBoolean::SignBit,
            "ReadUARTN",
            13,
        );
        Ok(true)
    }
}
