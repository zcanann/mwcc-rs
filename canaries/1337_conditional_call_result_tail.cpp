// flags: -inline noauto -O4,s -ipa file
// builds: GC/3.0a3p1

typedef unsigned int u32;

struct Point {};

struct Actor {
    u32 pad;
    u32 id;
};

class BaseCheck {
public:
    void Set2(Point const* start, Point const* end, u32 id);
};

class OtherBase {};

class Check : public BaseCheck, public OtherBase {
public:
    void Set(Point const* start, Point const* end, Actor const* actor);
};

void Check::Set(Point const* start, Point const* end, Actor const* actor) {
    u32 selected;
    if (actor != 0) {
        selected = actor != 0 ? actor->id : 0xFFFFFFFF;
    } else {
        selected = 0xFFFFFFFF;
    }
    Set2(start, end, selected);
}
