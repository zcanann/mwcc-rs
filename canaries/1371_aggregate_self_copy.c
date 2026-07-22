// builds: GC/1.2.5n

typedef struct Vector {
    float x;
    float y;
    float z;
} Vector;

typedef struct Object {
    Vector scale;
} Object;

void aggregate_self_copy(Object* object)
{
    object->scale = *&object->scale;
}
