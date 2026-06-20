// Array-of-structs member load: a[i].field scales the index by the struct size
// (slwi/mulli), then loads the member (lwzx at offset 0, add+lwz otherwise).
struct B { int x; int y; };
int arrstruct(struct B *a, int i){ return a[i].y; }
int arrstruct0(struct B *a, int i){ return a[i].x; }
