// TEV count updates preserve byte promotion before arithmetic.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned field, dirty; };
extern struct Context *p;
typedef struct Context *ContextPointer;
extern volatile ContextPointer vp;
void narrow_byte(unsigned char n) { p->field=__rlwimi(p->field,(unsigned char)(n-1),0,0,31); p->dirty|=4; }
void narrow_signed(unsigned char n) { p->field=__rlwimi(p->field,(signed char)(n+1),0,0,31); p->dirty|=4; }
void outside_immediate(unsigned char n) { p->field=__rlwimi(p->field,n+32768,0,0,31); p->dirty|=4; }
void volatile_pointer(unsigned char n) { vp->field=__rlwimi(vp->field,n-1,10,18,21); vp->dirty=vp->dirty|4; }
