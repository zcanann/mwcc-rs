// A memory-loaded SDK dirty mask survives a chain of conditional calls in r31. Each contiguous
// mask is tested with one recorded rotate/mask, and the final clear reloads the owning global
// pointer only after the calls. This is Dolphin GX's `__GXSetDirtyState` control-flow family.
// builds: GC/1.2.5n
typedef struct DirtyState {
    unsigned padding[317];
    unsigned dirty;
} DirtyState;

extern DirtyState* state;
extern void update_a(void);
extern void update_b(void);
extern void update_c(void);

void saved_bitmask_call_chain(void) {
    unsigned flags = state->dirty;
    if (flags & 1) {
        update_a();
    }
    if (flags & 2) {
        update_b();
    }
    if (flags & 24) {
        update_c();
    }
    state->dirty = 0;
}
