use crate::{compile, SourceLanguage};

fn gc_2_0p1(source: &[u8], name: &str) -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
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
    .expect("the measured member-pointer/bit-field shape should compile")
}

#[test]
fn loads_a_byte_through_a_pointer_typed_member() {
    let object = gc_2_0p1(
        br#"
            struct Span { unsigned char* text; unsigned size; };
            int matches(struct Span* span) {
                if (span->text[0] != 58) return 0;
                return 1;
            }
        "#,
        "member-pointer-byte.c",
    );

    let entry = [
        0x80, 0x63, 0x00, 0x00, 0x88, 0x03, 0x00, 0x00, 0x20, 0x00, 0x00, 0x3a, 0x7c, 0x00, 0x00,
        0x34, 0x54, 0x03, 0xd9, 0x7e, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}

#[test]
fn preserves_the_value_of_a_chained_bit_field_assignment() {
    let object = gc_2_0p1(
        br#"
            struct Flags {
                unsigned char insert : 1;
                unsigned char dynamic : 1;
            };
            void set(struct Flags* flags) {
                flags->insert = flags->dynamic = 1;
            }
        "#,
        "chained-bit-field-assignment.c",
    );

    let entry = [
        0x88, 0x03, 0x00, 0x00, 0x38, 0x80, 0x00, 0x01, 0x50, 0x80, 0x36, 0x72, 0x98, 0x03, 0x00,
        0x00, 0x54, 0x04, 0xd7, 0xfe, 0x88, 0x03, 0x00, 0x00, 0x50, 0x80, 0x3e, 0x30, 0x98, 0x03,
        0x00, 0x00, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}
