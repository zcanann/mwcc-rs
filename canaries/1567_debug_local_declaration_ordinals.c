// flags: -sym on -Cpp_exceptions off
/* Mutable and const automatic declarations have distinct frontend costs. */
int mutable_result(int input) {
    int result = input;
    return result;
}
int constant_result(void) {
    const int result = 7;
    return result;
}
