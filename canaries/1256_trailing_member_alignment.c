typedef struct Layout {
    unsigned char head;
    int value __attribute__((aligned(32)));
    unsigned char tail;
    unsigned char data[7] __attribute__((aligned(16)));
    int end;
} Layout;

int read_aligned_value(Layout* value) {
    return value->value;
}

int read_after_aligned_array(Layout* value) {
    return value->end;
}
