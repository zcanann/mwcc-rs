//! Full-width BSS address recipes for the early compilers.
//!
//! A cursor at offset zero can own the section base itself. Other addresses
//! consist of a rounded high page and a signed low displacement. Reuse is of
//! that exact page expression, not arbitrary nearby completed addresses.

use super::{AddressPacket, Instruction};

pub(super) fn plan(
    offsets: &[u32],
    destinations: &[u32],
    temporaries: &[u32],
    share_pages: bool,
) -> AddressPacket {
    let zero = offsets.iter().position(|offset| *offset == 0);
    let temporary = temporaries[0];
    let first_page = high_page(offsets[0]);
    let distinct_pages = offsets
        .iter()
        .any(|offset| high_page(*offset) != first_page);
    // With several pages and no zero cursor, preserve the section base in a
    // second volatile register. Under pressure the source-order recipe remains
    // valid without that additional temporary.
    let share_pages = share_pages && (zero.is_some() || !distinct_pages || temporaries.len() >= 2);
    let base = zero.map_or_else(
        || {
            if share_pages && distinct_pages {
                temporaries[1]
            } else {
                temporary
            }
        },
        |index| destinations[index],
    );
    let mut recipe = Recipe {
        section_base: base,
        temporary,
        share_pages,
        instructions: vec![
            Instruction::AddImmediateShifted {
                d: temporary,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: base,
                a: temporary,
                immediate: 0,
            },
        ],
        pages: vec![(0, base)],
    };
    let mut complete = vec![false; offsets.len()];
    if let Some(index) = zero {
        complete[index] = true;
    }
    for index in 0..offsets.len() {
        if complete[index] {
            continue;
        }
        let page = high_page(offsets[index]);
        if share_pages && !recipe.pages.iter().any(|(value, _)| *value == page) {
            if let Some(owner) = offsets.iter().position(|offset| *offset == page) {
                recipe.emit(offsets[owner], destinations[owner]);
                complete[owner] = true;
            }
        }
        if !complete[index] {
            recipe.emit(offsets[index], destinations[index]);
            complete[index] = true;
        }
    }
    AddressPacket {
        instructions: recipe.instructions,
        displacements: Vec::new(),
    }
}

fn high_page(offset: u32) -> u32 {
    offset.wrapping_add(0x8000) & 0xffff0000
}

struct Recipe {
    section_base: u32,
    temporary: u32,
    share_pages: bool,
    instructions: Vec<Instruction>,
    pages: Vec<(u32, u32)>,
}

impl Recipe {
    fn emit(&mut self, offset: u32, destination: u32) {
        let page = high_page(offset);
        let low = offset as i16;
        let high = if let Some((_, register)) = self.pages.iter().find(|(value, _)| *value == page)
        {
            *register
        } else {
            let high = if self.share_pages && low != 0 {
                self.temporary
            } else {
                destination
            };
            self.instructions.push(Instruction::AddImmediateShifted {
                d: high,
                a: self.section_base,
                immediate: (page >> 16) as i16,
            });
            self.pages.retain(|(_, register)| *register != high);
            self.pages.push((page, high));
            high
        };
        if low != 0 || destination != high {
            self.instructions.push(Instruction::AddImmediate {
                d: destination,
                a: high,
                immediate: low,
            });
            self.pages.retain(|(_, register)| *register != destination);
        }
        if low == 0 {
            self.pages.push((page, destination));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_cursor_and_completed_page_have_separate_owners() {
        let p = plan(&[65536, 32768, 0], &[31, 30, 29], &[3, 4], false);
        assert_eq!(p.instructions.len(), 4);
        assert!(matches!(
            p.instructions[1],
            Instruction::AddImmediate {
                d: 29,
                a: 3,
                immediate: 0
            }
        ));
        assert!(matches!(
            p.instructions[2],
            Instruction::AddImmediateShifted {
                d: 31,
                a: 29,
                immediate: 1
            }
        ));
        assert!(matches!(
            p.instructions[3],
            Instruction::AddImmediate {
                d: 30,
                a: 31,
                immediate: -32768
            }
        ));
    }

    #[test]
    fn page_priority_introduces_only_exact_high_page_dependencies() {
        let ordinary = plan(&[0, 32768, 65536], &[31, 30, 29], &[3, 4], false);
        let page_first = plan(&[0, 32768, 65536], &[31, 30, 29], &[3, 4], true);
        assert_eq!(ordinary.instructions.len(), 5);
        assert_eq!(page_first.instructions.len(), 4);
        assert!(matches!(
            page_first.instructions[2],
            Instruction::AddImmediateShifted {
                d: 29,
                a: 31,
                immediate: 1
            }
        ));
        assert!(matches!(
            page_first.instructions[3],
            Instruction::AddImmediate {
                d: 30,
                a: 29,
                immediate: -32768
            }
        ));
        for policy in [false, true] {
            let partial = plan(&[32768, 33088, 33408], &[31, 30, 29], &[3, 4], policy);
            assert_eq!(partial.instructions.len(), if policy { 6 } else { 8 });
            assert_eq!(
                partial
                    .instructions
                    .iter()
                    .filter(|i| matches!(
                        i,
                        Instruction::AddImmediateShifted {
                            a: 3,
                            immediate: 1,
                            ..
                        }
                    ))
                    .count(),
                if policy { 1 } else { 3 }
            );
        }
    }

    #[test]
    fn recipes_preserve_complete_wrapping_addresses_across_boundaries() {
        let offsets = [
            0, 320, 32764, 32768, 65532, 65536, 98304, 131072, 0xffff8000,
        ];
        for values in offsets.windows(3) {
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                let values = order.map(|i| values[i]);
                for (policy, temporaries) in
                    [(false, &[3][..]), (true, &[3][..]), (true, &[3, 4][..])]
                {
                    let p = plan(&values, &[31, 30, 29], temporaries, policy);
                    for base in [0x106000u32, 0xabcdfedc, 0x7ffffffc, 0x80008000] {
                        let mut registers = [0u32; 32];
                        for (at, instruction) in p.instructions.iter().enumerate() {
                            let (d, a, immediate, shifted) = match instruction {
                                Instruction::AddImmediate { d, a, immediate } => {
                                    (*d, *a, *immediate, false)
                                }
                                Instruction::AddImmediateShifted { d, a, immediate } => {
                                    (*d, *a, *immediate, true)
                                }
                                _ => panic!("address arithmetic"),
                            };
                            let immediate = match at {
                                0 => (base.wrapping_add(0x8000) >> 16) as i16,
                                1 => base as i16,
                                _ => immediate,
                            } as i32 as u32;
                            let addend = if shifted { immediate << 16 } else { immediate };
                            registers[d as usize] =
                                (if a == 0 { 0u32 } else { registers[a as usize] })
                                    .wrapping_add(addend);
                        }
                        for (index, offset) in values.iter().enumerate() {
                            assert_eq!(
                                registers[31 - index],
                                base.wrapping_add(*offset),
                                "{values:?} page priority {policy}"
                            );
                        }
                    }
                }
            }
        }
    }
}
