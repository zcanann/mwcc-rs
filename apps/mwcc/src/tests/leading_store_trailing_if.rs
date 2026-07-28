use crate::{compile, SourceLanguage};

#[test]
fn schedules_link_front_null_test_inside_the_zero_store_window() {
    let source = br#"
        typedef struct Cell {
            struct Cell* prev;
            struct Cell* next;
            long size;
        } Cell;

        Cell* add_front(Cell* list, Cell* cell) {
            cell->next = list;
            cell->prev = 0;
            if (list) {
                list->prev = cell;
            }
            return cell;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "leading-store-trailing-if.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the linked-list front insertion should compile");

    let expected = [
        0x90, 0x64, 0x00, 0x04, // stw r3,4(r4)
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x28, 0x03, 0x00, 0x00, // cmplwi r3,0
        0x90, 0x04, 0x00, 0x00, // stw r0,0(r4)
        0x41, 0x82, 0x00, 0x08, // beq +8
        0x90, 0x83, 0x00, 0x00, // stw r4,0(r3)
        0x7c, 0x83, 0x23, 0x78, // mr r3,r4
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn rotates_a_for_header_linked_list_search() {
    let source = br#"
        typedef struct Cell {
            struct Cell* prev;
            struct Cell* next;
            long size;
        } Cell;

        Cell* find_cell(Cell* list, Cell* cell) {
            for (; list; list = list->next) {
                if (list == cell) {
                    return list;
                }
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
        "for-header-linked-list-search.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the for-header linked-list search should compile");

    let expected = [
        0x48, 0x00, 0x00, 0x10, // b +16
        0x7c, 0x03, 0x20, 0x40, // cmplw r3,r4
        0x4d, 0x82, 0x00, 0x20, // beqlr
        0x80, 0x63, 0x00, 0x04, // lwz r3,4(r3)
        0x28, 0x03, 0x00, 0x00, // cmplwi r3,0
        0x40, 0x82, 0xff, 0xf0, // bne -16
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
