// Absolute member addresses and dependent pointer arguments.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef float f32;
struct Context { unsigned tag; f32 values[6]; };
extern struct Context* g;
static inline void copy(const register f32* src, register f32* dest) {
 register f32 a, b, c;
 asm {
  psq_l a, 0(src), 0, 0
  psq_l b, 8(src), 0, 0
  psq_l c, 16(src), 0, 0
  psq_st a, 0(dest), 0, 0
  psq_st b, 8(dest), 0, 0
  psq_st c, 16(dest), 0, 0
 }
}
void out(f32* p) { copy(g->values, &p[1]); }
void in(f32* p) { copy(&p[1], g->values); }
void local(f32* p) { copy(&p[1], &p[2]); }
