// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -O4,s

typedef short s16;

typedef struct Effect {
    s16 timer;
    char reserved[74];
    s16 phase;
} Effect;

void narrow_member_initialization(Effect *effect, void *unused, void *source) {
    effect->phase = *(s16 *)source + 16384;
    effect->timer = 60;
}
