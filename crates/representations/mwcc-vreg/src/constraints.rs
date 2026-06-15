//! The target's register-allocation rules, as data.
//!
//! These are the invariants the allocator must honor for the Gekko/PowerPC target
//! mwcceppc emits — the allocatable pools, the scratch registers, and the one
//! hardware quirk that shapes everything: `r0` can never be an address base. They
//! are expressed as data (not buried in the allocator) so a different target or a
//! version that allocated differently is a different [`RegisterConstraints`], not
//! a forked allocator — the same reason behavior lives in `mwcc-versions`.

use crate::register::Class;

/// The registers the allocator may assign, the scratch registers it may not, and
/// the placement rules, for one target.
#[derive(Debug, Clone)]
pub struct RegisterConstraints {
    /// General-purpose registers the allocator may assign, in preference order
    /// (mwcc favors the lowest). `r3..=r12` — `r0..=r2`/`r13` are reserved by the
    /// ABI (scratch, stack, small-data) and `r14+` are callee-saved (unused so far).
    pub general_pool: Vec<u8>,
    /// Floating-point registers the allocator may assign, in preference order.
    /// `f1..=f13` — `f0` is the scratch, `f14+` callee-saved.
    pub float_pool: Vec<u8>,
    /// The general scratch (`r0`): a transient for a value consumed immediately.
    pub general_scratch: u8,
    /// The float scratch (`f0`).
    pub float_scratch: u8,
}

impl RegisterConstraints {
    /// The Gekko/PowerPC constraints for the GameCube/Wii EABI mwcceppc targets.
    pub fn gekko() -> Self {
        RegisterConstraints {
            general_pool: (3..=12).collect(),
            float_pool: (1..=13).collect(),
            general_scratch: 0,
            float_scratch: 0,
        }
    }

    /// The allocatable pool for a class, in preference order.
    pub fn pool(&self, class: Class) -> &[u8] {
        match class {
            Class::General => &self.general_pool,
            Class::Float => &self.float_pool,
        }
    }

    /// The scratch register for a class.
    pub fn scratch(&self, class: Class) -> u8 {
        match class {
            Class::General => self.general_scratch,
            Class::Float => self.float_scratch,
        }
    }

    /// Whether a general register may hold a computed *address base*. `r0` may
    /// not: `addi rD,r0,x` and `lwz rD,x(r0)` read literal zero, not the register
    /// (the `li`/literal-zero trap). This is why an address materialized for a
    /// folded load/store never lands in `r0`, and why the absolute-addressing
    /// fold rule takes the shape it does.
    pub fn can_be_base(&self, register: u8) -> bool {
        register != self.general_scratch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gekko_pools_are_lowest_first_and_exclude_the_scratch() {
        let constraints = RegisterConstraints::gekko();
        assert_eq!(constraints.pool(Class::General).first(), Some(&3));
        assert_eq!(constraints.pool(Class::General).last(), Some(&12));
        assert_eq!(constraints.pool(Class::Float).first(), Some(&1));
        assert!(!constraints.pool(Class::General).contains(&constraints.general_scratch));
        assert!(!constraints.pool(Class::Float).contains(&constraints.float_scratch));
    }

    #[test]
    fn r0_is_never_a_valid_base() {
        let constraints = RegisterConstraints::gekko();
        assert!(!constraints.can_be_base(0));
        assert!(constraints.can_be_base(3));
    }
}
