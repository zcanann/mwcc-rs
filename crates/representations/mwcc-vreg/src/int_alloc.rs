//! The INT LOCAL-ALLOCATOR model (fires 391-397): mwcc's register
//! assignment for frame/punned shapes with a computed guard and a shifted
//! mask, fitted 13/13 against the capture bank in
//! docs/int-allocator-frontier.md.
//!
//! Values assign the LOWEST register in r3..r10 free over their inclusive
//! live range, ordered
//!   Temp -> discarded loads (def-asc, ONLY when some r3+ value crosses
//!   the first branch) -> Computed -> Mask -> surviving loads (def-asc)
//!   -> Shift -> discarded loads (def-desc, when nothing crosses).
//! r0 separately takes branch-free values (the guard fold, record temps,
//! store-only rewrites, a single-use mask with adjacent def/use).

/// Value classes: the constant-synthesis temp, the mask constant, the
/// computed guard local (j0), a loaded home dead before the first branch
/// (discarded), a loaded home that survives it, and the multi-use shifted
/// mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Temp,
    Mask,
    Computed,
    LoadDiscarded,
    LoadSurviving,
    Shift,
}

/// One allocation candidate: its class, first-def and last-read positions
/// (any monotone instruction numbering).
#[derive(Clone, Copy, Debug)]
pub struct Value {
    pub class: Class,
    pub def: u32,
    pub last: u32,
}

/// The fitted assignment order (see the module docs).
pub fn model_order(values: &[Value]) -> Vec<usize> {
    let crossers = values
        .iter()
        .any(|value| matches!(value.class, Class::LoadSurviving | Class::Shift));
    let mut order: Vec<usize> = Vec::new();
    let mut push_class = |order: &mut Vec<usize>, class: Class, descending: bool| {
        let mut members: Vec<usize> =
            (0..values.len()).filter(|&i| values[i].class == class).collect();
        members.sort_by_key(|&i| {
            let def = values[i].def as i64;
            if descending {
                -def
            } else {
                def
            }
        });
        order.extend(members);
    };
    push_class(&mut order, Class::Temp, false);
    if crossers {
        push_class(&mut order, Class::LoadDiscarded, false);
    }
    // Computed and Mask order by DEATH between themselves: every early
    // fixture had the single-read guard dying before the mask (fold <
    // sraw), but arm3's multi-read guard outlives its li-form mask
    // (mask r3, j0 r4 — measured fire 400).
    {
        let mut members: Vec<usize> = (0..values.len())
            .filter(|&i| matches!(values[i].class, Class::Computed | Class::Mask))
            .collect();
        members.sort_by_key(|&i| values[i].last);
        order.extend(members);
    }
    push_class(&mut order, Class::LoadSurviving, false);
    push_class(&mut order, Class::Shift, false);
    if !crossers {
        push_class(&mut order, Class::LoadDiscarded, true);
    }
    order
}

/// Lowest register in r3..r10 free over each value's [def,last] (inclusive
/// overlap), assigned in `order`. Returns one register per value.
pub fn assign(order: &[usize], values: &[Value]) -> Vec<u8> {
    let mut chosen = vec![0u8; values.len()];
    for &index in order {
        let value = &values[index];
        'reg: for register in 3u8..=10 {
            for &previous in order.iter().take_while(|&&p| p != index) {
                if chosen[previous] == register {
                    let other = &values[previous];
                    if value.def <= other.last && other.def <= value.last {
                        continue 'reg;
                    }
                }
            }
            chosen[index] = register;
            break;
        }
    }
    chosen
}

/// The model applied: order + assign in one step.
pub fn allocate(values: &[Value]) -> Vec<u8> {
    assign(&model_order(values), values)
}
