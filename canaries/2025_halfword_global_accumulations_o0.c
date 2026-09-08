// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Packet { short a, b, c; unsigned short u, v; };
int first, second, third;
unsigned int fourth, fifth;
void accumulate(struct Packet* packet) {
    first += packet->a;
    second += packet->b;
    third += packet->c;
}
void accumulate_unsigned(struct Packet* packet) {
    fourth += packet->u;
    fifth += packet->v;
}
