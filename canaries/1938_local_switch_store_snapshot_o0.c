// flags: -Cpp_exceptions off -pragma "cats off"
struct Mode { unsigned bits; };
extern struct Mode *mode;
void remap(unsigned *out) { unsigned key = (mode->bits >> 14) & 3; switch (key) { case 1: *out = 2; break; case 2: *out = 1; break; default: *out = key; break; } }
void local_remap(struct Mode *p, unsigned *out) { unsigned key = (p->bits >> 14) & 3; switch (key) { case 1: *out = 2; break; case 2: *out = 1; break; default: *out = key; break; } }
