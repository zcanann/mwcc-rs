// builds: GC/1.2.5n
// A bit-field assignment is one read-modify-write of its containing unit. The
// low source bits rotate into the field while every neighboring bit is retained.
struct BitFieldStore {
    unsigned char prefix : 2;
    unsigned char flag : 1;
    unsigned char suffix : 5;
};

void bit_field_clear(struct BitFieldStore* state) { state->flag = 0; }
void bit_field_set(struct BitFieldStore* state) { state->flag = 1; }
void bit_field_assign(struct BitFieldStore* state, unsigned value)
{
    state->flag = value;
}
