// flags: -Cpp_exceptions off -pragma "cats off"
extern void (*single_callback)(float value);
extern void (*double_callback)(double value);

void invoke_single_callback(void) {
    single_callback(1);
}

void invoke_double_callback(void) {
    double_callback(-2);
}
