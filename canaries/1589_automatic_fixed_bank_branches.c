// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
volatile u32 registers[32] : 0xCC006800;
u32 next_channel(void);
static int choose(u32 channel) {
    u32 value = registers[10];
    value &= 0x405;
    value |= 0x40 | (channel << 3);
    registers[10] = value;
    return 7;
}
static int release(void) {
    registers[10] &= 0x405;
    return 1;
}
int branch_true(u32 channel) {
    int error = 0;
    if (choose(channel)) error |= !release();
    else return 0;
    return !error;
}
int branch_false(u32 channel) {
    if (!choose(channel)) release();
    return 5;
}
void effectful_argument(void) {
    if (choose(next_channel())) release();
}
