// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Limits {
    int timer_base;
    float timer_slop;
    float minimum;
} Limits;

typedef struct Sample {
    float stick;
    unsigned char timer;
} Sample;

extern Limits* limits;
extern void accept_sample(Sample*, float);
extern void observe_sample(Sample*);

void legacy_dual_float_condition(Sample* sample)
{
    float stick = sample->stick;
    if (stick < 0) {
        stick = -stick;
    }
    if (stick >= limits->minimum &&
        sample->timer < limits->timer_base + limits->timer_slop) {
        accept_sample(sample, stick);
    }
    observe_sample(sample);
}
