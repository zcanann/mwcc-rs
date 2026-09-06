// flags: -fp_contract off
/* A second reduction count distinguishes block scheduling from matrix names. */
struct Pair { float x, y; };
void two_row_snapshot(const float* m, const float* input, struct Pair* output) {
    struct Pair temporary;
    temporary.x = m[2] * input[2] + (m[0] * input[0] + m[1] * input[1]);
    temporary.y = m[5] * input[2] + (m[3] * input[0] + m[4] * input[1]);
    output->x = temporary.x;
    output->y = temporary.y;
}
