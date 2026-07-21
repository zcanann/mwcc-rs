// builds: GC/1.1p1

typedef struct Resource {
    int lock;
    int used;
} Resource;

extern Resource* acquire_resource_at(int index);
extern void lock_resource(Resource* resource);
extern void reset_resource(Resource* resource, int preserve);
extern void mark_resource_used(Resource* resource, int used);
extern void unlock_resource(Resource* resource);

int find_free_resource(int* selected_index, Resource** selected_resource)
{
    int status = 768;
    int index;
    *selected_resource = 0;

    for (index = 0; index < 3; index++) {
        Resource* resource = acquire_resource_at(index);

        lock_resource(resource);
        if (!resource->used) {
            reset_resource(resource, 1);
            mark_resource_used(resource, 1);
            status = 0;
            *selected_resource = resource;
            *selected_index = index;
            index = 3;
        }
        unlock_resource(resource);
    }

    return status;
}
