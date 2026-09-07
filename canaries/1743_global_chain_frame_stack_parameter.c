// Incoming-argument regression probe; fresh reference objects pending.
// Known failure: the ninth integer parameter is read from r11, not the caller stack.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Packet { unsigned short pad[7]; unsigned short state; unsigned short gap[26]; unsigned short updates[5]; unsigned short tail[83]; };
extern struct Packet packets[64];
extern void pulse(void);
unsigned clear_stack(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f, unsigned g, unsigned h, unsigned index) { pulse(); packets[index].state = packets[index].updates[0] = packets[index].updates[1] = packets[index].updates[2] = packets[index].updates[3] = packets[index].updates[4] = 0; return index; }
