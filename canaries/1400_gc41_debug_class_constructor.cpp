// builds: GC/3.0a3p1
// flags: -nodefaults -proc gekko -O4,s -inline noauto -sym on -schedule off -RTTI on -pragma "cats off"

class Gc41DebugDerived {
public:
    Gc41DebugDerived();
    virtual ~Gc41DebugDerived() {}
    int value;
};

Gc41DebugDerived::Gc41DebugDerived() {
    value = 1;
}
