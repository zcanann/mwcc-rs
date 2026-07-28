use crate::{compile, SourceLanguage};

#[test]
fn schedules_a_nested_tail_result_between_reloadable_addresses() {
    let source = br#"
        struct Scene {
            unsigned int id;
        };

        extern struct Scene* scene;
        char* id_to_string(unsigned int, int);
        int format(char*, const char*, ...);

        char* render(void) {
            static char buffer[32];
            format(buffer, "%s", id_to_string(scene->id, 0));
            return buffer;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_read_only = true;
    flags.string_literals_packed = true;
    let object = compile(
        source,
        "nested-tail-call-arguments.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("reloadable prefix arguments should not require a callee-saved register");

    // After the nested call, overlap the string and static-array address
    // chains, preserve its r3 result in argument r5, and only then replace r3.
    let schedule = [
        0x3c, 0x80, 0x00, 0x00, // lis r4,string@ha
        0x3c, 0xc0, 0x00, 0x00, // lis r6,buffer@ha
        0x38, 0x84, 0x00, 0x00, // addi r4,r4,string@l
        0x7c, 0x65, 0x1b, 0x78, // mr r5,r3
        0x38, 0x66, 0x00, 0x00, // addi r3,r6,buffer@l
    ];
    assert!(object
        .windows(schedule.len())
        .any(|bytes| bytes == schedule));
}
