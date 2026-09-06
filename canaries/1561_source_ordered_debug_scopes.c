// flags: -sym on -Cpp_exceptions off
/* Later parameter declarations must not advance the first non-IPA line header. */
int first_value(void) {
    return 1;
}
int second_value(int discarded) {
    return 2;
}
int third_value(void) {
    return 3;
}
