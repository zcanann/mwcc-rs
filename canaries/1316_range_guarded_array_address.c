// builds: GC/1.1p1

typedef struct Resource {
    unsigned char bytes[2192];
} Resource;

extern Resource resources[3];

void* resource_at(int index)
{
    Resource* result = 0;
    if (index >= 0 && index < 3) {
        result = &resources[index];
    }
    return result;
}
