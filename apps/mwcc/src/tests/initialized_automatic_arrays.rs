use crate::{compile, SourceLanguage};

#[test]
fn copies_a_short_double_array_through_one_rodata_image() {
    let source = br#"
        void consume(const double*);

        double initialize(unsigned index) {
            const double table[8] = {
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0
            };
            consume(table);
            return table[index];
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "short-double-array-image.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_3_0A3,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a short double array should use its direct image transaction");

    // lfdu f7,image(r4); lfd f6..f0; stfd f7..f0,8..64(r1).
    let transaction = [
        0xcc, 0xe4, 0x00, 0x00, 0xc8, 0xc4, 0x00, 0x08, 0xc8, 0xa4, 0x00, 0x10,
        0xc8, 0x84, 0x00, 0x18, 0xc8, 0x64, 0x00, 0x20, 0xc8, 0x44, 0x00, 0x28,
        0xc8, 0x24, 0x00, 0x30, 0xc8, 0x04, 0x00, 0x38, 0xd8, 0xe1, 0x00, 0x08,
        0xd8, 0xc1, 0x00, 0x10, 0xd8, 0xa1, 0x00, 0x18, 0xd8, 0x81, 0x00, 0x20,
        0xd8, 0x61, 0x00, 0x28, 0xd8, 0x41, 0x00, 0x30, 0xd8, 0x21, 0x00, 0x38,
        0xd8, 0x01, 0x00, 0x40,
    ];
    assert!(object
        .windows(transaction.len())
        .any(|bytes| bytes == transaction));
}

#[test]
fn zero_fills_the_implicit_tail_of_an_initialized_automatic_array() {
    let source = br#"
        void consume(char*);

        void initialize(void) {
            char buffer[32] = "";
            consume(buffer);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "initialized-automatic-array.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an initialized automatic array should compile through its structured frame");

    // li r0,0; li r3,8; addi r4,r1,slot-4; mtctr r3;
    // loop: stwu r0,4(r4); bdnz loop
    let zero_fill = [
        0x38, 0x00, 0x00, 0x00, 0x38, 0x60, 0x00, 0x08, 0x38, 0x81, 0x00, 0x04, 0x7c, 0x69, 0x03,
        0xa6, 0x94, 0x04, 0x00, 0x04, 0x42, 0x00, 0xff, 0xfc,
    ];
    assert!(object
        .windows(zero_fill.len())
        .any(|bytes| bytes == zero_fill));
}

#[test]
fn pools_multiple_initialized_arrays_into_a_dense_copy_transaction() {
    let source = br#"
        void consume(char*);

        void initialize(void) {
            char date[32] = "";
            char time[32] = "";
            char ampm[32] = "";
            char buffer[256] = "";
            consume(date);
            consume(time);
            consume(ampm);
            consume(buffer);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "pooled-initialized-automatic-arrays.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("multiple initialized arrays should use the pooled structured frame");

    // stmw r14,...(r1) owns the fixed direct-copy window and the tail loop's
    // count register.
    assert!(object.windows(2).any(|bytes| bytes == [0xbd, 0xc1]));

    // addi r3,r5,pool-image; li r14,32
    let first_image = [0x38, 0x65, 0x00, 0x5c, 0x39, 0xc0, 0x00, 0x20];
    assert!(object
        .windows(first_image.len())
        .any(|bytes| bytes == first_image));

    // mtctr r14; lwz r5,4(r3); lwzu r0,8(r3); stw r5,4(r4);
    // stwu r0,8(r4); bdnz
    let tail_copy = [
        0x7d, 0xc9, 0x03, 0xa6, 0x80, 0xa3, 0x00, 0x04, 0x84, 0x03, 0x00, 0x08, 0x90, 0xa4, 0x00,
        0x04, 0x94, 0x04, 0x00, 0x08, 0x42, 0x00, 0xff, 0xf0,
    ];
    assert!(object
        .windows(tail_copy.len())
        .any(|bytes| bytes == tail_copy));
}

#[test]
fn copies_pooled_frame_parameters_in_incoming_register_order() {
    let source = br#"
        #pragma use_lmw_stmw on
        void consume(char*);
        void consume_int(int);

        char* initialize(int index, char* output, unsigned unused) {
            char date[32] = "";
            char time[32] = "";
            char ampm[32] = "";
            char buffer[256] = "";
            consume(date);
            consume(time);
            consume(ampm);
            consume(buffer);
            consume_int(index);
            return output;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "pooled-frame-parameter-order.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a pooled frame with two retained parameters should compile");

    // stmw r14,360(r1); mr r30,r3; mr r31,r4
    let entry = [
        0xbd, 0xc1, 0x01, 0x68, 0x7c, 0x7e, 0x1b, 0x78, 0x7c, 0x9f, 0x23, 0x78,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));
}

