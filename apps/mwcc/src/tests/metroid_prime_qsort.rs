use crate::{compile, SourceLanguage};

const SOURCE: &[u8] = br#"
    typedef unsigned long size_t;
    typedef int (*_compare_function)(const void*, const void*);

    void qsort(void* table_base, size_t num_members, size_t member_size,
               _compare_function compare_members) {
        size_t l, r, j;
        char* lp;
        char* rp;
        char* ip;
        char* jp;
        char* kp;

        if (num_members < 2)
            return;

        r = num_members;
        l = (r / 2) + 1;
        lp = ((char*)table_base) + (member_size * (l - 1));
        rp = ((char*)table_base) + (member_size * (r - 1));

        for (;;) {
            if (l > 1) {
                l--;
                lp -= member_size;
            } else {
                do {
                    char* p;
                    char* q;
                    size_t n = member_size;
                    unsigned long tmp;
                    for (p = (char*)rp - 1, q = (char*)lp - 1, n++; --n;) {
                        tmp = *++q;
                        *q = *++p;
                        *p = tmp;
                    }
                } while (0);

                if (--r == 1)
                    return;
                rp -= member_size;
            }

            j = l;
            jp = ((char*)table_base) + (member_size * (j - 1));

            while (j * 2 <= r) {
                j *= 2;
                ip = jp;
                jp = ((char*)table_base) + (member_size * (j - 1));

                if (j < r) {
                    kp = jp + member_size;
                    if (compare_members(jp, kp) < 0) {
                        j++;
                        jp = kp;
                    }
                }

                if (compare_members(ip, jp) < 0)
                    do {
                        char* p;
                        char* q;
                        size_t n = member_size;
                        unsigned long tmp;
                        for (p = (char*)jp - 1, q = (char*)ip - 1, n++; --n;) {
                            tmp = *++q;
                            *q = *++p;
                            *p = tmp;
                        }
                    } while (0);
                else
                    break;
            }
        }
    }
"#;

#[test]
fn emits_the_exact_gc_1_3_inline_save_qsort_schedule() {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.use_lmw_stmw = true;
    flags.char_default = mwcc_versions::CharDefault::Signed;
    let object = compile(
        SOURCE,
        "qsort.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_3,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the Metroid Prime qsort variant should compile");

    let prologue = [
        0x94, 0x21, 0xff, 0xc0, // stwu r1,-64(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x28, 0x04, 0x00, 0x02, // cmplwi r4,2
        0x90, 0x01, 0x00, 0x44, // stw r0,68(r1)
        0xbe, 0xa1, 0x00, 0x14, // stmw r21,20(r1)
    ];
    let older_heap_scaling = [
        0x38, 0x00, 0x00, 0x02, // li r0,2
        0x7e, 0xd7, 0xb3, 0x78, // mr r23,r22
        0x7f, 0x5a, 0x01, 0xd6, // mullw r26,r26,r0
    ];
    let epilogue = [
        0xba, 0xa1, 0x00, 0x14, // lmw r21,20(r1)
        0x80, 0x01, 0x00, 0x44, // lwz r0,68(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x40, // addi r1,r1,64
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(prologue.len())
        .any(|bytes| bytes == prologue));
    assert!(object
        .windows(older_heap_scaling.len())
        .any(|bytes| bytes == older_heap_scaling));
    assert!(object
        .windows(epilogue.len())
        .any(|bytes| bytes == epilogue));
}
