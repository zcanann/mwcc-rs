/* Volatile input reads remain separate despite private-field promotion. */
struct Pair { float x, y; };
void volatile_snapshot(float* destination, const volatile float* source, float x, float y) {
    struct Pair temporary;
    temporary.x = source[0] * x;
    temporary.y = source[0] * y;
    destination[0] = temporary.x;
    destination[1] = temporary.y;
}
