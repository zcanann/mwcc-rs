/* Fire 428: the SIGN-INDEXED DOUBLE RETURN (e_fmod's Zero[] exit).
   The index (sx>>31)<<3 fuses into ONE rotate-mask (rlwinm
   r0,sx,4,28,28); the base is a lis/addi ADDR16_HA/LO pair on the
   LOCAL .data symbol (16 bytes stays out of sdata); the load is
   lfdx f1,lo,index. ha->r4, lo->r3 (sx's home, dead after the
   rlwinm), index->r0. @N +0. */
static double Zero[] = {0.0, -0.0,};
double zret(int sx)
{
	return Zero[(unsigned)sx >> 31];
}
