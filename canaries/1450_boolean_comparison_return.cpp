// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -O4,p -inline auto

bool equal_boolean(int value) { return value == 1; }
bool less_boolean(int value) { return value < 3; }
bool not_boolean(int value) { return !value; }
