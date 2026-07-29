use crate::{compile, SourceLanguage};

#[test]
fn materializes_member_and_byte_array_indices_for_a_struct_table_load() {
    let source = br#"
        typedef unsigned char u8;
        typedef unsigned short u16;

        typedef struct Entry {
            u16 text_id;
            unsigned value;
        } Entry;

        typedef struct State {
            u8 pad;
            u8 index;
        } State;

        extern u8 text_ids[80];
        extern Entry entries[80];
        extern void consume(int);

        void compiled(State* state) {
            consume(entries[text_ids[state->index]].text_id);
        }
    "#;
    let object = compile(
        source,
        "nested-global-index-member.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags: mwcc_versions::Flags::default(),
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a nested member/byte-array index should feed a global struct-table load");

    // lbz index; materialize text_ids; lbzx mapped index; scale by Entry's
    // eight-byte stride; materialize entries; lhz text_id.
    let nested_load = [
        0x88, 0x63, 0x00, 0x01, 0x3c, 0x80, 0x00, 0x00, 0x38, 0x04, 0x00, 0x00, 0x7c, 0x60,
        0x1a, 0x14, 0x88, 0x63, 0x00, 0x00, 0x3c, 0x80, 0x00, 0x00, 0x54, 0x63, 0x18, 0x38,
        0x38, 0x04, 0x00, 0x00, 0x7c, 0x60, 0x1a, 0x14, 0xa0, 0x63, 0x00, 0x00,
    ];
    assert!(object
        .windows(nested_load.len())
        .any(|bytes| bytes == nested_load));
}
