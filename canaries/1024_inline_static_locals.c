/* A skipped inline function's static locals (measured matrix): a PLAIN
 * inline emits each as a WEAK object `<name>$localstatic<K>$<fn>` (K from 3,
 * statics only, per function; const -> .sdata2, non-zero -> .sdata, zero ->
 * .sbss), laid ahead of the pool constants, with NO @N shift, and .comment
 * flags 0x0d (a weak FUNCTION carries 0x0e). A STATIC inline emits NO data
 * and bumps the @N counter by one per static local. */
inline double plain_one(double x)
{
	static const double b = 2.5;
	return x + b;
}

inline double plain_two(double x)
{
	static const double c = 4.5;
	static const double d = 6.5;
	return x + c + d;
}

inline int plain_int(int x)
{
	static int counter = 7;
	return x + counter;
}

inline int plain_zero(int x)
{
	static int z;
	z += x;
	return z;
}

static inline double dropped(double x)
{
	static const double e = 8.5;
	static const double f2 = 9.5;
	return x + e + f2;
}

double g(double x)
{
	return x * 3.0;
}
