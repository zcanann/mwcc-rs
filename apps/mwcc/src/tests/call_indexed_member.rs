use crate::{compile, SourceLanguage};

#[test]
fn loads_a_member_backed_array_after_calling_for_its_index() {
    let source = br#"
        struct Part { void* joint; char pad[12]; };
        struct Owner { char pad[1512]; struct Part* parts; };
        extern int bone(struct Owner*, int);
        void* compiled(struct Owner* owner) {
            return owner->parts[bone(owner, 4)].joint;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "call-indexed-member.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a call result should index a member-backed struct array");

    // bl bone; lwz r4,1512(r31); slwi r0,r3,4; lwzx r3,r4,r0
    let expected = [
        0x48, 0x00, 0x00, 0x01, 0x80, 0x9f, 0x05, 0xe8, 0x54, 0x60, 0x20, 0x36,
        0x7c, 0x64, 0x00, 0x2e,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
