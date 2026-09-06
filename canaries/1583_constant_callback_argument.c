// flags: -Cpp_exceptions off -pragma "cats off"
// Borrow the unfinished integer argument register for a function address.
typedef void (*Callback)(int, void*);
void handler(int, void*);
void install(int, Callback);
void finish(void);

void register_one(void) {
    install(25, handler);
    finish();
}
void register_negative(void) {
    install(-7, handler);
    finish();
}
void register_tail(void) {
    install(3, handler);
}
