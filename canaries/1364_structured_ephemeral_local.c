// builds: GC/1.2.5n

typedef struct Object {
    char padding[44];
    void* user_data;
} Object;

typedef struct Holder {
    Object* child;
} Holder;

void consume(void*, Holder*);
void observe(Holder*);

void structured_ephemeral_local(Holder* holder)
{
    void* value = holder->child->user_data;
    consume(value, holder);
    observe(holder);
}
