//! Read-only images retained after automatic storage itself is optimized away.
//!
//! The 2.x optimizer separates constant array-image creation from frame
//! liveness. An unused initialized automatic therefore leaves no instructions
//! or frame slot, but its anonymous `.rodata` object survives. Keeping that
//! object creation here, after executable lowering, prevents every specialized
//! body owner from having to reproduce the same compiler artifact.

use mwcc_machine_code::{AnonymousRodata, MachineFunction};
use mwcc_syntax_trees::{Function, LocalDeclaration, Type};
use mwcc_versions::{Behavior, FunctionOrdinalAccountingStyle};

pub(super) fn retain_unused_array_images(
    function: &Function,
    output: &mut MachineFunction,
    behavior: Behavior,
) {
    if behavior.function_ordinal_accounting_style != FunctionOrdinalAccountingStyle::Mainline {
        return;
    }

    let source_images: Vec<_> = function
        .locals
        .iter()
        .filter(|local| {
            !local.is_static && local.array_length.is_some() && local.data_bytes.is_some()
        })
        .map(materialize_image)
        .collect();
    if output.anonymous_rodata.len() >= source_images.len()
        && output
            .anonymous_rodata
            .iter()
            .zip(&source_images)
            .all(|(attached, source)| attached.bytes == *source)
    {
        // A structured copy transaction already attached the complete source
        // image run. Some of those arrays may prove dead in executable code,
        // but their images are already represented in declaration order.
        return;
    }

    let mut retained = function.locals.iter().filter(|local| {
        !local.is_static
            && local.array_length.is_some()
            && local.data_bytes.is_some()
            && !crate::analysis::function_uses_name(function, &local.name)
    });
    let Some(first) = retained.next() else {
        return;
    };

    output.anonymous_rodata.push(AnonymousRodata {
        bytes: materialize_image(first),
        // The first automatic image occupies the function's static-local slot;
        // subsequent images continue from that ordinal.
        static_slot_prefix_bump: Some(output.object_anonymous_bump()),
        anonymous_offset: 0,
    });
    output
        .anonymous_rodata
        .extend(retained.map(|local| AnonymousRodata {
            bytes: materialize_image(local),
            static_slot_prefix_bump: None,
            anonymous_offset: 0,
        }));
}

fn materialize_image(local: &LocalDeclaration) -> Vec<u8> {
    let explicit = local
        .data_bytes
        .as_ref()
        .expect("retained initialized array has a byte image");
    let element_size = match local.declared_type {
        Type::Struct { size, .. } => usize::try_from(size).unwrap_or(usize::MAX),
        other => usize::from(other.width()).div_ceil(8),
    };
    let size = element_size.saturating_mul(usize::from(
        local
            .array_length
            .expect("retained initialized array has a declared length"),
    ));
    let mut image = vec![0; size.max(explicit.len())];
    image[..explicit.len()].copy_from_slice(explicit);
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::Instruction;
    use mwcc_syntax_trees::{LocalDeclaration, Type};

    fn local(name: &str, size: u16) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Char,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(size),
            is_static: false,
            data_bytes: Some(vec![0; usize::from(size)]),
            data_relocations: Vec::new(),
            is_const: true,
            row_bytes: None,
        }
    }

    #[test]
    fn retains_mainline_dead_mutable_arrays_at_the_static_local_ordinal() {
        let mut first = local("first", 12);
        first.is_const = false;
        let mut second = local("second", 40);
        second.is_const = false;
        let function = Function {
            return_type: Type::Void,
            name: "dead_arrays".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![first, second],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let mut output = MachineFunction::new("dead_arrays");
        output.instructions.push(Instruction::BranchToLinkRegister);

        retain_unused_array_images(
            &function,
            &mut output,
            Behavior::resolve(&mwcc_versions::CompilerConfig {
                build: mwcc_versions::GC_2_0P1,
                flags: mwcc_versions::Flags::default(),
            }),
        );

        assert_eq!(output.anonymous_rodata.len(), 2);
        assert_eq!(
            output.anonymous_rodata[0].static_slot_prefix_bump,
            Some(0)
        );
        assert_eq!(output.anonymous_rodata[0].anonymous_offset, 0);
        assert_eq!(output.anonymous_rodata[0].bytes.len(), 12);
        assert_eq!(output.anonymous_rodata[1].anonymous_offset, 0);
        assert_eq!(output.anonymous_rodata[1].bytes.len(), 40);
    }

    #[test]
    fn does_not_duplicate_dead_images_already_attached_by_a_copy_transaction() {
        let mut first = local("first", 12);
        first.is_const = false;
        let mut second = local("second", 40);
        second.is_const = false;
        let function = Function {
            return_type: Type::Void,
            name: "partially_live_arrays".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![first, second],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let mut output = MachineFunction::new("partially_live_arrays");
        for bytes in [vec![0; 12], vec![0; 40]] {
            output.anonymous_rodata.push(AnonymousRodata {
                bytes,
                static_slot_prefix_bump: None,
                anonymous_offset: 0,
            });
        }

        retain_unused_array_images(
            &function,
            &mut output,
            Behavior::resolve(&mwcc_versions::CompilerConfig {
                build: mwcc_versions::GC_2_0P1,
                flags: mwcc_versions::Flags::default(),
            }),
        );

        assert_eq!(output.anonymous_rodata.len(), 2);
    }
}
