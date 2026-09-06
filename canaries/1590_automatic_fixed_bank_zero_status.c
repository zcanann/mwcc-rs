// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
static int report_zero(void) {
    registers[10] &= 0x405;
    return 0;
}
int false_guard(void) {
    if (report_zero()) return 9;
    return 3;
}
int true_guard(void) {
    if (!report_zero()) return 3;
    return 9;
}
int keep_accumulator(int error) {
    if (report_zero()) error = 0;
    return error;
}
