// flags: -Cpp_exceptions off -pragma "cats off"
unsigned heads[32], tails[32], selected;
void compare_entries(unsigned i) {
    if (heads[i] == tails[i]) selected = 1;
    else selected = 2;
}
unsigned choose_entry(unsigned i) {
    unsigned p;
    p = 0;
    if (heads[i]) {
        if (heads[i] == tails[i]) p = heads[i];
        else p = tails[i];
    }
    return p;
}
void compare_indirect(unsigned *left, unsigned *right) {
    if (*left != *right) selected = 3;
    else selected = 4;
}
