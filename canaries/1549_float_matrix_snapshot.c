// flags: -fp_contract off
/* Three independent row reductions share an input snapshot before stores. */
struct Vector { float x, y, z; };
void transform_snapshot(const float m[3][3], const struct Vector* input, struct Vector* output) {
    struct Vector temporary;
    temporary.x = m[0][2] * input->z + (m[0][0] * input->x + m[0][1] * input->y);
    temporary.y = m[1][2] * input->z + (m[1][0] * input->x + m[1][1] * input->y);
    temporary.z = m[2][2] * input->z + (m[2][0] * input->x + m[2][1] * input->y);
    output->x = temporary.x;
    output->y = temporary.y;
    output->z = temporary.z;
}
