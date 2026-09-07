// Reduced from Mario Kart Double Dash src/Sato/GeoTree.cpp.
// Original getter bodies; minimal class declarations replace project headers.
// flags: -Cpp_exceptions off -pragma "cats off"
class GeoTree { public: const char* getBmdFileName(); };
class GeoMarioTree1 { public: const char* getBmdFileName(); };
class GeoMarioKinoko1 { public: const char* getBmdFileName(); };

const char *GeoTree::getBmdFileName() {
    static const char *cTreeBmdName = "ki_00.bmd";
    return cTreeBmdName;
}

const char *GeoMarioTree1::getBmdFileName() {
    static const char *cMarioTree1BmdName = "MarioTree1.bmd";
    return cMarioTree1BmdName;
}

const char *GeoMarioKinoko1::getBmdFileName() {
    static const char *cMarioKinoko1BmdName = "/Objects/MarioKinoko1.bmd";
    return cMarioKinoko1BmdName;
}
