// Frame-growth regression probe; fresh reference-compiler objects pending.
// Regresses saved-register capacity growth and restored-stack LR slot identity.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
// AX's final inactive-voice sweep stores through a 244-byte global element stride.
struct Packet { unsigned short pad[7]; unsigned short state; unsigned short gap[26]; unsigned short updates[5]; unsigned short tail[83]; };
struct Voice { struct Voice* next; unsigned pad[5]; unsigned index; };
extern struct Packet packets[64];
extern void pulse(void);
void clear_index(unsigned index) { pulse(); packets[index].state = packets[index].updates[0] = packets[index].updates[1] = packets[index].updates[2] = packets[index].updates[3] = packets[index].updates[4] = 0; }
