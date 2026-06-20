// A cast is transparent for a pointer global's address initializer: `(T *)&x` is
// just an ADDR32 relocation to `x`. Works in a table and with a null pointer.
extern int ptrcast_x;
long* ptrcast_p = (long*)&ptrcast_x;
void* ptrcast_tbl[] = { (void*)&ptrcast_x, (void*)0 };
