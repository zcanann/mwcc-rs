// A 64-bit result occupies r3:r4 (high:low). Probe every currently modeled
// source that has to construct that pair: scalar widening, a non-result
// parameter pair, memory through a pointer/member, and a global pair load.
// This distinguishes a result-lane copy convention from syntax-specific moves.
struct LLBox {
    int pad;
    long long value;
};

extern long long ll_global;

long long ll_widen_signed(int value) { return value; }
unsigned long long ll_widen_unsigned(unsigned value) { return value; }
long long ll_return_second(int ignored, long long value) { return value; }
long long ll_dereference(long long *value) { return *value; }
long long ll_member(struct LLBox *box) { return box->value; }
long long ll_read_global(void) { return ll_global; }
long long ll_mask_byte(long long value) { return value & 0xff; }
