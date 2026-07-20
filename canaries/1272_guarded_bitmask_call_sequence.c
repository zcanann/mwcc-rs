// A guarded SDK dirty-state dispatcher shares its saved mask with a subsequent flush guard, then
// writes three call-surviving parameters to the same memory-mapped FIFO. This is the older Dolphin
// GXBegin allocation/scheduling family, expressed without depending on SDK names.
// builds: GC/1.2.5n
typedef struct PortState {
    unsigned words[317];
    unsigned dirty;
} PortState;

extern PortState* state;
extern void update_a(void);
extern void update_b(void);
extern void update_c(void);
extern void flush_port(void);

void guarded_bitmask_call_sequence(int type, int format, unsigned short count) {
    PortState* data = state;
    unsigned flags = data->dirty;
    if (data->dirty != 0) {
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
    if (*(unsigned*)state == 0) {
        flush_port();
    }
    *(unsigned char*)0xCC008000 = format | type;
    *(unsigned short*)0xCC008000 = count;
}
