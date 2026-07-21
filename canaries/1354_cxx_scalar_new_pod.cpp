void* operator new(unsigned long);

int* make_int() {
    return new int;
}
