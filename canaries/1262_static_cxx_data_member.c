// builds: GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
#pragma cplusplus on

namespace Game {
struct Creature {
    static bool enabled;
    int packet_culling_enabled(void);
};

bool Creature::enabled = true;

int Creature::packet_culling_enabled(void) {
    return Creature::enabled;
}
}
