void* operator new(unsigned long);

struct Box {
    int value;
    Box(int);
};

Box* make_box(int value) {
    return new Box(value);
}
