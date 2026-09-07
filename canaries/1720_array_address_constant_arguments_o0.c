// Candidate execution probe; fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
struct Packet { unsigned data[61]; };
extern struct Packet packets[64];
extern unsigned words[64];
extern unsigned char bytes[4];
extern void consume(void*, unsigned);
void pass_packets(void) { consume(packets, sizeof(packets)); }
void pass_words(void) { consume(words, sizeof(words)); }
void pass_bytes(void) { consume(bytes, sizeof(bytes)); }
