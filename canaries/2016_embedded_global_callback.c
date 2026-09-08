// flags: -Cpp_exceptions off -pragma "cats off"
void (*callback)(void);
unsigned state;
extern void begin(unsigned value);
extern void end(unsigned value);
void run(unsigned value) {
    begin(value);
    if (callback) callback();
    end(value);
}
void finish(void) {
    if (callback) callback();
    state += 1;
}
void repeat(void) {
    if (callback) callback();
    if (callback) callback();
}
