// builds: GC/1.2.5n
// flags: -sym on -Cpp_exceptions off -inline all -char signed

extern double __frsqrte(double value);

extern inline float inline_root(float value)
{
    static const double half = 0.5;
    static const double three = 3.0;
    volatile float result;

    if (value > 0.0f)
    {
        double guess = __frsqrte((double)value);
        guess = half * guess * (three - guess * guess * value);
        result = (float)(value * guess);
        return result;
    }
    return value;
}

int constant_with_inline_statics(void)
{
    return 0;
}
