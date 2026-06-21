// A struct value/array initializer is serialized field-by-field into the object's
// byte image, each field at its own offset and width — so char/short (sub-word),
// padded, and mixed-width fields are exact, not just all-word structs.
struct Flags { unsigned char a, b, c, d; };
struct Flags swi_bytes = { 0x11, 0x22, 0x33, 0x44 };
struct Packed { char tag; short id; int value; };
struct Packed swi_mixed = { 1, 0x1234, 0x05060708 };
struct Packed swi_arr[2] = { { 1, 2, 3 }, { 4, 5, 6 } };
struct Pad { char c; int n; };
struct Pad swi_pad = { 7, 0x0A0B0C0D };
