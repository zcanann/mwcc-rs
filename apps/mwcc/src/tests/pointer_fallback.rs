use crate::{compile, SourceLanguage};

#[test]
fn reuses_a_member_load_in_a_pointer_fallback_getter() {
    let source = br#"
        typedef struct Owner {
            int pad;
            void* primary;
        } Owner;
        extern void* fallback;
        void* get(Owner* owner) {
            if (owner->primary) {
                return owner->primary;
            }
            return fallback;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "pointer-fallback.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_7,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the member pointer fallback should compile");

    let expected = [
        0x80, 0x63, 0x00, 0x04, // lwz r3,4(r3)
        0x28, 0x03, 0x00, 0x00, // cmplwi r3,0
        0x4c, 0x82, 0x00, 0x20, // bnelr
        0x80, 0x60, 0x00, 0x00, // lwz r3,fallback@sda21
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
