use crate::{compile, SourceLanguage};

#[test]
fn emits_the_legacy_guarded_global_address_call() {
    let source = br#"
        typedef struct List List;
        static List* registry;
        extern int make_list(List** output, int size);

        int setup(void) {
            if (!make_list(&registry, 8)) {
                return 0;
            }
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "guarded-global-address-call.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the guarded global-address call should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, 0x38, 0x60, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04, 0x38, 0x80, 0x00,
        0x08, 0x94, 0x21, 0xff, 0xf8, 0x48, 0x00, 0x00, 0x01, 0x2c, 0x03, 0x00, 0x00, 0x40, 0x82,
        0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x48, 0x00, 0x00, 0x08, 0x38, 0x60, 0x00, 0x01, 0x80,
        0x01, 0x00, 0x0c, 0x38, 0x21, 0x00, 0x08, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
