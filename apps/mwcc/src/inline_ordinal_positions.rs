//! Source-positioned accounting for dropped inline definitions.
//!
//! The frontend observes every inline definition in one translation unit, but
//! a definition appearing later in the file cannot renumber constants already
//! owned by an earlier function. This pass distributes cumulative parser
//! samples over lowered functions before any deferred emission reordering.

use mwcc_machine_code::MachineFunction;
use std::collections::HashMap;

pub(crate) fn distribute(
    functions: &mut [MachineFunction],
    cumulative_before_function: &HashMap<String, usize>,
    total: usize,
) -> u32 {
    if functions.is_empty() || total == 0 {
        return 0;
    }
    if cumulative_before_function.is_empty() {
        return total as u32;
    }

    // Work already completed before every source function belongs to the unit's
    // emitted head, not permanently to the first source body. Return that common
    // prefix so the caller can attach it after deferred/pragma emission ordering.
    let leading = functions
        .iter()
        .find_map(|function| cumulative_before_function.get(&function.name).copied())
        .unwrap_or(0);
    let mut accounted = leading;
    let mut last_source_function = None;
    for (index, function) in functions.iter_mut().enumerate() {
        let Some(&cumulative) = cumulative_before_function.get(&function.name) else {
            continue;
        };
        debug_assert!(cumulative >= accounted);
        function.anonymous_label_bump += cumulative.saturating_sub(accounted) as u32;
        accounted = accounted.max(cumulative);
        last_source_function = Some(index);
    }

    // Definitions after the last emitted source function still advance the
    // unit counter, but only after that function's constants have numbered.
    let trailing = total.saturating_sub(accounted) as u32;
    if trailing != 0 {
        let index = last_source_function.unwrap_or(0);
        functions[index].post_constant_label_bump += trailing;
    }
    leading as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(name: &str) -> MachineFunction {
        MachineFunction::new(name.to_string())
    }

    #[test]
    fn later_inline_work_stays_with_the_later_function() {
        let mut functions = vec![function("first"), function("second"), function("third")];
        let cumulative = HashMap::from([
            ("first".to_string(), 180),
            ("second".to_string(), 180),
            ("third".to_string(), 187),
        ]);

        let leading = distribute(&mut functions, &cumulative, 187);

        assert_eq!(leading, 180);
        assert_eq!(functions[0].anonymous_label_bump, 0);
        assert_eq!(functions[1].anonymous_label_bump, 0);
        assert_eq!(functions[2].anonymous_label_bump, 7);
    }

    #[test]
    fn trailing_inline_work_follows_the_last_function_constants() {
        let mut functions = vec![function("only")];
        let cumulative = HashMap::from([("only".to_string(), 3)]);

        let leading = distribute(&mut functions, &cumulative, 8);

        assert_eq!(leading, 3);
        assert_eq!(functions[0].anonymous_label_bump, 0);
        assert_eq!(functions[0].post_constant_label_bump, 5);
    }
}
