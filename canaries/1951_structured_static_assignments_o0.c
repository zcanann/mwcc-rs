// flags: -Cpp_exceptions off -pragma "cats off"
extern void observe(unsigned);
extern unsigned next_value(unsigned);
unsigned register_once(unsigned token) {
    static unsigned registered;
    observe(token);
    if (!registered) { observe(token + 1); registered = 1; }
    observe(token + 2);
    return registered;
}
unsigned loop_static(unsigned token) {
    static unsigned total;
    unsigned i;
    for (i = 0; i < 4; ++i) {
        if (token & (1u << i)) total = next_value(total + i);
        observe(token + i);
    }
    return total;
}
extern unsigned shadowed;
unsigned parameter_shadow(unsigned shadowed) {
    shadowed = next_value(shadowed);
    observe(shadowed);
    return shadowed;
}
unsigned local_shadow(unsigned token) {
    unsigned shadowed = next_value(token);
    shadowed = next_value(shadowed + 1);
    observe(shadowed);
    return shadowed;
}
