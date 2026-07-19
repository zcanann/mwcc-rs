// flags: -O4,p -inline auto,deferred
/* Deferred compilation retains three additional anonymous labels around the
 * __frsqrte sqrt control-flow graph on both compiler generations. The ordinary
 * form consumes 12 labels before its three pooled doubles; deferred consumes
 * 15. */
extern int __float_nan[];
extern int __float_huge[];

double sqrt(double x)
{
	if (x > 0.0) {
		double guess = __frsqrte(x);
		guess = .5 * guess * (3.0 - guess * guess * x);
		guess = .5 * guess * (3.0 - guess * guess * x);
		guess = .5 * guess * (3.0 - guess * guess * x);
		guess = .5 * guess * (3.0 - guess * guess * x);
		return x * guess;
	} else if (x == 0.0) {
		return 0;
	} else if (x) {
		return (*(float*)__float_nan);
	}
	return (*(float*)__float_huge);
}
