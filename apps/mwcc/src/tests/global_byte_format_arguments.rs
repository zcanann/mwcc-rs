use crate::{compile, SourceLanguage};

#[test]
fn schedules_a_global_byte_formatter_argument_between_addresses() {
    let source = br#"
        struct Globals {
            unsigned char padding[1745];
            unsigned char active;
        };

        extern struct Globals globals;
        int format(char*, const char*, ...);

        char* render(void) {
            static char buffer[12];
            format(buffer, "%d", globals.active + 1);
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
        "global-byte-format-arguments.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the global byte formatter schedule should compile");

    let schedule = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,globals@ha
        0x3c, 0x80, 0x00, 0x00, // lis r4,string@ha
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x38, 0x63, 0x00, 0x00, // addi r3,r3,globals@l
        0x3c, 0xc0, 0x00, 0x00, // lis r6,buffer@ha
        0x38, 0x84, 0x00, 0x00, // addi r4,r4,string@l
        0x88, 0xa3, 0x06, 0xd1, // lbz r5,1745(r3)
        0x38, 0x66, 0x00, 0x00, // addi r3,r6,buffer@l
        0x38, 0xa5, 0x00, 0x01, // addi r5,r5,1
        0x4c, 0xc6, 0x31, 0x82, // crclr 6
    ];
    assert!(object
        .windows(schedule.len())
        .any(|bytes| bytes == schedule));
}
