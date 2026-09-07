// flags: -Cpp_exceptions off -pragma "cats off"
extern void* current;
extern int enter(void);
extern void leave(int);
extern void observe(unsigned);
void* protected_swap(void* replacement) {
    void* old = current;
    int token = enter();
    current = replacement;
    leave(token);
    return old;
}
void* snapshot_call(void) { void* old = current; observe(1); return old; }
void* direct_swap(void* replacement) { void* old = current; current = replacement; return old; }
