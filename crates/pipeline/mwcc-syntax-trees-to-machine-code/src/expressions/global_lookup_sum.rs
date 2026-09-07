//! Compose pure integer lookup sums without assigning intermediate values to r0.
//! Address preparation is separate from loads, allowing the leading lookup to
//! overlap a tail addition and allowing existing section anchors to stay shared.

use super::masked_global_address::MaskedGlobalAddress;
use super::two_global_loads::GlobalMaskedLoad;
use super::*;
use std::collections::HashMap;

struct PreparedLookup<'a> {
    address: MaskedGlobalAddress,
    section_symbol: Option<&'a str>,
}

fn flatten_add<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    {
        flatten_add(left, terms);
        flatten_add(right, terms);
    } else {
        terms.push(expression);
    }
}

impl Generator {
    fn prepare_sum_lookup<'a>(
        &mut self,
        lookup: &GlobalMaskedLoad<'a>,
        bases: &mut HashMap<String, u8>,
    ) -> Compilation<PreparedLookup<'a>> {
        let anchor = self
            .data_section_anchor
            .as_ref()
            .filter(|a| a.symbols.contains(lookup.name))
            .and_then(|a| a.register);
        let base = if let Some(anchor) = anchor {
            anchor
        } else if let Some(base) = bases.get(lookup.name) {
            *base
        } else {
            let base = self.fresh_virtual_general();
            self.emit_global_array_base(lookup.name, lookup.total_size, base)?;
            bases.insert(lookup.name.to_owned(), base);
            base
        };
        let source = self.general_register_of_leaf(lookup.index.source)?;
        let offset = self.fresh_virtual_general();
        self.emit_masked_scale(&lookup.index, source, offset);
        let address = if anchor.is_some()
            || self.behavior.global_array_index_style
                == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
        {
            self.output.instructions.push(Instruction::Add {
                d: offset,
                a: base,
                b: offset,
            });
            MaskedGlobalAddress::Displacement { base: offset }
        } else {
            MaskedGlobalAddress::Indexed { base, offset }
        };
        Ok(PreparedLookup {
            address,
            section_symbol: anchor.map(|_| lookup.name),
        })
    }

    fn load_sum_lookup(
        &mut self,
        prepared: PreparedLookup<'_>,
        pointee: Pointee,
        destination: u8,
    ) -> Compilation<()> {
        if let Some(symbol) = prepared.section_symbol {
            self.record_data_section_symbol_displacement(symbol);
        }
        self.output
            .instructions
            .push(prepared.address.load(pointee, destination)?);
        Ok(())
    }

    fn add_lookup_sum_constant(&mut self, accumulator: u8, constant: u32) {
        if constant == 0 {
            return;
        }
        if let Ok(immediate) = i16::try_from(constant as i32) {
            self.output.instructions.push(Instruction::AddImmediate {
                d: accumulator,
                a: accumulator,
                immediate,
            });
        } else {
            self.load_integer_constant(GENERAL_SCRATCH, i64::from(constant as i32));
            self.output.instructions.push(Instruction::Add {
                d: accumulator,
                a: accumulator,
                b: GENERAL_SCRATCH,
            });
        }
    }

    pub(super) fn try_emit_global_lookup_sum(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let nested = |expr: &Expression| {
            matches!(
                expr,
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    ..
                }
            )
        };
        if !nested(left) && !nested(right) {
            return Ok(false);
        }
        let mut terms = Vec::new();
        flatten_add(left, &mut terms);
        flatten_add(right, &mut terms);
        let mut lookups = Vec::new();
        let mut scalars = Vec::new();
        let mut constant = 0u32;
        for term in terms {
            if let Expression::IntegerLiteral(value) = term {
                constant = constant.wrapping_add(*value as u32);
            } else if let Some(lookup) = self.global_masked_load(term) {
                if self.general_register_of_leaf(lookup.index.source)? == GENERAL_SCRATCH {
                    return Ok(false);
                }
                lookups.push((term, lookup));
            } else if let Some(register) = self
                .plain_integer_leaf_register(term)
                .filter(|r| *r != GENERAL_SCRATCH)
            {
                scalars.push(register);
            } else {
                return Ok(false);
            }
        }
        if lookups.len() < 2 {
            return Ok(false);
        }

        self.with_reserved_inputs(left, |me| {
            me.with_reserved_inputs(right, |me| {
                let accumulator = me.fresh_virtual_general();
                if me.behavior.optimization == mwcc_versions::Optimization::O0 {
                    me.evaluate_general(lookups[0].0, accumulator)?;
                    for (expression, _) in &lookups[1..] {
                        let value = me.fresh_virtual_general();
                        me.evaluate_general(expression, value)?;
                        me.output.instructions.push(Instruction::Add {
                            d: accumulator,
                            a: accumulator,
                            b: value,
                        });
                    }
                    me.add_lookup_sum_constant(accumulator, constant);
                    for &scalar in &scalars {
                        me.output.instructions.push(Instruction::Add {
                            d: accumulator,
                            a: accumulator,
                            b: scalar,
                        });
                    }
                    me.output
                        .instructions
                        .push(Instruction::move_register(destination, accumulator));
                    return Ok(true);
                }

                let mut bases = HashMap::new();
                let last = &lookups.last().unwrap().1;
                let prepared = me.prepare_sum_lookup(last, &mut bases)?;
                me.load_sum_lookup(prepared, last.pointee, accumulator)?;
                let mut pending = None;
                for (_, lookup) in lookups[1..lookups.len() - 1].iter().rev() {
                    let value = me.fresh_virtual_general();
                    let prepared = me.prepare_sum_lookup(lookup, &mut bases)?;
                    me.load_sum_lookup(prepared, lookup.pointee, value)?;
                    if let Some(previous) = pending {
                        me.output.instructions.push(Instruction::Add {
                            d: accumulator,
                            a: previous,
                            b: accumulator,
                        });
                    }
                    pending = Some(value);
                }
                let leading = &lookups[0].1;
                let prepared = me.prepare_sum_lookup(leading, &mut bases)?;
                if let Some(value) = pending {
                    me.output.instructions.push(Instruction::Add {
                        d: accumulator,
                        a: value,
                        b: accumulator,
                    });
                }
                let value = me.fresh_virtual_general();
                me.load_sum_lookup(prepared, leading.pointee, value)?;
                me.add_lookup_sum_constant(accumulator, constant);
                for &scalar in &scalars {
                    me.output.instructions.push(Instruction::Add {
                        d: accumulator,
                        a: accumulator,
                        b: scalar,
                    });
                }
                me.output.instructions.push(Instruction::Add {
                    d: destination,
                    a: value,
                    b: accumulator,
                });
                Ok(true)
            })
        })
    }
}
