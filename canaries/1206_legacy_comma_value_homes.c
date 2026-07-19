/* Characterize build 163's stack homes for comma-operator values. */
int comma_home_a, comma_home_b;

void comma_right_first(int a, int b) {
    comma_home_a = (b, a);
}

void comma_right_fourth(int a, int b, int c, int d) {
    comma_home_a = (a, d);
}

void comma_chain_four(int a, int b, int c, int d) {
    comma_home_a = (a, b, c, d);
}

void comma_assign_then_chain(int a, int b, int c) {
    comma_home_a = (comma_home_b = a, b, c);
}

void comma_pair_same(int a, int b) {
    comma_home_a = (a, b) + (a, b);
}

void comma_pair_distinct(int a, int b, int c, int d) {
    comma_home_a = (a, b) + (c, d);
}

void comma_pair_reverse(int a, int b, int c, int d) {
    comma_home_a = (a, d) + (c, b);
}
