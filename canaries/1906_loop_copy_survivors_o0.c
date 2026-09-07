// flags: -lang c
// A mutable copy must preserve the original word through nested control flow.
unsigned returned_source(unsigned s) {
    unsigned a; a = s;
    if (a > 128 && a < 256) { while (a % 2 == 0) a /= 2; }
    return a + s;
}
unsigned subsequent_source(unsigned s) {
    unsigned a; unsigned b; a = s;
    if (a > 128 && a < 256) { while (a % 2 == 0) a /= 2; }
    b = s + 1;
    return a + b;
}
unsigned nested_copy(unsigned s, unsigned test) {
    unsigned a; a = s;
    if (test) { if (a > 128 && a < 256) { while (a % 2 == 0) a /= 2; } }
    return a + s;
}
unsigned conditional_copy(unsigned s, unsigned test) {
    unsigned a = 0; unsigned b;
    if (test) {
        a = s;
        if (a > 128 && a < 256) { while (a % 2 == 0) a /= 2; }
    }
    b = s + 1;
    return a + b;
}
unsigned chained_copy(unsigned s) {
    unsigned copy; unsigned a;
    copy = s; a = copy;
    if (a > 128 && a < 256) { while (a % 2 == 0) a /= 2; }
    return a + s;
}
