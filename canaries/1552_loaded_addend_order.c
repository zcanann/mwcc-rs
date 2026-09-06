/* Legacy load/arithmetic joins use canonical add register slots. */
extern int output, other;
void load_first(int x, const int* input) {
    output = *input + x * 3;
    other = x * 7;
}
void computed_first(int x, const int* input) {
    output = (x ^ 7) + *input;
    other = x * 7;
}
