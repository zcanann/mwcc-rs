// flags: -Cpp_exceptions off -pragma "cats off"
int operation(int);
void marker(int);
int one(int value) {
    int error = 0;
    marker(value);
    error |= !operation(value);
    return !error;
}
int two(int value) {
    int error = 0;
    marker(value);
    error |= !operation(value);
    error |= !operation(value + 1);
    return !error;
}
int observed(int value) {
    int error = 0;
    marker(error);
    error |= !operation(value);
    return !error;
}
int argument(int value) {
    int error = 0;
    marker(value);
    error |= !operation(error);
    return !error;
}
