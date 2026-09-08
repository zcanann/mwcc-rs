// flags: -Cpp_exceptions off -pragma "cats off" -O1
int input;
volatile int observed;
extern void mutate(void);
void clamped(int *out) {
    int q;
    q = input / 160;
    if (q) {
        if (q > 20) q = 20;
        if (q < -20) q = -20;
        *out = input;
        input -= q * 160;
    } else *out = 0;
}
void local_join(int *out, int flag) {
    int q;
    q = input / 160;
    if (q) {
        if (flag) q = 7;
        *out = input + q;
    } else *out = 0;
}
void store_join(int *out, int flag) {
    int q;
    q = input / 160;
    if (q) {
        if (flag) *out = 19;
        *out = input;
    } else *out = 0;
}
void changed(int *out, int flag) {
    int q;
    q = input / 160;
    if (q) {
        if (flag) input = 23;
        *out = input;
    } else *out = 0;
}
void called(int *out) {
    int q;
    q = input / 160;
    if (q) { mutate(); *out = input; } else *out = 0;
}
void volatile_source(int *out) {
    int q;
    q = observed / 160;
    if (q) *out = observed; else *out = 0;
}
void straight(int *out) {
    int q;
    q = input / 160;
    *out = input + q;
}
