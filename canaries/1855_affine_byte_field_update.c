// TEV count updates preserve byte promotion before arithmetic.
// flags: -Cpp_exceptions off -pragma "cats off"
struct Context { unsigned field, dirty; };
extern struct Context * const p;
void subtract_one(unsigned char n) { p->field=__rlwimi(p->field,n-1,10,18,21); p->dirty|=4; }
void add_seven(unsigned char n) { p->field=__rlwimi(p->field,n+7,8,16,23); p->dirty|=0x8000; }
void subtract_limit(unsigned char n) { p->field=__rlwimi(p->field,n-32768,7,29,4); p->dirty|=1; }
void add_limit(unsigned char n) { p->field=__rlwimi(p->field,n+32767,0,0,31); p->dirty|=0x1234; }
void alias(unsigned char n) { p->field=__rlwimi(p->field,n-1,24,0,7); p->field|=4; }
