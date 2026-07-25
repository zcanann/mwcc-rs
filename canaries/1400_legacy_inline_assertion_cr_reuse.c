// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Object {
    unsigned int flags;
} Object;

extern void __assert(char*, int, char*);
extern void mark_dirty(Object*);

static inline int object_is_dirty(Object* object)
{
    int result;

    ((object) ? ((void) 0) : __assert("object.h", 17, "object"));
    result = 0;
    if (!(object->flags & (1 << 23)) && (object->flags & (1 << 6))) {
        result = 1;
    }
    return result;
}

void legacy_inline_assertion_cr_reuse(Object* object)
{
    if (object != 0 && !object_is_dirty(object)) {
        mark_dirty(object);
    }
}
