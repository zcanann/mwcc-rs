// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Sample {
    float value;
} Sample;

typedef struct Wrapper {
    Sample* sample;
} Wrapper;

int dependent_float_literal_schedule(Wrapper* wrapper)
{
    if (wrapper->sample->value < 0) {
        return 1;
    }
    return 0;
}