#[test]
fn reuses_an_expired_pool_lane_for_a_parsed_hour() {
    let source = br#"
        void consume(char*);
        void consume_int(int);
        int atoi(const char*);

        int initialize(void) {
            char date[32] = "";
            char time[32] = "";
            char ampm[32] = "";
            char buffer[256] = "";
            consume(date);
            consume(time);
            consume(ampm);
            consume(buffer);

            int hour = atoi(time);
            if (hour >= 12) {
                if (hour != 12)
                    hour -= 12;
            } else if (hour == 0) {
                hour = 12;
            }
            consume_int(hour);
            return hour;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "pooled-parsed-hour.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a parsed value should reuse an expired pooled-copy lane");

    // mr r16,r3; cmpwi r16,12
    let parsed_hour = [0x7c, 0x70, 0x1b, 0x78, 0x2c, 0x10, 0x00, 0x0c];
    assert!(object
        .windows(parsed_hour.len())
        .any(|bytes| bytes == parsed_hour));
}

#[test]
fn issues_compact_pooled_array_entry_packets_in_mwcc_order() {
    let source = br#"
        #pragma use_lmw_stmw on
        typedef unsigned long size_t;
        struct SaveEntry {
            char prefix[64];
            char date[32];
        };
        extern SaveEntry saves[];
        char* strncpy(char*, const char*, size_t);
        int sprintf(char*, const char*, ...);

        char* initialize(int index, char* output, size_t unused) {
            char date[32] = "";
            char time[32] = "";
            char ampm[32] = "";
            char buffer[256] = "";

            strncpy(date, saves[index].date, 5);
            date[2] = '/';
            sprintf(buffer, "%s/%c%c", date, saves[index].date[8],
                    saves[index].date[9]);
            strncpy(date, buffer, 32);
            date[31] = '\0';
            sprintf(time, "%c%c", saves[index].date[11],
                    saves[index].date[12]);
            return output;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_packed = true;
    flags.string_literals_read_only = true;
    let object = compile(
        source,
        "compact-pooled-array-entry.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the compact pooled-array frame should compile");

    // li r0,32; addi r6,r1,68; stmw r21,340(r1); addi r21,r5,rodata@l;
    // mr r31,r4; addi r5,r21,92
    let entry_packets = [
        0x38, 0x00, 0x00, 0x20, 0x38, 0xc1, 0x00, 0x44, 0xbe, 0xa1, 0x01, 0x54, 0x3a, 0xa5, 0x00,
        0x00, 0x7c, 0x9f, 0x23, 0x78, 0x38, 0xb5, 0x00, 0x5c,
    ];
    assert!(object
        .windows(entry_packets.len())
        .any(|bytes| bytes == entry_packets));

    // The following global-table address overlaps the date separator store,
    // then releases r3/r5 for the formatted call's frame arguments.
    let following_call = [
        0x3c, 0x60, 0x00, 0x00, 0x38, 0xa0, 0x00, 0x2f, 0x38, 0x03, 0x00, 0x00, 0x3c, 0x80, 0x00,
        0x00, 0x7c, 0x60, 0xaa, 0x14, 0x98, 0xa1, 0x00, 0x2a, 0x88, 0xc3, 0x00, 0x48, 0x38, 0x84,
        0x00, 0x00, 0x88, 0xe3, 0x00, 0x49, 0x38, 0x61, 0x00, 0x48, 0x38, 0xa1, 0x00, 0x28, 0x4c,
        0xc6, 0x31, 0x82, 0x48, 0x00, 0x00, 0x01,
    ];
    assert!(object
        .windows(following_call.len())
        .any(|bytes| bytes == following_call));
}
