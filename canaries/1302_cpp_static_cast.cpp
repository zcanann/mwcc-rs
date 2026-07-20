// C++ named static casts use the ordinary numeric conversion pipeline.
// builds: GC/1.3.2
// flags: -Cpp_exceptions off -O4,p -inline auto

float convert(unsigned value) {
    return static_cast<float>(value);
}
