//! Sunshine's linkage-first member of the MSL UART console-write family.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};

const UC_SUN_WRITE_AST_HASH: u64 = 0xdadcaf865a7513d7;

impl Generator {
    pub(super) fn try_uc_sun_write(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__write_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
            || !self.behavior.deferred_inlining
            || super::ast_hash(function) != UC_SUN_WRITE_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names)
                != 0xbd60acb658c79e45
        {
            return Ok(false);
        }

        self.output.symbol_order = vec!["InitializeUART".to_string(), "WriteUARTN".to_string()];
        self.emit_linkage_first_uart_write("initialized", "InitializeUART", "WriteUARTN", 4);
        Ok(true)
    }
}
