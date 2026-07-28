use crate::{compile, SourceLanguage};

#[test]
fn guards_an_indirect_return_through_a_direct_call_result() {
    let source = br#"
        struct Entry {
            int first;
            int second;
            int (*callback)(void);
        };
        extern struct Entry* find(int*);

        int dispatch(int* key) {
            struct Entry* entry = find(key);
            if (entry == 0)
                return 0;
            return entry->callback();
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "call-result-member-callback-guard.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the direct-call result should feed a guarded member callback");

    let entry = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x14, 0x48, 0x00, 0x00,
        0x01, 0x28, 0x03, 0x00, 0x00, 0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x48, 0x00,
        0x00, 0x10, 0x81, 0x83, 0x00, 0x08, 0x7d, 0x89, 0x03, 0xa6, 0x4e, 0x80, 0x04, 0x21, 0x80,
        0x01, 0x00, 0x14, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}
