// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

struct Entry {
    int first;
    int second;
};

struct Holder {
    int index;
    struct Entry* link;
};

struct Entry entries[32];

void set_link(struct Holder* holder)
{
    holder->link = &entries[holder->index];
}
