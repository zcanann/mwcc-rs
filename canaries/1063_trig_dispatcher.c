/* Fire 408: the TRIG DISPATCHER (s_sin/s_cos) — range dispatch with
   a frame array (y[2] at 16, x spill at 8, NO callee-saved), the K1
   synthesis in the mflr latency slot, cmpw against lis-built
   bounds, the binary switch tree over n&3, per-arm lfd/li/lfd
   argument loads, and fneg on quadrants 2/3. @N +13. */
extern double __kernel_sin(double, double, int);
extern double __kernel_cos(double, double);
extern int __ieee754_rem_pio2(double, double*);
double sin(double x)
{
    double y[2], z = 0.0;
    int n, ix;
    ix = *(int *)&x;
    ix &= 0x7fffffff;
    if (ix <= 0x3fe921fb)
        return __kernel_sin(x, z, 0);
    else if (ix >= 0x7ff00000)
        return x - x;
    else {
        n = __ieee754_rem_pio2(x, y);
        switch (n & 3) {
        case 0:
            return __kernel_sin(y[0], y[1], 1);
        case 1:
            return __kernel_cos(y[0], y[1]);
        case 2:
            return -__kernel_sin(y[0], y[1], 1);
        default:
            return -__kernel_cos(y[0], y[1]);
        }
    }
}
