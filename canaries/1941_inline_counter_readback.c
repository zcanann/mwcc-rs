// flags: -Cpp_exceptions off -pragma "cats off"
// GXInit reads a split hardware counter until the sampled upper half is stable.
extern volatile unsigned short *registers;
static inline unsigned counter(unsigned low, unsigned high) {
    unsigned old_high, new_high, low_value;
    old_high = registers[high];
    do {
        new_high = old_high;
        low_value = registers[low];
        old_high = registers[high];
    } while (old_high != new_high);
    return (old_high << 16) | low_value;
}
unsigned read_counter(void) { unsigned result; result = counter(0x28, 0x27); return result; }
unsigned read_dynamic(unsigned low, unsigned high) { unsigned result; result = counter(low, high); return result; }
unsigned read_twice(unsigned *out) { unsigned first, second; first = counter(0x28, 0x27); *out = first; second = counter(0x28, 0x27); return second; }
unsigned read_guarded(unsigned enabled) { unsigned result = 0; if (enabled) { result = counter(0x28, 0x27); } return result; }
