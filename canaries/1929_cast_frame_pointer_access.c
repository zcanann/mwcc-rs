// flags: -Cpp_exceptions off -pragma "cats off"
// Constant displacement must follow the current frame-backed pointer value.
extern void redirect(unsigned **);
unsigned frame_pointer(unsigned *p) { redirect(&p); return *(unsigned short *)(p + 2); }
unsigned frame_array(unsigned x) { unsigned a[4]; a[0] = x; a[1] = x + 1; a[2] = x + 2; a[3] = x + 3; return *(unsigned short *)(a + 2); }
