// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

// A later member comparison overlaps a structured floating-point local.  Both
// values belong in the FPR allocator: pinning the member load to f1 forces an
// avoidable fmr before the call.

typedef struct Sample {
    float value;
} Sample;

extern void consume(Sample*, float);

void float_compare_temp_allocation(Sample* sample)
{
    float value = sample->value;
    if (value < 0) {
        value = -value;
    }
    if (sample->value < 0) {
        consume(sample, value);
    }
}
