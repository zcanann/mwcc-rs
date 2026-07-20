typedef struct {
    unsigned char head;
    union {
        unsigned char raw;
        signed char signed_raw;
    } flags;
    union {
        int words[3];
        double force_alignment;
    } payload;
    unsigned short tail;
} Packet;

int named_inline_union_size(void) { return sizeof(Packet); }
unsigned char named_inline_union_read(Packet* packet) { return packet->flags.raw; }
// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
