// A non-leaf `if (c) { ...; ...; }` with a multi-statement straight-line body:
// the condition test is scheduled into the prologue, then a forward branch skips
// all the body statements (calls/stores) when false — the same shape as the
// single-statement case, generalized. A value read across one of the body's calls
// would need callee-saving, so that defers instead.
void ifmulti_g(void);
void ifmulti_h(void);
extern int ifmulti_flag;
void ifmulti(int a) {
    if (a) {
        ifmulti_flag = 1;
        ifmulti_g();
        ifmulti_h();
    }
}
