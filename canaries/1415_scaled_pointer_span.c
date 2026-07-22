// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,p -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef unsigned int u32;

struct Card {
    u32 sector_size;
    void* work_area;
};

static u32 scaled_pointer_span(struct Card* card, void* current)
{
    return ((u32)current - (u32)card->work_area) / 8192 * card->sector_size;
}
