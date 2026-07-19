// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef void (*Callback)(void);

typedef struct Profile {
    int tag;
    Callback callback;
} Profile;

void callback(void);

Profile grouped_cast_function_address = {
    7,
    ((Callback)&callback),
};
