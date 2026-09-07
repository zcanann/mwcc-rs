// Candidate frame-growth execution probe; fresh reference objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Packet { unsigned short pad[7]; unsigned short state; unsigned short gap[26]; unsigned short updates[5]; unsigned short tail[83]; };
extern struct Packet packets[64];
extern void pulse(void);
extern void fill(unsigned*, unsigned);
extern unsigned consume(unsigned*);
unsigned clear_local(unsigned index) { unsigned local[4]; fill(local, index); packets[index].state = packets[index].updates[0] = packets[index].updates[1] = packets[index].updates[2] = packets[index].updates[3] = packets[index].updates[4] = 0; return consume(local); }
