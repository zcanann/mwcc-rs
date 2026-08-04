int select_even_nibble(int value) {
    return ((value & 1) == 0) ? 4 : 0;
}
