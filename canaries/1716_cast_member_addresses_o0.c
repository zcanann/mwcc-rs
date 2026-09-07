// Candidate execution probe; fresh compiler-reference objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
// AX service copies word values through cast addresses of nested u16 members.
struct Packet { unsigned short pad[57]; unsigned short field; unsigned short rest[8]; };
struct Voice { unsigned pad[78]; struct Packet packet; };
void copy_cast(struct Packet* dst, struct Voice* src) {
    *(unsigned*)&dst->field = *(unsigned*)&src->packet.field;
}
unsigned load_cast(struct Voice* src) { return *(unsigned*)&src->packet.field; }
void store_cast(struct Voice* dst, unsigned value) { *(unsigned*)&dst->packet.field = value; }
unsigned short load_array(struct Voice* src) { return *(unsigned short*)&src->packet.rest; }
