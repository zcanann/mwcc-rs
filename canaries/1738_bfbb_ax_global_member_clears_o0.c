// Candidate execution probe; fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
// AX's final inactive-voice sweep stores through a 244-byte global element stride.
struct Packet { unsigned short pad[7]; unsigned short state; unsigned short gap[26]; unsigned short updates[5]; unsigned short tail[83]; };
struct Voice { struct Voice* next; unsigned pad[5]; unsigned index; };
extern struct Packet packets[64];
extern void pulse(void);
void clear_walk(struct Voice* voice) { for (; voice; voice = voice->next) { pulse(); packets[voice->index].state = packets[voice->index].updates[0] = packets[voice->index].updates[1] = packets[voice->index].updates[2] = packets[voice->index].updates[3] = packets[voice->index].updates[4] = 0; } }
