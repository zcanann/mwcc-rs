/* Fire 419: the CTR LOOP — a counted `while (n--)` whose body BRANCHES
   escapes the ×8 unroll entirely: `mtctr n; cmpwi n,0; beq(lr); BODY;
   bdnz BODY`. The skip branch mirrors the entry test faithfully (beq —
   a negative n runs 2^32 times on the unsigned CTR, same as the C).
   The diamond: `hz = hx - K` fuses into `addic. r0` (the condition-only
   computed rides r0 through the else arm), both arms write the param
   home, join at the bdnz. Post-loop code takes `beq END` instead of
   beqlr. Straight-line bodies take the ×8 unroll (deferred); the
   `for(i<n)` variant if-converts its diamond via an eager else + mr
   join (unclaimed, captured fire 419). @N: +0 (measured, objprobe). */
int ctr_walk(int hx, int hz, int n)
{
	while (n--) {
		hz = hx - 3;
		if (hz < 0)
			hx = hx + hx;
		else
			hx = hz + hz;
	}
	return hx;
}
int ctr_walk_post(int hx, int n)
{
	int hz;
	while (n--) {
		hz = hx - 3;
		if (hz < 0)
			hx = hx + hx;
		else
			hx = hz + hz;
	}
	return hx + 7;
}
