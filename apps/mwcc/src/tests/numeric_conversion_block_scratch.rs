use crate::{compile, SourceLanguage};

fn gc_2_0p1(source: &[u8], name: &str) -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    compile(
        source,
        name,
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("numeric conversion probe should compile")
}

fn stack_d_form_count(object: &[u8], opcode: u32, displacement: i16) -> usize {
    object
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().expect("four-byte instruction")))
        .filter(|word| {
            word >> 26 == opcode
                && (word >> 16) & 0x1f == 1
                && *word as u16 == displacement as u16
        })
        .count()
}

#[test]
fn reuses_one_numeric_conversion_image_across_control_flow_regions() {
    let object = gc_2_0p1(
        br#"
            typedef struct Curve {
                int pad;
                int clamp;
                float delta;
                unsigned n;
                float* points;
            } Curve;
            int abs(int);
            float evaluate(Curve* curve, float t) {
                float max_t = curve->delta * (curve->n - 1);
                if (curve->clamp == 0) {
                    float limited = t < max_t ? t : max_t;
                    t = limited > 0.0f ? limited : 0.0f;
                } else {
                    int shift = t / max_t;
                    if (t < 0.0f) shift--;
                    t -= shift * max_t;
                    if (curve->clamp == 2 && abs(shift % 2) == 1) t = max_t - t;
                }
                unsigned point = t / curve->delta;
                float u = (t - point * curve->delta) / curve->delta;
                return (1.0f - u) * curve->points[point]
                    + u * curve->points[point + 1];
            }
        "#,
        "numeric-conversion-block-scratch.c",
    );

    // GC/1.3.2 through GC/2.7 all select a 64-byte frame here. Every
    // stack-backed conversion uses the one doubleword at r1+8/r1+12;
    // the unsigned conversion remains a runtime call and owns no image.
    assert!(object.windows(4).any(|bytes| bytes == [0x94, 0x21, 0xff, 0xc0]));
    assert_eq!(stack_d_form_count(&object, 54, 8), 1); // stfd
    assert_eq!(stack_d_form_count(&object, 50, 8), 3); // lfd
    assert_eq!(stack_d_form_count(&object, 36, 8), 3); // stw high word
    assert_eq!(stack_d_form_count(&object, 36, 12), 3); // stw low word
}

#[test]
fn preserves_distinct_images_for_a_straight_line_conversion_cluster() {
    let object = gc_2_0p1(
        br#"
            float straight(int first, int second) {
                float left = first;
                float right = second;
                return left + right;
            }
        "#,
        "straight-line-conversion-scratch.c",
    );

    assert!(object.windows(4).any(|bytes| bytes == [0x94, 0x21, 0xff, 0xe0]));
    assert_eq!(stack_d_form_count(&object, 36, 12), 1);
    assert_eq!(stack_d_form_count(&object, 36, 20), 1);
}

#[test]
fn preserves_distinct_images_for_mixed_branch_conversions_without_a_source_call() {
    let object = gc_2_0p1(
        br#"
            float mixed(int count, float delta, float t, int enabled) {
                float maximum = delta * (count - 1);
                if (enabled) {
                    int shift = t / maximum;
                    t -= shift * maximum;
                }
                return t;
            }
        "#,
        "mixed-branch-conversion-scratch.c",
    );

    // This also has at most one conversion per source basic block, but MWCC
    // retains three direction-specific images for this leaf shape.
    assert!(object.windows(4).any(|bytes| bytes == [0x94, 0x21, 0xff, 0xe0]));
    assert_eq!(stack_d_form_count(&object, 36, 8), 1);
    assert_eq!(stack_d_form_count(&object, 36, 16), 1);
    assert_eq!(stack_d_form_count(&object, 54, 24), 1);
}
