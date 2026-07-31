//! Floating-point home constraints recovered from decompilation local names.

#[allow(unused_imports)]
use super::*;

/// Decode the register-home convention used by decompilation sources. Names
/// outside the explicit `var_fN` / `temp_fN` forms carry no allocation policy.
pub(super) fn register(name: &str) -> Option<u8> {
    let suffix = name
        .strip_prefix("var_f")
        .or_else(|| name.strip_prefix("temp_f"))?;
    let register = suffix.parse::<u8>().ok()?;
    (14..=31).contains(&register).then_some(register)
}

/// Prefer an explicitly recovered home over the generic lifetime-group
/// placement. This remains a preference rather than a pin, so interference
/// and allocator constraints still take precedence.
pub(super) fn preference(local: &LocalDeclaration, fallback: u8) -> u8 {
    register(&local.name).unwrap_or(fallback)
}

/// The prologue must reserve every saved FPR named by the recovered source,
/// including a lower home whose lifetime no longer overlaps another local.
pub(super) fn saved_count(function: &Function) -> u8 {
    function
        .locals
        .iter()
        .filter(|local| matches!(local.declared_type, Type::Float | Type::Double))
        .filter_map(|local| register(&local.name))
        .map(|register| 32 - register)
        .max()
        .unwrap_or(0)
}

/// Reassert recovered homes after structured lowering has finished splitting
/// local lifetimes.  An assignment or semantic loop owner can replace the
/// virtual initially allocated for a declaration; the final live version still
/// represents the same recovered source local and therefore carries the same
/// allocation preference.
pub(super) fn apply_final_preferences(generator: &mut Generator, function: &Function) {
    for local in &function.locals {
        let Some(preferred) = register(&local.name) else {
            continue;
        };
        let Some(location) = generator.locations.get(&local.name) else {
            continue;
        };
        if location.class != ValueClass::Float {
            continue;
        }
        let Some(virtual_register) =
            mwcc_vreg::Reg::from_field(location.register, mwcc_vreg::Class::Float)
                .virtual_register()
        else {
            continue;
        };
        generator
            .register_prefer
            .insert(virtual_register, preferred);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_explicit_recovered_saved_float_homes() {
        assert_eq!(register("var_f25"), Some(25));
        assert_eq!(register("temp_f31"), Some(31));
        assert_eq!(register("var_f13"), None);
        assert_eq!(register("coefficient_f25"), None);
    }
}
