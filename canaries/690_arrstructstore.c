// Array-of-structs member store: a[i].field = v scales the index by the struct
// size, then stores (stwx at offset 0, add+stw otherwise). A constant value goes
// in a fresh register (the allocator reuses the freed index register).
struct B { int x; int y; };
void arrstore(struct B *a, int i, int v){ a[i].y = v; }
void arrstorec(struct B *a, int i){ a[i].x = 5; }
