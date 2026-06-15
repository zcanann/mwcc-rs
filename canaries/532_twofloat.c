// Two functions each pooling a float constant: the constants share one .sdata2
// (f's first, g's second), and the anonymous @N counter advances across the
// functions (f's constant @5, g's @10).
float fdbl(float x){ return x * 2.0f; }
float ftpl(float x){ return x * 3.0f; }
