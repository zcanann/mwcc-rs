// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,p -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

struct Triangle {
    unsigned int map_code;
};

static int guarded_map_code(struct Triangle* triangle)
{
    if (triangle) {
        return triangle->map_code >> 27 & 3;
    }
    return 0;
}
