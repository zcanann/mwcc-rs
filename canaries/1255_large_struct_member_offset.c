typedef struct Huge {
    unsigned char first[20000];
    unsigned char second[20000];
    unsigned char third[20000];
    unsigned char fourth[20000];
    int tail;
} Huge;

Huge large_global;

int load_large_pointer_member(Huge* value) {
    return value->tail;
}

int load_large_global_member(void) {
    return large_global.tail;
}

int* address_large_global_member(void) {
    return &large_global.tail;
}
