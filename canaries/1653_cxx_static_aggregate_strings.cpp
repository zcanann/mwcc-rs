// C++ array/record initializers retain constant data even on guarded-scalar builds.
// flags: -Cpp_exceptions off -pragma "cats off"
char** array_text() {
    static char* p[2] = { "abc", "long literal" };
    return p;
}
struct Entry { char* text; int tag; };
Entry* record_text() {
    static Entry p = { "record text", 1 };
    return &p;
}
