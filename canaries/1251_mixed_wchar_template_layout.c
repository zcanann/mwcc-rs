#pragma cplusplus on

typedef unsigned int uint;

template <typename T, typename Traits = int, typename Alloc = int>
class Box {
    struct Metadata {
        uint capacity;
    };
    const T* data;
    Metadata* metadata;
    uint size;
    uint padding;
    void ignored(int);
};

typedef Box<wchar_t> WideBox;

struct Text {
    void value(wchar_t);
    void pointer(wchar_t*);
};

void Text::value(wchar_t) {}
void Text::pointer(wchar_t*) {}

#pragma cplusplus off

int wide_box_size(void) { return sizeof(WideBox); }
// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
