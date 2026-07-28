use crate::{compile, SourceLanguage};

#[test]
fn emits_internal_const_template_value_images_after_an_extern_declaration() {
    let source = br#"
        typedef float F32;
        template <class T> struct basic_rect {
            T x, y, w, h;
            const static basic_rect m_Null;
            basic_rect& assign(T x, T y, T w, T h);
        };
        extern const basic_rect<F32> screen_bounds;
        const basic_rect<F32> screen_bounds = { 0.0f, 0.0f, 1.0f, 1.0f };
        const basic_rect<F32> default_adjust = { 0.0f, 0.0f, 1.0f, 1.0f };
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_read_only = true;
    let object = compile(
        source,
        "const-template-globals.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("const template-value globals should remain materialized");

    let image = [
        0, 0, 0, 0, 0, 0, 0, 0, 0x3f, 0x80, 0, 0, 0x3f, 0x80, 0, 0,
    ];
    assert_eq!(
        object
            .windows(image.len())
            .filter(|window| *window == image)
            .count(),
        2
    );
}
