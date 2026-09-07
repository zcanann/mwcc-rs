// Candidate execution probe; fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
// __AXDumpVPB's guarded depop, chained clears, and callback in a mutable list walk.
struct Packet { unsigned short pad[7]; unsigned short state; unsigned short gap[26]; unsigned short updates[5]; unsigned short tail[83]; };
struct Voice { struct Voice* next; unsigned pad[5]; unsigned index; unsigned gap[71]; struct Packet pb; };
extern struct Packet packets[64];
extern void depop(struct Packet*);
extern void push(struct Voice*);
inline void dump(struct Voice* voice) {
    struct Packet* dsp;
    dsp = &packets[voice->index];
    if (dsp->state == 1) { depop(dsp); }
    voice->pb.state = dsp->state = dsp->updates[0] = dsp->updates[1] = dsp->updates[2] = dsp->updates[3] = dsp->updates[4] = 0;
    push(voice);
}
void walk(struct Voice* voice) { for (; voice; voice = voice->next) { dump(voice); voice->index = 0; } }
