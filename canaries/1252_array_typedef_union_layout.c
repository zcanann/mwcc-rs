typedef long Mtx_t[4][4];

typedef union {
    Mtx_t m;
    long long force_alignment;
} Mtx;

typedef struct {
    unsigned char prefix[16];
    Mtx first;
    Mtx second;
    unsigned short tail;
} Demo;

int array_typedef_union_size(void) { return sizeof(Demo); }
// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
