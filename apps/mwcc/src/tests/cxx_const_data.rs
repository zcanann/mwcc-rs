use super::elf_object::symbols;
use crate::{compile, SourceLanguage};

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
