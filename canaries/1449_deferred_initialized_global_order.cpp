// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -O4,p -inline auto,deferred

class DeferredDataOwner {
public:
    static int table[2];
    int before_one();
    int before_two();
    float after_one();
    int after_two();
};

int DeferredDataOwner::before_one() { return 1; }
int DeferredDataOwner::before_two() { return 2; }

int DeferredDataOwner::table[2] = { 3, 4 };

float DeferredDataOwner::after_one() { return 0.0f; }
int DeferredDataOwner::after_two() { return 5; }
