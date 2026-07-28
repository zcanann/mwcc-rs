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

#[test]
fn schedules_a_nested_result_offset_after_reloadable_addresses() {
    let source = br#"
        int select_card(void);
        int format(char*, const char*, ...);

        char* seed_format(void) {
            static char buffer[12];
            format(buffer, "%d", 0);
            return buffer;
        }

        char* render_card(void) {
            static char buffer[12];
            format(buffer, "%c", 65 + select_card());
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
        "nested-result-offset.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the nested result offset schedule should compile");

    // The earlier "%d" makes "%c" an interior packed-string address at +3.
    // Preserve the nested result in r6, complete both reloadable addresses,
    // form the third argument, and materialize that pool offset last.
    let schedule = [
        0x3c, 0x80, 0x00, 0x00, // lis r4,string@ha
        0x3c, 0xa0, 0x00, 0x00, // lis r5,buffer@ha
        0x7c, 0x66, 0x1b, 0x78, // mr r6,r3
        0x38, 0x84, 0x00, 0x00, // addi r4,r4,string@l
        0x38, 0x65, 0x00, 0x00, // addi r3,r5,buffer@l
        0x38, 0xa6, 0x00, 0x41, // addi r5,r6,65
        0x38, 0x84, 0x00, 0x03, // addi r4,r4,3
        0x4c, 0xc6, 0x31, 0x82, // crclr 6
    ];
    assert!(object
        .windows(schedule.len())
        .any(|bytes| bytes == schedule));
}

#[test]
fn compiles_a_frame_buffer_nested_scene_tag_call_exactly() {
    let source = br#"
        struct Scene {
            unsigned id;
        };

        struct Globals {
            char padding[8128];
            struct Scene* scene;
        };

        extern struct Globals globals;
        char* idtag(unsigned, int);
        int sprintf(char*, const char*, ...);

        unsigned current_letter(void* context) {
            char buffer[16];
            unsigned most;
            unsigned char lowercase;

            sprintf(buffer, "%s", idtag(globals.scene->id, 0));
            most = buffer[0];
            lowercase = 0;
            if (most >= 'a' && most <= 'z')
                lowercase = 1;
            if (lowercase)
                most -= 0x20;
            return (most - 'A') + 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_read_only = true;
    flags.string_literals_packed = true;
    flags.char_default = mwcc_versions::CharDefault::Unsigned;
    let object = compile(
        source,
        "frame-buffer-nested-scene-tag.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the complete nested scene-tag expression should compile");

    // GC/2.0p1 oracle .text. This covers the two argument schedules, frame
    // element load, canonical-byte boolean home, and shared LR epilogue.
    let expected = [
        0x94, 0x21, 0xff, 0xe0, 0x7c, 0x08, 0x02, 0xa6, 0x3c, 0x60, 0x00, 0x00, 0x38, 0x80, 0x00,
        0x00, 0x90, 0x01, 0x00, 0x24, 0x38, 0x63, 0x00, 0x00, 0x80, 0x63, 0x1f, 0xc0, 0x80, 0x63,
        0x00, 0x00, 0x48, 0x00, 0x00, 0x01, 0x3c, 0x80, 0x00, 0x00, 0x7c, 0x65, 0x1b, 0x78, 0x38,
        0x84, 0x00, 0x00, 0x38, 0x61, 0x00, 0x08, 0x4c, 0xc6, 0x31, 0x82, 0x48, 0x00, 0x00, 0x01,
        0x88, 0x61, 0x00, 0x08, 0x38, 0x00, 0x00, 0x00, 0x28, 0x03, 0x00, 0x61, 0x41, 0x80, 0x00,
        0x10, 0x28, 0x03, 0x00, 0x7a, 0x41, 0x81, 0x00, 0x08, 0x38, 0x00, 0x00, 0x01, 0x54, 0x00,
        0x06, 0x3f, 0x41, 0x82, 0x00, 0x08, 0x38, 0x63, 0xff, 0xe0, 0x80, 0x01, 0x00, 0x24, 0x38,
        0x63, 0xff, 0xc0, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x20, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
