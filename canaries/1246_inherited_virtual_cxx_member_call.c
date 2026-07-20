// builds: GC/1.1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7
#pragma cplusplus on

struct Base {
    virtual int value(int);
};

struct Child : Base {
    int value(int);
    virtual int added(int);
};

extern "C" int caller(Child* child, int input) { return child->value(input); }

#pragma cplusplus off
