// A two-bit enum permutation is inserted into one SDK state word before setting a separate dirty
// bit. Build 163 keeps the state base in r4 and forms the permutation with slwi/rlwimi in r6.
// builds: GC/1.2.5n
typedef struct EnumState {
    unsigned padding_a[129];
    unsigned value;
    unsigned padding_b[187];
    unsigned dirty;
} EnumState;

extern EnumState* const state;

typedef enum RemapMode {
    REMAP_NONE,
    REMAP_FRONT,
    REMAP_BACK,
    REMAP_ALL,
} RemapMode;

void enum_remap_member_update(RemapMode mode) {
    EnumState* data;
    RemapMode remapped;
    data = state;
    remapped = (mode >> 1) & 1;
    remapped = (remapped & -3) | ((int)mode << 1);
    data->value = (data->value & -49153) | ((int)remapped << 14);
    data->dirty = data->dirty | 4;
}
