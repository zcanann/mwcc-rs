// MWCC's absolute-address object syntax carries different scheduling provenance from an explicit
// integer-to-pointer cast. Build 163 hoists the state pointer before materializing the fixed base.
// builds: GC/1.2.5n
typedef struct FixedObjectState {
    unsigned short dirty;
    unsigned char padding[514];
    unsigned value;
} FixedObjectState;

typedef union FixedObjectPipe {
    unsigned char u8;
    unsigned u32;
    double f64;
} FixedObjectPipe;

extern FixedObjectState* const state;
volatile FixedObjectPipe PORT : (0xCC008000);

void fixed_address_object_flush(void) {
    PORT.u8 = 0x61;
    PORT.u32 = state->value;
    state->dirty = 0;
}
