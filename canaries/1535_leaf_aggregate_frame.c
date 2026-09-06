/* Volatile aggregate temporaries need local storage but no leaf LR save. */
struct Pair { float x, y; };
void leaf_aggregate_frame(float* destination, float a, float b) {
    volatile struct Pair temporary;
    temporary.x = a * b;
    temporary.y = a + b;
    destination[0] = temporary.x;
    destination[1] = temporary.y;
}
