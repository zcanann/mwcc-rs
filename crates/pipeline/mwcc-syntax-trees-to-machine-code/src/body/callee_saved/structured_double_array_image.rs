//! Direct copy-in for a short initialized automatic `double` array.
//!
//! GC 4.1 materializes one anonymous read-only image, loads arrays of at most
//! eight elements through descending volatile FPRs, then spills the complete
//! image into its structured frame slot. Nine elements cross a distinct
//! counted-copy threshold; that loop deliberately remains a separate owner.

#[allow(unused_imports)]
use super::*;

const DIRECT_ELEMENT_LIMIT: u16 = 8;

pub(super) struct StructuredDoubleArrayImage<'a> {
    array: &'a LocalDeclaration,
    explicit: &'a [u8],
    element_count: u16,
}

pub(super) fn plan<'a>(
    arrays: &[&'a LocalDeclaration],
    image_sources: &[&'a LocalDeclaration],
) -> Option<StructuredDoubleArrayImage<'a>> {
    let [array] = arrays else {
        return None;
    };
    let [source] = image_sources else {
        return None;
    };
    let element_count = array.array_length?;
    let explicit = source.data_bytes.as_deref()?;
    let byte_count = usize::from(element_count).checked_mul(8)?;
    if source.name != array.name
        || array.declared_type != Type::Double
        || !(1..=DIRECT_ELEMENT_LIMIT).contains(&element_count)
        || !array.data_relocations.is_empty()
        || explicit.is_empty()
        || explicit.len() > byte_count
        || explicit.iter().all(|byte| *byte == 0)
    {
        return None;
    }
    Some(StructuredDoubleArrayImage {
        array,
        explicit,
        element_count,
    })
}

impl Generator {
    pub(super) fn emit_structured_double_array_image(
        &mut self,
        plan: StructuredDoubleArrayImage<'_>,
    ) -> Compilation<()> {
        let slot = self
            .frame_slots
            .get(&plan.array.name)
            .copied()
            .ok_or_else(|| Diagnostic::error("initialized double array has no frame slot"))?;
        let byte_count = usize::from(plan.element_count) * 8;
        if slot.offset % 8 != 0 || slot.size != byte_count as u32 {
            return Err(Diagnostic::error(
                "initialized double array has an incompatible frame slot",
            ));
        }

        let mut image = vec![0; byte_count];
        image[..plan.explicit.len()].copy_from_slice(plan.explicit);
        self.output
            .anonymous_rodata
            .push(mwcc_machine_code::AnonymousRodata {
                bytes: image,
                // Ordinal accounting resolves the declaration-time slot after
                // all structured front labels have been measured.
                static_slot_prefix_bump: Some(0),
                anonymous_offset: 0,
            });
        let image = self.output.anonymous_rodata.len() - 1;

        let base = self.fresh_virtual_general_preferring(4);
        let floating_values: Vec<u8> = (0..plan.element_count)
            .map(|index| {
                self.fresh_virtual_float_preferring(
                    u8::try_from(plan.element_count - index - 1)
                        .expect("direct double-array register is bounded"),
                )
            })
            .collect();

        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::AnonymousRodataAt(image),
        );
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: base,
                a: 0,
                immediate: 0,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::AnonymousRodataAt(image),
        );
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleWithUpdate {
                d: floating_values[0],
                a: base,
                offset: 0,
            });
        for (index, &value) in floating_values.iter().enumerate().skip(1) {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: value,
                a: base,
                offset: i16::try_from(index * 8)
                    .map_err(|_| Diagnostic::error("double-array image is too large"))?,
            });
        }
        for (index, value) in floating_values.into_iter().enumerate() {
            self.output
                .instructions
                .push(Instruction::StoreFloatDouble {
                    s: value,
                    a: 1,
                    offset: slot.offset
                        + i16::try_from(index * 8)
                            .map_err(|_| Diagnostic::error("double-array slot is too large"))?,
                });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn double_array(length: u16, bytes: Vec<u8>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Double,
            name: "table".into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(length),
            is_static: false,
            data_bytes: Some(bytes),
            data_relocations: Vec::new(),
            is_const: true,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn accepts_the_measured_eight_element_direct_window() {
        let table = double_array(8, vec![1; 64]);
        assert!(plan(&[&table], &[&table]).is_some());
    }

    #[test]
    fn leaves_the_nine_element_counted_copy_to_its_own_owner() {
        let table = double_array(9, vec![1; 72]);
        assert!(plan(&[&table], &[&table]).is_none());
    }

    #[test]
    fn leaves_zero_images_on_the_zero_fill_path() {
        let table = double_array(8, vec![0; 64]);
        assert!(plan(&[&table], &[&table]).is_none());
    }
}
