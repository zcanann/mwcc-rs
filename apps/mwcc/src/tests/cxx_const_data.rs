use crate::{compile, SourceLanguage};

fn be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn c_string(bytes: &[u8], offset: usize) -> &str {
    let length = bytes[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap();
    std::str::from_utf8(&bytes[offset..offset + length]).unwrap()
}

fn symbols(object: &[u8]) -> Vec<(String, String, u32, u8)> {
    let section_headers = be_u32(object, 32) as usize;
    let section_size = be_u16(object, 46) as usize;
    let section_count = be_u16(object, 48) as usize;
    let section_name_index = be_u16(object, 50) as usize;
    let section_header = |index: usize| section_headers + index * section_size;
    let section_names_offset =
        be_u32(object, section_header(section_name_index) + 16) as usize;
    let section_names = (0..section_count)
        .map(|index| {
            let name = be_u32(object, section_header(index)) as usize;
            c_string(object, section_names_offset + name).to_owned()
        })
        .collect::<Vec<_>>();
    let symtab_index = (0..section_count)
        .find(|index| be_u32(object, section_header(*index) + 4) == 2)
        .unwrap();
    let symtab = section_header(symtab_index);
    let symtab_offset = be_u32(object, symtab + 16) as usize;
    let symtab_size = be_u32(object, symtab + 20) as usize;
    let strings_index = be_u32(object, symtab + 24) as usize;
    let strings_offset = be_u32(object, section_header(strings_index) + 16) as usize;

    (0..symtab_size / 16)
        .filter_map(|index| {
            let symbol = symtab_offset + index * 16;
            let name = c_string(
                object,
                strings_offset + be_u32(object, symbol) as usize,
            );
            let section = be_u16(object, symbol + 14) as usize;
            (section < section_names.len()).then(|| {
                (
                    name.to_owned(),
                    section_names[section].clone(),
                    be_u32(object, symbol + 4),
                    object[symbol + 12] >> 4,
                )
            })
        })
        .collect()
}

fn config() -> mwcc_versions::CompilerConfig {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    flags.string_literals_read_only = false;
    mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    }
}

#[test]
fn external_cxx_const_follows_earlier_function_strings_into_sdata() {
    let source = br#"
        struct Values {
            static const float value;
        };
        extern void sink(const char*);
        void first() { sink("name"); }
        const float Values::value = 1.25f;
    "#;
    let object = compile(
        source,
        "external-cxx-const.cpp",
        config(),
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .unwrap();
    let symbols = symbols(&object);
    let string = symbols
        .iter()
        .find(|(name, _, _, _)| name.starts_with('@'))
        .unwrap();
    let value = symbols
        .iter()
        .find(|(name, _, _, _)| name == "value__6Values")
        .unwrap();
    let sink_position = symbols
        .iter()
        .position(|(name, _, _, _)| name.starts_with("sink"))
        .unwrap();
    let value_position = symbols
        .iter()
        .position(|(name, _, _, _)| name == "value__6Values")
        .unwrap();

    assert_eq!(string.1, ".sdata");
    assert_eq!(string.2, 0);
    assert_eq!(value.1, ".sdata");
    assert_eq!(value.2, 8);
    assert!(sink_position < value_position);
}

#[test]
fn external_c_const_remains_read_only() {
    let object = compile(
        b"const float value = 1.25f;",
        "external-c-const.c",
        config(),
        Some(SourceLanguage::C),
        None,
        false,
    )
    .unwrap();
    let value = symbols(&object)
        .into_iter()
        .find(|(name, _, _, _)| name == "value")
        .unwrap();

    assert_eq!(value.1, ".sdata2");
}

#[test]
fn template_inline_assertion_strings_keep_the_weak_owner_symbol() {
    let source = br#"
        namespace api {
            namespace report {
                void Panic(const char*, int, const char*, ...);
            }
            class Node {
            public:
                Node* GetNext() const { return next; }
            private:
                Node* next;
            };
            class ListBase {
            public:
                class Iterator {
                public:
                    Node* operator->() const { return node; }
                    Iterator& operator++() {
                        node = node->GetNext();
                        return *this;
                    }
                    friend bool operator==(Iterator lhs, Iterator rhs) {
                        return lhs.node == rhs.node;
                    }
                private:
                    Node* node;
                };
            };
            template <typename T, int Offset>
            class List : public ListBase {
            public:
                class Iterator {
                public:
                    T* operator->() const { return GetPointer(iterator.operator->()); }
                    Iterator& operator++() {
                        ++iterator;
                        return *this;
                    }
                    Iterator operator++(int) {
                        Iterator old = *this;
                        ++*this;
                        return old;
                    }
                    friend bool operator==(Iterator lhs, Iterator rhs) {
                        return lhs.iterator == rhs.iterator;
                    }
                    friend bool operator!=(Iterator lhs, Iterator rhs) {
                        return !(lhs == rhs);
                    }
                private:
                    ListBase::Iterator iterator;
                };
                static T* GetPointer(Node* pointer) {
                    (void)(((pointer != 0))
                        || (api::report::Panic("List.h", 73, "null pointer"), 0));
                    return reinterpret_cast<T*>(
                        reinterpret_cast<char*>(pointer) - Offset);
                }
            };
        }
        namespace client {
            struct Entry {
                virtual void apply();
                int link;
            };
            typedef api::List<Entry, 0> EntryList;
            void read(EntryList::Iterator iterator, EntryList::Iterator end) {
                for (; iterator != end; iterator++) {
                    iterator->apply();
                }
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    let object = compile(
        source,
        "inline-template-strings.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_3_0A3,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the source-proven iterator assertion should compile");

    let base = "@STRING@GetPointer__Q23api23List<Q26client5Entry,0>FPQ23api4Node";
    let symbols = symbols(&object);
    let first = symbols
        .iter()
        .find(|(name, _, _, _)| name == base)
        .expect("first inline-owned string");
    let second = symbols
        .iter()
        .find(|(name, _, _, _)| name == &format!("{base}@0"))
        .expect("second inline-owned string");
    assert_eq!(first.1, ".sdata");
    assert_eq!(second.1, ".data");
    assert_eq!(first.3, 2);
    assert_eq!(second.3, 2);
}
