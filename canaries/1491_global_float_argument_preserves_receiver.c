// builds: GC/1.2.5n GC/1.3.2

typedef struct Vector {
    float x;
    float y;
    float z;
} Vector;

extern Vector vector;
void animate(void*, float);

void animate_z(void* object)
{
    animate(object, vector.z);
}
