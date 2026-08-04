//! Entry scheduling for floating parameters retained across calls.
//!
//! Home selection and copy scheduling are deliberately separate. Saved FPRs
//! are planned from f31 downward in every measured generation, while the 2.4.x
//! scheduler reverses those independent entry copies and 2.3.3 does not.

use mwcc_machine_code::Instruction;
use mwcc_versions::SavedFloatParameterCopyOrder;

/// Append `(destination, incoming)` copies. `copies` is in descending planned
/// saved-home order because structured liveness discovers parameters from the
/// end of the source parameter list.
pub(super) fn emit(
    instructions: &mut Vec<Instruction>,
    copies: &[(u8, u8)],
    order: SavedFloatParameterCopyOrder,
) {
    let mut append = |&(destination, incoming): &(u8, u8)| {
        instructions.push(Instruction::FloatMove {
            d: destination,
            b: incoming,
        });
    };
    match order {
        SavedFloatParameterCopyOrder::AscendingSavedHome => {
            copies.iter().rev().for_each(&mut append)
        }
        SavedFloatParameterCopyOrder::DescendingSavedHome => copies.iter().for_each(append),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainline_issues_entry_copies_from_the_lowest_saved_home() {
        let mut instructions = Vec::new();
        emit(
            &mut instructions,
            &[(31, 4), (30, 3), (29, 2)],
            SavedFloatParameterCopyOrder::AscendingSavedHome,
        );
        assert_eq!(
            instructions,
            [
                Instruction::FloatMove { d: 29, b: 2 },
                Instruction::FloatMove { d: 30, b: 3 },
                Instruction::FloatMove { d: 31, b: 4 },
            ]
        );
    }

    #[test]
    fn legacy_issues_entry_copies_from_the_highest_saved_home() {
        let mut instructions = Vec::new();
        emit(
            &mut instructions,
            &[(31, 4), (30, 3), (29, 2)],
            SavedFloatParameterCopyOrder::DescendingSavedHome,
        );
        assert_eq!(
            instructions,
            [
                Instruction::FloatMove { d: 31, b: 4 },
                Instruction::FloatMove { d: 30, b: 3 },
                Instruction::FloatMove { d: 29, b: 2 },
            ]
        );
    }
}
