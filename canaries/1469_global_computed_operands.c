// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

unsigned bits;

unsigned global_or_product(unsigned a, unsigned b)
{
    return bits | (a * b);
}

unsigned product_or_global(unsigned a, unsigned b)
{
    return (a * b) | bits;
}

unsigned global_minus_product(unsigned a, unsigned b)
{
    return bits - (a * b);
}

unsigned product_minus_global(unsigned a, unsigned b)
{
    return (a * b) - bits;
}

unsigned one_shifted_by_computed_amount(unsigned amount)
{
    return 1U << (31 - amount);
}
