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
/* Fire 420: two leaves toward e_fmod's real walk. The register head
   `hz = hx - hy` fuses into `subf. r0,hy,hx`; the PAIR CARRY STEP
   `hx = hx+hx+(lx>>31); lx = lx+lx` (the 2-word left shift) emits
   srwi r0 first, schedules the LOW doubling between, then adds as
   hx + (hx + carry) — the unsigned low is required (signed would
   srawi). @N still +0. */
int ctr_sub_reg(int hx, int hy, int n)
{
	int hz;
	while (n--) {
		hz = hx - hy;
		if (hz < 0)
			hx = hx + hx;
		else
			hx = hz + hz;
	}
	return hx;
}
int ctr_pair_step(int hx, unsigned lx, int hy, int n)
{
	int hz;
	while (n--) {
		hz = hx - hy;
		if (hz < 0) {
			hx = hx + hx + (lx >> 31);
			lx = lx + lx;
		} else
			hx = hz + hz;
	}
	return hx;
}
/* Fire 421: e_fmod's core loop captured WHOLE — the 2-word walk with
   borrow. cmplw hoists above both subf (latency fill); hz/lz take the
   freed count home + next-up, plain subf (the borrow decrement forces
   an explicit cmpwi re-test); else-arm intermediates land directly in
   r3 (hx is not a source); lx's home takes lz+lz. @N +0. */
int ctr_fmod_core(int hx, unsigned lx, int hy, unsigned ly, int n)
{
	int hz;
	unsigned lz;
	while (n--) {
		hz = hx - hy;
		lz = lx - ly;
		if (lx < ly)
			hz -= 1;
		if (hz < 0) {
			hx = hx + hx + (lx >> 31);
			lx = lx + lx;
		} else {
			hx = hz + hz + (lz >> 31);
			lx = lz + lz;
		}
	}
	return hx;
}
/* Fire 422: the ZERO EXIT — the else arm may lead with
   if ((hz|lz)==0) return K; emitted INLINE: or. r0,hz,lz; bne CONT;
   li r3,K; blr — a bare mid-loop return, no exit label. @N +0. */
int ctr_fmod_exit(int hx, unsigned lx, int hy, unsigned ly, int n)
{
	int hz;
	unsigned lz;
	while (n--) {
		hz = hx - hy;
		lz = lx - ly;
		if (lx < ly)
			hz -= 1;
		if (hz < 0) {
			hx = hx + hx + (lx >> 31);
			lx = lx + lx;
		} else {
			if ((hz | lz) == 0)
				return 0;
			hx = hz + hz + (lz >> 31);
			lx = lz + lz;
		}
	}
	return hx;
}
/* Fire 424: the NORMALIZE LOOP (e_fmod's tail) — non-counted rotated
   pair-step walk. The big bound hoists lis r0 BEFORE the loop and r0
   stays occupied across it, evicting the carry temp to the next free
   register past the params; the iy decrement schedules into the add
   latency. @N +0. */
int norm_loop(int hx, unsigned lx, int iy)
{
	while (hx < 0x00100000) {
		hx = hx + hx + (lx >> 31);
		lx = lx + lx;
		iy -= 1;
	}
	return hx + iy;
}
