/* A ready arithmetic producer must issue before a deliberately delayed load. */
extern int output, other;
void load_arithmetic_join(int x, const int* input) {
    output = x * 3 + *input;
    other = x * 7;
}
