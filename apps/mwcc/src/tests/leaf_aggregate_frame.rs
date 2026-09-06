use crate::{compile, SourceLanguage};

fn executable_words(object: &[u8]) -> Vec<u32> {
    let word = |offset| u32::from_be_bytes(object[offset..offset + 4].try_into().unwrap());
    let half = |offset| u16::from_be_bytes(object[offset..offset + 2].try_into().unwrap());
    let table = word(32) as usize;
    let stride = half(46) as usize;
    (0..half(48) as usize)
        .filter_map(|index| {
            let section = table + index * stride;
            (word(section + 8) & 4 != 0).then(|| {
                let start = word(section + 16) as usize;
                let end = start + word(section + 20) as usize;
                object[start..end].chunks_exact(4)
                    .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
            })
        })
        .flatten()
        .collect()
}

#[test]
fn frame_resident_aggregate_leaf_drops_linkage_but_preserves_its_stack() {
    use mwcc_versions::*;
    for build in [GC_1_1, GC_1_1P1, GC_1_2_5, GC_1_2_5N, GC_1_3_2, GC_2_6] {
        let mut config = CompilerConfig::new(build);
        config.flags.debug_info = false;
        config.flags.cpp_exceptions = false;
        config.flags.emit_mwcats = false;
        let object = compile(
            include_bytes!("../../../../canaries/1535_leaf_aggregate_frame.c"),
            "leaf_aggregate_frame.c", config, Some(SourceLanguage::C), None, false,
        ).unwrap_or_else(|diagnostic| panic!("{}: {diagnostic:?}", build.label));
        let words = executable_words(&object);
        // The aggregate still occupies a real frame, even though a leaf never
        // needs to save or restore LR. Check the balanced stack and both local
        // accesses so cleanup cannot silently delete required frame storage.
        let allocation = words.iter().find(|word| **word & 0xffff0000 == 0x94210000)
            .expect("volatile aggregate needs stack storage");
        let frame = -(*allocation as i16);
        assert_eq!(words[words.len() - 2], 0x38210000 | frame as u32);
        assert_eq!(words.last(), Some(&0x4e800020));
        assert!(!words.contains(&0x7c0802a6)); // mflr r0
        assert!(!words.contains(&0x7c0803a6)); // mtlr r0
        assert!(words.iter().any(|word| word & 0xfc1f0000 == 0xd0010000)); // stfs to r1
        assert!(words.iter().any(|word| word & 0xfc1f0000 == 0xc0010000)); // lfs from r1
    }
}
