// builds: GC/2.6
// A producing call consumes every incoming parameter in its ABI lane. Its pointer
// result therefore remains in r3 as the first argument of the immediately following
// consumer; integer and float literals fill r4 and f1 without a callee-saved home.
// This is Melee's `Fighter* fp = HSD_GObjGetUserData(gobj); set(fp, 0, 0.0F)` shape.
void* forward_make(void*);
void forward_use(void*, int, float);

void result_call_forward_mixed(void* object)
{
    void* value = forward_make(object);
    forward_use(value, 0, 0.0F);
}
