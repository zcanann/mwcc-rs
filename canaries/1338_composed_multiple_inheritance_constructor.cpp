// flags: -inline noauto -O4,s -ipa file -Cpp_exceptions off
// builds: GC/3.0a3p1

class PrimaryCheck {
    void* poly;
    void* group;
    unsigned int actor;
    bool same_actor;

public:
    PrimaryCheck();
    void SetPoly(void* value) { poly = value; }
    void SetGroup(void* value) { group = value; }
    virtual ~PrimaryCheck();
};

class PolyInfo {
    unsigned short poly_index;
    unsigned short bg_index;
    void* owner;
    unsigned int actor;

public:
    virtual ~PolyInfo();
};

class LineCheck : public PrimaryCheck, public PolyInfo {
    unsigned char payload[52];

public:
    LineCheck();
    virtual ~LineCheck();
};

class PolyPass {
public:
    virtual ~PolyPass();
    void SetCamera();

private:
    bool flags[11];
};

class GroupPass {
public:
    virtual ~GroupPass();

private:
    unsigned int group;
};

class SecondaryCheck : public PolyPass, public GroupPass {
public:
    SecondaryCheck();
    void* GetPoly();
    void* GetGroup();
    virtual ~SecondaryCheck();
};

class ComposedCheck : public LineCheck, public SecondaryCheck {
public:
    ComposedCheck();
    virtual ~ComposedCheck();
};

class CameraCheck : public ComposedCheck {
public:
    CameraCheck();
    virtual ~CameraCheck();
};

ComposedCheck::ComposedCheck() {
    SetPoly(GetPoly());
    SetGroup(GetGroup());
}

CameraCheck::CameraCheck() {
    SetCamera();
}
