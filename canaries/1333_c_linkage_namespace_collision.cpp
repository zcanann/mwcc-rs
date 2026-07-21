// A global C-linkage name does not donate its linkage to the same unqualified
// spelling in a namespace; the qualified function retains C++ mangling.
// builds: GC/2.0p1

extern "C" float sinf(float value);

namespace std {
float sinf(float value);
}

float wrapper(float value) {
    return std::sinf(value);
}

float std::sinf(float value) {
    return value;
}
