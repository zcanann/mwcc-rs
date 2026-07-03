/* Fire 405: s_modf — pointer-param stores, the INTEGRAL block
   (*iptr = x[*one folded]; sign-only pun store; f1 reload — the
   stfd schedules after the pun stores), and x - *iptr (lfd+fsub).
   Registers per the capture with r3 = the live pointer param.
   @N +19. */
static const double one = 1.0;
double modf(double x, double *iptr)
{
int i0,i1,j0;
unsigned i;
i0 = *(int*)&x ;
i1 = *(1 + (int*)&x) ;
j0 = ((i0>>20)&0x7ff)-0x3ff;
if(j0<20) {
if(j0<0) {
*(int*)iptr = i0&0x80000000;
*(1 + (int*)iptr) = 0;
return x;
} else {
i = (0x000fffff)>>j0;
if(((i0&i)|i1)==0) {
*iptr = x;
*(int*)&x &= 0x80000000;
*(1 + (int*)&x) = 0;
return x;
} else {
*(int*)iptr = i0&(~i);
*(1 + (int*)iptr) = 0;
return x - *iptr;
}
}
} else if (j0>51) {
*iptr = x*one;
*(int*)&x &= 0x80000000;
*(1 + (int*)&x) = 0;
return x;
} else {
i = ((unsigned)(0xffffffff))>>(j0-20);
if((i1&i)==0) {
*iptr = x;
*(int*)&x &= 0x80000000;
*(1 + (int*)&x) = 0;
return x;
} else {
*(int*)iptr = i0;
*(1 + (int*)iptr) = i1&(~i);
return x - *iptr;
}
}
}
