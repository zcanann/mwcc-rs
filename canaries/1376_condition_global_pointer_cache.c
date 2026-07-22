// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Limits {
    float minimum;
    int age;
} Limits;

typedef struct Sample {
    float value;
    unsigned char age;
} Sample;

extern Limits* limits;
extern void accept_sample(Sample*);
extern void observe_sample(Sample*);

void condition_global_pointer_cache(Sample* sample)
{
    if (sample->value >= limits->minimum && sample->age < limits->age) {
        accept_sample(sample);
    }
    observe_sample(sample);
}
