// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct One { unsigned a; } One;
typedef struct Two { unsigned a; unsigned b; } Two;
typedef struct Three { unsigned a; unsigned b; unsigned short c; } Three;
typedef struct Packet { unsigned prefix; One one; Two two; Three three; unsigned suffix; } Packet;
void copy_one(Packet* packet, const One* source) { packet->one = *source; }
void copy_two(Packet* packet, const Two* source) { packet->two = *source; }
void copy_three(Packet* packet, const Three* source) { packet->three = *source; }
