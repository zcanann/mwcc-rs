// flags: -Cpp_exceptions off -pragma "cats off"
struct Entry { unsigned attr, count, type; unsigned char fraction; };
struct Formats { unsigned a[8], b[8], c[8], dirty; unsigned char formats; };
extern struct Formats* formats;
static inline void set_format(unsigned* a, unsigned* b, unsigned* c, unsigned attr, unsigned count, unsigned type, unsigned char fraction) {
    switch (attr) {
    case 0: *a = (*a & ~15u) | ((count & 1) | ((type & 7) << 1)); break;
    case 1: *a = (*a & ~255u) | ((count & 3) | ((type & 7) << 2) | ((fraction & 7) << 5)); break;
    case 2: *b = (*b & ~15u) | ((count & 1) | ((type & 7) << 1)); break;
    case 3: *b = (*b & ~255u) | ((count & 3) | ((type & 7) << 2) | ((fraction & 7) << 5)); break;
    case 4: *c = (*c & ~15u) | ((count & 1) | ((type & 7) << 1)); break;
    case 5: *c = (*c & ~255u) | ((count & 3) | ((type & 7) << 2) | ((fraction & 7) << 5)); break;
    case 6: *a += fraction; *b ^= type; break;
    case 7: *b += fraction; *c ^= count; break;
    }
}
void format_list(unsigned index, const struct Entry* list) {
    unsigned* a = &formats->a[index];
    unsigned* b = &formats->b[index];
    unsigned* c = &formats->c[index];
    while (list->attr != 255) {
        set_format(a, b, c, list->attr, list->count, list->type, list->fraction);
        list++;
    }
    formats->dirty |= 16;
    formats->formats |= (unsigned char)(1 << (unsigned char)index);
}
