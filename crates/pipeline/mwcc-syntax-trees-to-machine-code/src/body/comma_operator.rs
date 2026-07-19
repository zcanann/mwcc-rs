//! Build-specific comma-operator value placement.

use super::*;
use std::collections::HashSet;

impl Generator {
    /// Build 163 assigns argument-home stack slots to register parameters that
    /// survive a comma operator. The spill set follows parameter order; each
    /// consumer reloads from the home. Later builds keep the values in their
    /// incoming registers and therefore never enter this path.
    pub(crate) fn try_legacy_comma_parameter_homes(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.comma_value_placement_style
            != mwcc_versions::CommaValuePlacementStyle::ParameterHome
            || function.return_type != Type::Void
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || function_makes_call(function)
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || !function.parameters.iter().all(|parameter| {
                matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt)
            })
        {
            return Ok(false);
        }
        let [Statement::Store {
            target: Expression::Variable(target),
            value,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(
            self.globals.get(target.as_str()),
            Some(Type::Int | Type::UnsignedInt)
        ) {
            return Ok(false);
        }

        let mut homes = HashSet::new();
        collect_comma_value_homes(value, &mut homes)?;
        if homes.is_empty()
            || homes.iter().any(|name| {
                !function
                    .parameters
                    .iter()
                    .any(|parameter| &parameter.name == name)
            })
        {
            return Ok(false);
        }

        let mut highest_offset = 0i16;
        let mut spills = Vec::new();
        for (index, parameter) in function.parameters.iter().enumerate() {
            if !homes.contains(&parameter.name) {
                continue;
            }
            let offset = 8 + 4 * index as i16;
            let register = self.general_register_of(&parameter.name)?;
            highest_offset = highest_offset.max(offset);
            spills.push((parameter.name.clone(), register, offset));
        }

        let home_lane_size = 16 + 8 * homes.len() as i16;
        let mut frame_size = align_up_to_eight(highest_offset + 8)
            .max(24)
            .max(home_lane_size);
        if has_nested_pure_comma_lane(value) {
            frame_size += 8;
        }
        self.frame_size = frame_size;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame_size,
            });
        for (name, register, offset) in spills {
            self.frame_slots.insert(
                name,
                FrameSlot {
                    offset,
                    class: ValueClass::General,
                    size: 4,
                    parameter_register: Some(register),
                    is_array: false,
                },
            );
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset,
            });
        }

        self.emit_legacy_comma_store_value(value)?;
        self.record_relocation(RelocationKind::EmbSda21, target);
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: 0,
            offset: 0,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_legacy_comma_store_value(&mut self, value: &Expression) -> Compilation<()> {
        if let Expression::Comma { left, .. } = value {
            let name = comma_terminal_parameter(value).ok_or_else(|| {
                Diagnostic::error(
                    "a build 163 comma value without a parameter leaf is not modeled yet (roadmap)",
                )
            })?;
            self.load_comma_home(name, GENERAL_SCRATCH)?;
            // The home reload is scheduled before discarded assignment effects.
            self.emit_comma_side_effect(left)?;
            return Ok(());
        }

        let Expression::Binary {
            operator,
            left,
            right,
        } = value
        else {
            return Err(Diagnostic::error(
                "a build 163 comma consumer of this form is not modeled yet (roadmap)",
            ));
        };
        let left_comma = comma_terminal_parameter(left);
        let right_comma = comma_terminal_parameter(right);
        if let (Some(left_name), Some(right_name)) = (left_comma, right_comma) {
            if *operator != BinaryOperator::Add
                || comma_prefix_has_side_effect(left)
                || comma_prefix_has_side_effect(right)
            {
                return Err(Diagnostic::error(
                    "a build 163 binary comma consumer of this form is not modeled yet (roadmap)",
                ));
            }
            if left_name == right_name {
                self.load_comma_home(left_name, GENERAL_SCRATCH)?;
                self.output.instructions.push(Instruction::Add {
                    d: GENERAL_SCRATCH,
                    a: GENERAL_SCRATCH,
                    b: GENERAL_SCRATCH,
                });
                return Ok(());
            }

            let left_offset = self.comma_home_offset(left_name)?;
            let right_offset = self.comma_home_offset(right_name)?;
            let mut loads = [
                (left_offset, left_name, Eabi::general_result().number),
                (right_offset, right_name, GENERAL_SCRATCH),
            ];
            loads.sort_by_key(|(offset, _, _)| *offset);
            for (_, name, destination) in loads {
                self.load_comma_home(name, destination)?;
            }
            self.output.instructions.push(Instruction::Add {
                d: GENERAL_SCRATCH,
                a: Eabi::general_result().number,
                b: GENERAL_SCRATCH,
            });
            return Ok(());
        }

        let (name, constant) = match (left_comma, right_comma) {
            (Some(name), None) => (name, constant_value(right)),
            (None, Some(name)) => (name, constant_value(left)),
            _ => ("", None),
        };
        let Some(constant) = constant else {
            return Err(Diagnostic::error(
                "a build 163 comma consumer needs a measured leaf/constant shape (roadmap)",
            ));
        };
        let immediate = i16::try_from(constant).map_err(|_| {
            Diagnostic::error("a wide build 163 comma immediate is not modeled yet (roadmap)")
        })?;
        match operator {
            BinaryOperator::Add if left_comma.is_some() => {
                let source = Eabi::general_result().number;
                self.load_comma_home(name, source)?;
                self.output.instructions.push(Instruction::AddImmediate {
                    d: GENERAL_SCRATCH,
                    a: source,
                    immediate,
                });
            }
            BinaryOperator::Multiply => {
                self.load_comma_home(name, GENERAL_SCRATCH)?;
                self.output.instructions.push(Instruction::MultiplyImmediate {
                    d: GENERAL_SCRATCH,
                    a: GENERAL_SCRATCH,
                    immediate,
                });
            }
            _ => {
                return Err(Diagnostic::error(
                    "a build 163 comma immediate consumer of this form is not modeled yet (roadmap)",
                ));
            }
        }
        Ok(())
    }

    fn comma_home_offset(&self, name: &str) -> Compilation<i16> {
        self.frame_slots
            .get(name)
            .map(|slot| slot.offset)
            .ok_or_else(|| Diagnostic::error("a build 163 comma home was not allocated"))
    }

    fn load_comma_home(&mut self, name: &str, destination: u8) -> Compilation<()> {
        let offset = self.comma_home_offset(name)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: destination,
            a: 1,
            offset,
        });
        Ok(())
    }
}

