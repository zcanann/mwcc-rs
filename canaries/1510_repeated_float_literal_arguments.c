// builds: GC/1.3.2
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

void take_three_floats(float, float, float);

void repeated_adjacent_float_literals(void)
{
    take_three_floats(2.0f, 2.0f, 1.0f);
}

void repeated_separated_float_literals(void)
{
    take_three_floats(0.0f, -30.0f, 0.0f);
}
