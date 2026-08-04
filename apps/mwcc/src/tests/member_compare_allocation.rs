use crate::{compile, SourceLanguage};

#[test]
fn coalesces_a_member_compare_value_with_its_dying_base() {
    let source = br#"
        struct Owner { void *value; };
        struct Target { int padding; void *value; };

        int same_value(struct Owner *owner, struct Target *target) {
            if (target->value == owner->value) {
                return 1;
            }
            return 0;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "member-compare.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the member comparison should compile");

    let expected = [
        0x80, 0x84, 0x00, 0x04, // lwz r4,4(r4)
        0x80, 0x03, 0x00, 0x00, // lwz r0,0(r3)
        0x7c, 0x04, 0x00, 0x40, // cmplw r4,r0
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
