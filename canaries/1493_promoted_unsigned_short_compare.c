// A 16-bit unsigned operand promotes to signed int before it is compared with
// an int on 32-bit PowerPC. The relational compare must therefore use `cmpw`,
// not `cmplw`.
// builds: GC/1.2.5n

extern void accepted(void);

struct State {
    unsigned short value;
};

void promoted_unsigned_short_compare(struct State* state, int limit)
{
    if (state->value >= limit) {
        accepted();
    }
}
