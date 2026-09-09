typedef unsigned long long U64;
typedef long long S64;
struct Node { struct Node *next; S64 fire; unsigned tag; };
U64 count_sum(U64 value, unsigned count) {
    while (count) { value = value + count; count = count - 1; }
    return value;
}
U64 skip_sum(U64 value, unsigned count) {
    unsigned i;
    for (i = 0; i < count; i = i + 1) {
        if (i & 1) continue;
        if (i > 12) break;
        value = value + i;
    }
    return value;
}
S64 first_later(struct Node *node, S64 fire) {
    for (; node; node = node->next) {
        if (node->fire <= fire) continue;
        return node->fire;
    }
    return fire;
}
U64 do_count(U64 value, unsigned count) {
    do {
        value = value + 3;
        count = count - 1;
        if (count & 1) continue;
        value = value ^ count;
    } while (count);
    return value;
}
U64 nested_sum(U64 value, unsigned count) {
    unsigned i, j;
    for (i = 0; i < count; i = i + 1) {
        for (j = 0; j < 4; j = j + 1) {
            if (j == 1) continue;
            if (i == 3) break;
            value = value + i + j;
        }
    }
    return value;
}
