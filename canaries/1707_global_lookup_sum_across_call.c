// Lookup sum using tables shared across a call; tests section-anchor composition.
// Candidate execution probe; fresh compiler references pending.
// flags: -Cpp_exceptions off -pragma "cats off"
static unsigned a[128] = {1}, b[128] = {2}, c[128] = {3};
extern void mutate(unsigned*, unsigned*, unsigned*);
unsigned across_call(unsigned i) {
    unsigned first = a[i & 3];
    mutate(a, b, c);
    return first + a[(i >> 2) & 3] + b[(i >> 4) & 3] + c[(i >> 6) & 3] + 13;
}
