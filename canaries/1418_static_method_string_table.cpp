// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,p -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

struct Entry {
    unsigned int code;
};

struct Names {
    static int attribute(Entry* entry);
    static const char* label(int index);
};

static const char* labels[] = {
    "solid", "rock", "grass", "wood", "mud", "water", "hole",
};

int Names::attribute(Entry* entry)
{
    return entry->code >> 29;
}

const char* Names::label(int index)
{
    return labels[index];
}
