/* Fire 407: the __frsqrte NEWTON SQRT (Dolphin math_inlines) — a
   LEAF float ladder around four refinement steps with stable
   registers (f2=guess, f4=.5, f3=3.0, f0=temp; the LAST step's
   product lands in f0). fcmpu for the equality rungs (pool-first,
   then swapped for the bare-x rung); lis+lfs Addr16 pairs through
   the NAN/INFINITY int-array globals. @N +12. */
extern int __float_nan[];
extern int __float_huge[];
double sqrt(double x)
{
	if (x > 0.0) {
		double guess = __frsqrte(x);
		guess     = .5 * guess * (3.0 - guess * guess * x);
		guess     = .5 * guess * (3.0 - guess * guess * x);
		guess     = .5 * guess * (3.0 - guess * guess * x);
		guess     = .5 * guess * (3.0 - guess * guess * x);
		return x * guess;
	} else if (x == 0.0) {
		return 0;
	} else if (x) {
		return (*(float*)__float_nan);
	}
	return (*(float*)__float_huge);
}
