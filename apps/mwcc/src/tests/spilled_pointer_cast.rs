use crate::{compile, SourceLanguage};

#[test]
fn reloads_an_address_taken_pointer_beneath_a_cast() {
    let source = br#"
        extern int acquire(void** output);

        int compiled(void) {
            void* node;
            if (!acquire(&node)) {
                return 0;
            }
            *(void**)node = 0;
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    compile(
        source,
        "spilled-pointer-cast.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an address-taken pointer should reload before its casted dereference");
}
