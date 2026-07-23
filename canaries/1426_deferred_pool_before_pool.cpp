// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -O4,p -inline auto,deferred

float deferred_source_pool_owner() {
    return 2.5f;
}

float deferred_later_pool_owner() {
    return 1.25f;
}
