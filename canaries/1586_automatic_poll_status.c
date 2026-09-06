// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
void submit(u32);
static int wait_ready(void) {
    while (registers[13] & 1) {}
    return 1;
}
void wait_only(void) { wait_ready(); }
int wait_result(void) { return wait_ready(); }
void wait_then_submit(u32 value) { wait_ready(); submit(value); }
int accumulate_wait(int error) { error |= !wait_ready(); return error; }
