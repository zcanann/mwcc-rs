// builds: GC/1.2.5n
// Build 163 linkage-first schedule for a member-loaded local that immediately
// dies as the first argument of a mixed integer/float call. This is Melee's
// `Fighter* fp = gobj->user_data; set(fp, 0, 0.0F)` shape.
struct ForwardObject {
    int padding[11];
    void* user_data;
};

void legacy_forward_use(void*, int, float);

void legacy_computed_local_call_forward(struct ForwardObject* object)
{
    void* value = (void*) object->user_data;
    legacy_forward_use(value, 0, 0.0F);
}
