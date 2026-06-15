// double<->int conversions. (double)int reuses the int->float bias idiom but
// ends in fsub (double) not fsubs; (int)double is fctiwz + a frame bounce.
double itod(int x){ return (double)x; }
double utod(unsigned x){ return (double)x; }
int dtoi(double x){ return (int)x; }
