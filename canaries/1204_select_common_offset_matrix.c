int add_truth_offset(int x) { return (x ? -4 : 9) + 3; }
int add_equal_offset(int x) { return (x == 7 ? 12 : -6) + 5; }
int add_commuted_offset(int x) { return 4 + (x != 0 ? 2 : 11); }
int subtract_compare_offset(int x) { return (x < 3 ? -3 : 8) - 4; }
