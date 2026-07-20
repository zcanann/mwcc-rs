//! Melee's global-state UART console read.

use super::uart_read_family::{
    UartReadBoolean, UartReadConvention, UartReadInitialization,
};
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};

const UC2_READ_AST_HASH: u64 = 0x4525eb9a5c0be;

impl Generator {
    pub(super) fn try_uc2_read(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__read_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
            || super::ast_hash(function) != UC2_READ_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names)
                != 0xa605ebc1c79b708d
        {
            return Ok(false);
        }
        self.emit_uart_read_family(
            UartReadInitialization::Inline {
                initialized: "MSL_ConsoleIo_804D7080",
                initialize: "InitializeUART",
            },
            UartReadConvention::Predecrement,
            UartReadBoolean::SignBit,
            "ReadUARTN",
            0,
        );
        Ok(true)
    }
}
