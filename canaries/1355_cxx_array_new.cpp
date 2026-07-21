void* operator new[](unsigned long);

char* make_buffer() {
    return new char[64];
}
