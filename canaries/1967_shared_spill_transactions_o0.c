// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned current_word;
typedef void (*Callback)(void);
extern Callback current_callback;
extern int enter(void);
extern void leave(int);
extern void first(unsigned);
extern void last(unsigned);
unsigned swap_word(unsigned replacement) {
    unsigned old = current_word;
    int token = enter();
    current_word = replacement;
    leave(token);
    return old;
}
Callback swap_callback(Callback replacement) {
    Callback old = current_callback;
    int token = enter();
    current_callback = replacement;
    leave(token);
    return old;
}
void computed_store(unsigned* out, unsigned value) {
    unsigned copy = value + 7;
    *out = copy;
    first(copy);
    last(value + 11);
}
void signed_computed_store(int* out, int value) {
    int copy = value - 5;
    *out = copy;
    first(copy);
    last(value + 2);
}
