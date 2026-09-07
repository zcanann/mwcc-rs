// flags: -Cpp_exceptions off -pragma "cats off"
// A pair of saved values is published to an addressable packet after a call.
extern void barrier(void);
extern int consume(const int*);

int reverse_packet(int first, int second) {
    int packet[2];
    barrier();
    packet[1] = first;
    packet[0] = second;
    return consume(packet);
}

int forward_packet(int first, int second) {
    int packet[2];
    barrier();
    packet[1] = second;
    packet[0] = first;
    return consume(packet);
}
