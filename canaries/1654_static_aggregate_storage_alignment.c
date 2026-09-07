// Function-local aggregates use object size and versioned storage alignment.
// flags: -Cpp_exceptions off -pragma "cats off"
struct R3 { char a,b,c; };
struct R8 { int a,b; };
struct R12 { int a,b,c; };
void* bytes3(void) { static char p[3]={1,2,3}; return &p; }
void* bytes8(void) { static char p[8]={1}; return &p; }
void* shorts3(void) { static short p[3]={1,2,3}; return &p; }
void* ints2(void) { static int p[2]={1,2}; return &p; }
void* record3(void) { static struct R3 p={1,2,3}; return &p; }
void* record8(void) { static struct R8 p={1,2}; return &p; }
void* records12(void) { static struct R12 p[2]={{1,2,3},{4,5,6}}; return &p; }
const void* const_bytes(void) { static const char p[3]={1,2,3}; return &p; }
void* zero_bytes(void) { static char p[8]; return &p; }
void* explicit_alignment(void) {
    static char a[1] __attribute__((aligned(2)))={1};
    static char b[1] __attribute__((aligned(2)))={2};
    static int p[2] __attribute__((aligned(16)))={1,2};
    return &p;
}
