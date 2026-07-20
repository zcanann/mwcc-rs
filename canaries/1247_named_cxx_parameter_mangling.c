// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
#pragma cplusplus on

struct Creature {
    int value;
};

namespace JUtility {
struct TColor {
    unsigned int rgba;
};
}

struct Action {
    void pointer(const Creature*);
    void reference(const Creature&);
    void value(Creature);
    void qualified(JUtility::TColor);
};

void Action::pointer(const Creature*) { }
void Action::reference(const Creature&) { }
void Action::value(Creature) { }
void Action::qualified(JUtility::TColor) { }

#pragma cplusplus off
