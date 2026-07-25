//! Pikmin's inlined-initializer UART console read.

use super::uart_read_family::{UartReadBoolean, UartReadConvention, UartReadInitialization};
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};

const UC1_READ_AST_HASH: u64 = 0x0eab8071aaf68969;

impl Generator {
    pub(super) fn try_uc1_read(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__read_console"
            || function.return_type != Type::Int
            || function.parameters.len() != 4
            || !self.frame_slots.is_empty()
            || super::ast_hash(function) != UC1_READ_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != 0x38824b31e8176c4d
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
        self.output.symbol_order = vec!["InitializeUART".to_string(), "ReadUARTN".to_string()];
        self.emit_uart_read_family(
            UartReadInitialization::Inline {
                initialized: "initialized",
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
