// A comparison reads the global aggregate field into r0, so the aggregate
// address and scaled index need distinct GPRs. The signed long long tail also
// pins the PowerPC EABI layout at a 32-byte stride (`slwi ...,5`).
// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n
typedef signed long long int Time;
struct Packet {
    int channel;
    void* output;
    unsigned output_bytes;
    void* input;
    unsigned input_bytes;
    void (*callback)(int, unsigned, void*);
    Time fire;
};
struct Control {
    int channel;
    int words[4];
};
struct Packet packet_table[4];
struct Control control;

int global_struct_scratch(int index) {
    return packet_table[index].channel != -1 || control.channel == index;
}
