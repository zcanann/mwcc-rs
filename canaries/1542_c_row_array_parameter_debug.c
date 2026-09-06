// flags: -sym on
/* Ordinary C signatures retain the same row identity as assembly signatures. */
typedef float Matrix[3][4];
void copy_first(register Matrix matrix, register float* output) {
    *output = matrix[0][0];
}
void copy_second(register Matrix matrix, register float* output) {
    *output = matrix[1][0];
}
