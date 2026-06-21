// A file-scope struct (or struct array) initializer folds each field with its own
// type from the layout, so float fields encode as their IEEE-754 bits where the
// flat element-typed parser (integers only) could not. All fields are word-width.
struct Vec3f { float x, y, z; };
struct Vec3f sfi_one = { 1.0f, 2.0f, 3.0f };
struct Vec3f sfi_arr[2] = { { 1.0f, 2.0f, 3.0f }, { 4.0f, 5.0f, 6.0f } };
struct Mixed { int tag; float value; };
struct Mixed sfi_mixed = { 7, 0.5f };
