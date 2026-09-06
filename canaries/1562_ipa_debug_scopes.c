// builds: GC/3.0a3 Wii/1.0
// flags: -sym on -Cpp_exceptions off -ipa file
/* IPA analyzes all function scopes before creating the first line header. */
int first_value(void) {
    return 1;
}
int second_value(int discarded) {
    return 2;
}
int third_value(void) {
    return 3;
}
