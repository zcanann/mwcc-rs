/* Characterize stores of live parameter registers around a leaf return. */
int return_store_a, return_store_b, return_store_c;

int return_first_of_two(int a, int b) {
    return_store_a = a;
    return_store_b = b;
    return a;
}
int return_second_of_two(int a, int b) {
    return_store_a = a;
    return_store_b = b;
    return b;
}
int return_first_of_three(int a, int b, int c) {
    return_store_a = a;
    return_store_b = b;
    return_store_c = c;
    return a;
}
int return_middle_of_three(int a, int b, int c) {
    return_store_a = a;
    return_store_b = b;
    return_store_c = c;
    return b;
}
int return_last_of_three(int a, int b, int c) {
    return_store_a = a;
    return_store_b = b;
    return_store_c = c;
    return c;
}
