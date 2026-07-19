// flags: -ipa file

extern int ipa_int_target(int);
extern double ipa_double_target(double);

int ipa_int_wrapper(int value)
{
    return ipa_int_target(value);
}

double ipa_double_wrapper(double value)
{
    return ipa_double_target(value);
}
