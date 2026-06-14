//! Pipeline: syntax trees -> machine code.
//!
//! Instruction selection and register assignment for the supported C subset,
//! reproducing mwcceppc's output byte-for-byte. `lib.rs` only wires the theme
//! modules together and exposes the entry point; the work lives in them.

use mwcc_core::Compilation;
use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Function, GlobalDeclaration};
use mwcc_versions::CompilerBuild;
use std::collections::{HashMap, HashSet};

mod analysis;
mod generator;
mod operands;
mod body;
mod expressions;
mod arithmetic;
mod division;
mod comparisons;
mod control_flow;
mod narrow;
mod casts;
mod placement;
mod floats;

use generator::Generator;

/// Lower a parsed function to machine code for the given compiler build.
pub fn lower_function(function: &Function, globals: &[GlobalDeclaration], build: CompilerBuild) -> Compilation<MachineFunction> {
    let mut generator = Generator {
        output: MachineFunction::new(function.name.clone()),
        locations: HashMap::new(),
        globals: globals.iter().map(|global| (global.name.clone(), global.declared_type)).collect(),
        reserved: HashSet::new(),
        frame_size: 0,
        build,
        non_leaf: false,
    };
    generator.assign_parameters(function)?;
    generator.evaluate_body(function)?;
    Ok(generator.output)
}
