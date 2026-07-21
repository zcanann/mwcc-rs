// builds: GC/1.1p1

typedef struct Resource {
    unsigned char bytes[2192];
} Resource;

extern Resource resources[3];
extern void lock_resource(Resource* resource);
extern void mark_resource_used(Resource* resource, int used);
extern void unlock_resource(Resource* resource);

void release_resource(int index)
{
    Resource* resource;
    if (index != -1 && index >= 0 && index < 3) {
        resource = &resources[index];
        lock_resource(resource);
        mark_resource_used(resource, 0);
        unlock_resource(resource);
    }
}
