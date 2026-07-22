// builds: GC/1.2.5n GC/1.3.2 GC/2.6

typedef struct Entry {
    int* pointer;
} Entry;

extern void consume(int*, int);

void member_constant_call_schedule(Entry* entry)
{
    consume(entry->pointer, 1);
}
