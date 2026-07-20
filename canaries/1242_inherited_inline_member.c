// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3
#pragma cplusplus on

struct DataNavi {
    inline void update(int);
};

void DataNavi::update(int n) {}

#pragma cplusplus off

int compiled(void) { return 3; }
