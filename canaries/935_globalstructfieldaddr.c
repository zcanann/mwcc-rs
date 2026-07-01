// `&g.field` where `g` is a file-scope struct VALUE global is the field ADDRESS `&g + offset` — an
// address computation (SDA21 `li d,g@sda21` for a small `.sdata` struct, `lis;addi` ADDR16 for a
// large one), then `addi d,d,offset`. `g` is the struct itself, so its address is TAKEN, not loaded;
// the struct-POINTER form `&p->field` (= `load p; addi offset`) is a different lvalue and is unchanged.
// Mirrors the `&global_array[i]` element-address path (they share emit_global_array_base).
//
// DEFERS (no wrong bytes): `&p->field` for a pointer, and `&s.field` for a frame-resident local
// struct — both a different lvalue than a file-scope struct value.
struct Small { int x, y; };            // 8 bytes -> SDA21
struct Big   { int a, b, c, d; };      // 16 bytes -> ADDR16
struct Mixed { char tag; int value; }; // padded; `value` at offset 4

struct Small small;
struct Big   big;
struct Mixed mixed;

int *small_x(void)  { return &small.x; }   // &small (offset 0)
int *small_y(void)  { return &small.y; }   // &small + 4
int *big_c(void)    { return &big.c; }      // lis;addi &big; addi +8
int *mixed_value(void) { return &mixed.value; } // &mixed + 4
