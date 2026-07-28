use crate::{compile, SourceLanguage};

#[test]
fn emits_the_legacy_bounded_member_assignment() {
    let source = br#"
        typedef struct File {
            int unused[4];
            int size;
            int position;
        } File;

        int set_position(File* file, int position) {
            if ((position >= 0) && (position < file->size)) {
                file->position = position;
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
        "bounded-member-assignment.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the bounded member assignment should compile");

    let expected = [
        0x2c, 0x04, 0x00, 0x00, 0x41, 0x80, 0x00, 0x1c, 0x80, 0x03, 0x00, 0x10, 0x7c, 0x04, 0x00,
        0x00, 0x40, 0x80, 0x00, 0x10, 0x90, 0x83, 0x00, 0x14, 0x38, 0x60, 0x00, 0x01, 0x4e, 0x80,
        0x00, 0x20, 0x38, 0x60, 0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
