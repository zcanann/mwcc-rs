// A non-leaf `if (c) { then } else { else }` with straight-line bodies: the
// condition test schedules into the prologue, `beq` jumps to the else body, the
// then body falls through an unconditional `b` over the else to the shared
// epilogue. The else branch + join advance the anonymous-`@N` counter by 3.
void ifelse_g(void);
void ifelse_h(void);
extern int ifelse_a;
extern int ifelse_b;
void ifelse(int c) {
    if (c) {
        ifelse_a = 1;
        ifelse_g();
    } else {
        ifelse_b = 2;
        ifelse_h();
    }
}
