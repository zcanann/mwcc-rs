// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Object {
    unsigned int flags;
} Object;

extern void mark_dirty(Object*);

void loaded_member_record_mask(Object* object)
{
    if (!(object->flags & (1 << 25))) {
        mark_dirty(object);
    }
}
