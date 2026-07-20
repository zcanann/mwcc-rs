// flags: -O3,s -sym on
// builds: 1.2.5 1.3 2.6

int debug_global = 7;

int add_one(int value) {
    int result = value + 1;
    return result;
}
