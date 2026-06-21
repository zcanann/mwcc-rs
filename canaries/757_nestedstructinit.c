// A struct value member is sized and aligned by its own layout (not a 4-byte
// default), so a struct containing struct values lays out correctly and its data
// initializer — including the float-heavy Vec3 tables of the board DLLs — serializes
// byte-exactly. Nested struct fields recurse in the per-field initializer.
struct Vec3 { float x, y, z; };
struct MapObject { struct Vec3 pos, rot, scale; int id; };
struct MapObject nsi_table[2] = {
    { { 3300.0f, 100.0f, -900.0f }, { 0.0f, 0.0f, 0.0f }, { 1.0f, 1.0f, 1.0f }, 0x11 },
    { { -2550.0f, 0.0f, 1350.0f }, { 0.0f, 0.0f, 0.0f }, { 1.0f, 1.0f, 1.0f }, 0x14 },
};
struct Inner { int a, b; };
struct Outer { struct Inner v; int n; };
struct Outer nsi_mixed = { { 1, 2 }, 3 };
