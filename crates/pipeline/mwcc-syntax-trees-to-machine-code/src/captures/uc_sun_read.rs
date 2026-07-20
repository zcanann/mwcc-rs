//! Sunshine's linkage-first member of the MSL UART console-read family.

use super::uart_read_family::{
    UartReadBoolean, UartReadConvention, UartReadInitialization,
};
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};

const UC_SUN_READ_AST_HASH: u64 = 0xb3d8762bd4ff94e1;

impl Generator {
    pub(super) fn try_uc_sun_read(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__read_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
            || !self.behavior.deferred_inlining
            || super::ast_hash(function) != UC_SUN_READ_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names)
                != 0xbd60acb658c79e45
        {
            return Ok(false);
        }

        self.output.static_locals = vec![mwcc_machine_code::StaticLocal {
            name: "initialized".to_string(),
            initial_bytes: None,
            size: 4,
            alignment: 4,
            is_const: false,
            relocations: Vec::new(),
        }];
        self.output.static_local_adjust = 14;
        self.output.symbol_order = vec!["InitializeUART".to_string(), "ReadUARTN".to_string()];
        self.emit_uart_read_family(
            UartReadInitialization::Inline {
                initialized: "initialized",
                initialize: "InitializeUART",
            },
            UartReadConvention::LinkageFirst,
            UartReadBoolean::BranchAndNarrow,
            "ReadUARTN",
            13,
        );
        Ok(true)
    }
}
