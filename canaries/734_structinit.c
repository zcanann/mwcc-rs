// An initialized struct global lists its fields, each serialized at its own
// (4-byte) offset — `struct P { int x; int y; } g = {3, 4}` is bytes 3,4, not a
// single 8-byte 3. Struct value arrays flatten the same way. (Sub-word-field
// structs defer — they need per-field strides.)
struct StructInitP { int x; int y; int z; };
struct StructInitP structinit_g = {10, 20, 30};
struct StructInitP structinit_arr[2] = {{1, 2, 3}, {4, 5, 6}};