fn collect_comma_value_homes(
    expression: &Expression,
    homes: &mut HashSet<String>,
) -> Compilation<()> {
    match expression {
        Expression::Comma { right, .. } => {
            let mut surviving = right.as_ref();
            while let Expression::Comma { right, .. } = surviving {
                surviving = right;
            }
            let Expression::Variable(name) = surviving else {
                return Err(Diagnostic::error(
                    "a build 163 comma value without a parameter leaf is not modeled yet (roadmap)",
                ));
            };
            homes.insert(name.clone());
        }
        Expression::Binary { left, right, .. } => {
            collect_comma_value_homes(left, homes)?;
            collect_comma_value_homes(right, homes)?;
        }
        _ => {}
    }
    Ok(())
}

fn has_nested_pure_comma_lane(expression: &Expression) -> bool {
    match expression {
        Expression::Comma { left, right } => {
            let nested_lane = matches!(
                left.as_ref(),
                Expression::Comma { right, .. } if !expression_has_side_effect(right)
            );
            nested_lane
                || has_nested_pure_comma_lane(left)
                || has_nested_pure_comma_lane(right)
        }
        Expression::Binary { left, right, .. } => {
            has_nested_pure_comma_lane(left) || has_nested_pure_comma_lane(right)
        }
        _ => false,
    }
}

fn comma_terminal_parameter(expression: &Expression) -> Option<&str> {
    let Expression::Comma { right, .. } = expression else {
        return None;
    };
    let mut surviving = right.as_ref();
    while let Expression::Comma { right, .. } = surviving {
        surviving = right;
    }
    match surviving {
        Expression::Variable(name) => Some(name.as_str()),
        _ => None,
    }
}

fn comma_prefix_has_side_effect(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Comma { left, .. } if expression_has_side_effect(left)
    )
}

fn align_up_to_eight(value: i16) -> i16 {
    (value + 7) & !7
}
