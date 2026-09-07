// Reduced from BFBB zAssetTypes.cpp's one-past-end texture-table pointer.
// flags: -Cpp_exceptions off -pragma "cats off"
static char* textures[] = {"one", "two", "three", "four", "five"};
static char** textures_end = &textures[5];
char** get_textures_end() { return textures_end; }
