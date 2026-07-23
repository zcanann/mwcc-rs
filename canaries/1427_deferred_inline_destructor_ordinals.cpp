// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -O4,p -inline auto,deferred

class DeferredDestructorOwner {
public:
    ~DeferredDestructorOwner() {}
};

float deferred_inline_destructor_pool_owner() {
    return 1.25f;
}
