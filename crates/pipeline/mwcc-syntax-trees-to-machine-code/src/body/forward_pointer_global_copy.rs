//! Forward a just-published pointer into an adjacent global copy.
//!
//! Build 163 normally reloads a global after storing it. Its scheduled pointer
//! publication chain is narrower: `source = value; destination = source;` keeps
//! the value in the store register and omits the intervening SDA load.

use super::*;
use mwcc_machine_code::RelocationTarget;

pub(super) fn pointer_type(value_type: Option<&Type>) -> bool {
    matches!(
        value_type,
        Some(Type::Pointer(_) | Type::StructPointer { .. })
    )
}

pub(super) fn sda_target(
    output: &mwcc_machine_code::MachineFunction,
    index: usize,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != index || relocation.kind != RelocationKind::EmbSda21 {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(target) => Some(target.as_str()),
            _ => None,
        }
    })
}

fn recognize(
    output: &mwcc_machine_code::MachineFunction,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<usize> {
    output
        .instructions
        .windows(3)
        .enumerate()
        .find_map(|(store, window)| {
            let [Instruction::StoreWord {
                s: published,
                a: 0,
                offset: 0,
            }, Instruction::LoadWord {
                d: reloaded,
                a: 0,
                offset: 0,
            }, Instruction::StoreWord {
                s: copied,
                a: 0,
                offset: 0,
            }] = window
            else {
                return None;
            };
            if published != reloaded || published != copied {
                return None;
            }
            let source = sda_target(output, store)?;
            let reload = sda_target(output, store + 1)?;
            let destination = sda_target(output, store + 2)?;
            (source == reload
                && source != destination
                && pointer_type(globals.get(source))
                && pointer_type(globals.get(destination)))
            .then_some(store + 1)
        })
}

impl Generator {
    pub(crate) fn forward_adjacent_pointer_global_copy(&mut self) {
        if self.behavior.stored_global_read_style
            != mwcc_versions::StoredGlobalReadStyle::ReloadAfterStore
        {
            return;
        }
        let Some(reload) = recognize(&self.output, &self.globals) else {
            return;
        };
        crate::remove_instruction_retargeting_to_next(self, reload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn sda(instruction_index: usize, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::External(target.into()),
        }
    }

    fn publication(
        source: &str,
        reload: &str,
        destination: &str,
    ) -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
            ],
            relocations: vec![sda(0, source), sda(1, reload), sda(2, destination)],
            ..Default::default()
        }
    }

    fn pointer_globals() -> std::collections::HashMap<String, Type> {
        [
            ("source".into(), Type::Pointer(Pointee::UnsignedInt)),
            ("destination".into(), Type::Pointer(Pointee::UnsignedInt)),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn recognizes_an_adjacent_pointer_publication_copy() {
        assert_eq!(
            recognize(
                &publication("source", "source", "destination"),
                &pointer_globals(),
            ),
            Some(1)
        );
    }

    #[test]
    fn rejects_a_reload_from_a_different_global() {
        assert_eq!(
            recognize(
                &publication("source", "destination", "destination"),
                &pointer_globals(),
            ),
            None
        );
    }

    #[test]
    fn rejects_scalar_global_copies() {
        let globals = [
            ("source".into(), Type::UnsignedInt),
            ("destination".into(), Type::UnsignedInt),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            recognize(&publication("source", "source", "destination"), &globals,),
            None
        );
    }
}
