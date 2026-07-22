// builds: GC/3.0a3p1
// flags: -Cpp_exceptions off -inline noauto -O4,s -ipa file -RTTI on -schedule off

typedef unsigned int u32;

class PolyInfo {
    unsigned char payload[12];

public:
    virtual ~PolyInfo();
};

class CoreCheck {
    void* poly;
    void* group;
    u32 actor;
    bool same_actor;

public:
    virtual ~CoreCheck();
};

class PolyPass {
public:
    virtual ~PolyPass();

private:
    bool flags[11];
};

class GroupPass {
public:
    virtual ~GroupPass();
    void EnableWater() { group |= 2; }

private:
    u32 group;
};

class SecondaryCheck : public PolyPass, public GroupPass {
public:
    SecondaryCheck();
    virtual ~SecondaryCheck();
};

class CompositeCheck : public PolyInfo, public CoreCheck, public SecondaryCheck {
    unsigned char payload[24];

public:
    CompositeCheck();
    virtual ~CompositeCheck();
};

class WaterCheck : public CompositeCheck {
public:
    WaterCheck();
    virtual ~WaterCheck() {}
};

WaterCheck::WaterCheck() {
    EnableWater();
}
