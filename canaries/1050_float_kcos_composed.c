/* THE COMPOSED __kernel_cos (fire 370, the second complete real libm
 * file): the punned early-const prefix (fctiwz slot 24), the big-const
 * ix dual with the lis/addi weave, the 6-const outer-mul chain, the
 * fsub-of-fsub then-tail, and the ELSE COMPOSITION — the inner qx
 * diamond (lis r0/cmpw/ble, literal arm, addis ix-C punned stores, +4
 * labels) with the tail reading x RE-reloaded and qx from the frame,
 * hz/a folding away, under the emission_over_tier register regime
 * (inside-params window, no tier-forward, no crossing tier) and the
 * frame-load source-order fmul. */
static const double
one =  1.00000000000000000000e+00,
C1  =  4.16666666666666019037e-02,
C2  = -1.38888888888741095749e-03,
C3  =  2.48015872894767294178e-05,
C4  = -2.75573143513906633035e-07,
C5  =  2.08757232129817482790e-09,
C6  = -1.13596475577881948265e-11;
double __kernel_cos(double x, double y)
{
	double a,hz,z,r,qx;
	int ix;
	ix = *(int*)&x & 0x7fffffff;
	if(ix<0x3e400000) {
	    if(((int)x)==0) return one;
	}
	z  = x*x;
	r  = z*(C1+z*(C2+z*(C3+z*(C4+z*(C5+z*C6)))));
	if(ix < 0x3FD33333)
	    return one - (0.5*z - (z*r - x*y));
	else {
	    if(ix > 0x3fe90000) {
		qx = 0.28125;
	    } else {
	        *(int*)&qx = ix-0x00200000;
	        *((int*)&qx+1) = 0;
	    }
	    hz = 0.5*z-qx;
	    a  = one-qx;
	    return a - (hz - (z*r-x*y));
	}
}
