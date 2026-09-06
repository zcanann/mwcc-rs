/* Scalar replacement preserves parameter values without a temporary frame. */
struct Pair { float x, y; };
void aggregate_parameter_copy(float* destination, float x, float y) {
    struct Pair temporary;
    temporary.x = x;
    temporary.y = y;
    destination[0] = temporary.x;
    destination[1] = temporary.y;
}
