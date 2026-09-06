// flags: -Cpp_exceptions off -pragma "cats off"
extern void (*callback)(int tag, ...);
extern void (*fixed_float_callback)(float value, ...);

void invoke_integer_arguments(void) {
    callback(3, 7);
}

void invoke_floating_argument(void) {
    callback(3, 1.1);
}

void invoke_fixed_float_parameter(void) {
    fixed_float_callback(1, 7);
}
