// A double literal pools as an 8-byte .sdata2 constant loaded with lfd (not the
// 4-byte lfs of a single), because the binary op is double-precision.
double addk(double x){ return x + 2.0; }
double half(double x){ return x * 0.5; }
