// flags: -Cpp_exceptions off -pragma "cats off"
extern void observe(unsigned);
inline void macro_update(unsigned* out, unsigned value) {
    do {
        *out = value;
        if (value & 1) *out = *out + 7;
    } while (0);
    observe(value);
}
inline void nested_macro(unsigned* out, unsigned value) {
    unsigned copy;
    do {
        copy = value + 3;
        do { *out = copy; } while (0);
    } while (0);
    observe(copy);
}
void updated(unsigned* out, unsigned value) { macro_update(out, value); observe(value + 1); }
void nested(unsigned* out, unsigned value) { nested_macro(out, value); observe(value + 1); }
