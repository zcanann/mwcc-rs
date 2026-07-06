extern double cos(double);
extern double pow(double, double);
float cosf(float x) { return cos((double)x); }
float powf(float x, float y) { return pow((double)x, (double)y); }
