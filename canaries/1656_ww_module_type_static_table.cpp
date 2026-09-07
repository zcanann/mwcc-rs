// Reduced from Wind Waker src/DynamicLink.cpp: original getter body.
// Minimal class declaration preserves mResourceType's documented 0x20 offset.
// flags: -Cpp_exceptions off -pragma "cats off"
struct DynamicModuleControl {
    unsigned char prefix[32];
    unsigned char mResourceType;
    const char* getModuleTypeString() const;
};
const char* DynamicModuleControl::getModuleTypeString() const {
    static const char* strings[4] = {"????", "MEM", "ARAM", "DVD"};
    return strings[mResourceType & 3];
}
