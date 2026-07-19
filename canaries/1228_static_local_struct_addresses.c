// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef void (*Callback)(void);

typedef struct Profile {
    int tag;
    Callback callback;
    int enabled;
} Profile;

void callback(void);

int static_local_struct_addresses(void)
{
    static Profile profile = { 7, (Callback)callback, 1 };
    return 0;
}
