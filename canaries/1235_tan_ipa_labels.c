// flags: -O4,p -ipa file
// builds: GC/3.0a3p1 Wii/1.0

extern double __kernel_tan(double, double, int);
extern int __ieee754_rem_pio2(double, double*);

double tan(double x) {
    double y[2], z = 0.0;
    int n, ix;
    ix = *(int*)&x;
    ix &= 0x7fffffff;
    if (ix <= 0x3fe921fb)
        return __kernel_tan(x, z, 1);
    else if (ix >= 0x7ff00000)
        return x - x;
    else {
        n = __ieee754_rem_pio2(x, y);
        return __kernel_tan(y[0], y[1], 1 - ((n & 1) << 1));
    }
}
