// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

void first(void);
void second(void);
extern int left;
extern int right;
typedef void (*Function)(void);

typedef struct Entry {
    int tag;
    int* address;
} Entry;

int static_local_address_tables(void)
{
    static Function functions[] = { first, second };
    static Entry entries[] = { { 1, &left }, { 2, &right } };
    return 0;
}
