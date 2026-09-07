// Constant local initializers consume fresh literal identities in declaration order.
// flags: -Cpp_exceptions off -pragma "cats off"
char* multiple(void) {
    static char* p = "abc";
    static char* q = "long literal";
    return q;
}
char* reused(void) {
    static char* p = "shared text";
    static char* q = "shared text";
    return q;
}
char* scalar_before(void) {
    static int n = 1;
    static char* p = "before";
    return p;
}
char* scalar_after(void) {
    static char* p = "after";
    static int n = 1;
    return p;
}
