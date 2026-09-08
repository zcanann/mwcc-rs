// flags: -Cpp_exceptions off -pragma "cats off"
static inline void tile_shifts(unsigned format, unsigned* x, unsigned* y) {
    switch (format) {
    case 0: case 8: case 14: case 32: case 48: *x = 3; *y = 3; break;
    case 1: case 2: case 9: case 17: case 34: case 39: case 40: case 41: case 42: case 57: case 58:
        *x = 3; *y = 2; break;
    case 3: case 4: case 5: case 6: case 10: case 19: case 22: case 35: case 43: case 44: case 60:
        *x = 2; *y = 2; break;
    default: *x = *y = 0; break;
    }
}
unsigned tile_count(unsigned format, unsigned short width, unsigned short height) {
    unsigned x, y;
    tile_shifts(format, &x, &y);
    return ((width + (1 << x) - 1) >> x) * ((height + (1 << y) - 1) >> y);
}
void tile_outputs(unsigned format, unsigned short width, unsigned short height, unsigned* output) {
    unsigned x, y;
    tile_shifts(format, &x, &y);
    if (width == 0) width = 1;
    if (height == 0) height = 1;
    output[0] = (width + (1 << x) - 1) >> x;
    output[1] = (height + (1 << y) - 1) >> y;
}
unsigned mipmapped_count(unsigned short width, unsigned short height, unsigned format,
                         unsigned char mipmap, unsigned char levels) {
    unsigned x, y, bytes, total, nx, ny, level;
    tile_shifts(format, &x, &y);
    bytes = (format == 6 || format == 22) ? 64 : 32;
    if (mipmap == 1) {
        total = 0;
        for (level = 0; level < levels; level++) {
            nx = (width + (1 << x) - 1) >> x;
            ny = (height + (1 << y) - 1) >> y;
            total += bytes * (nx * ny);
            if (width == 1 && height == 1) break;
            width = width > 1 ? width >> 1 : 1;
            height = height > 1 ? height >> 1 : 1;
        }
    } else {
        nx = (width + (1 << x) - 1) >> x;
        ny = (height + (1 << y) - 1) >> y;
        total = nx * ny * bytes;
    }
    return total;
}
