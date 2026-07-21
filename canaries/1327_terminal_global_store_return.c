// builds: GC/1.3
// flags: -pragma "cats off" -Cpp_exceptions off -sdata 0 -sdata2 0 -inline auto,deferred

static int status;

extern int initialize(void);
extern void welcome(void);
extern void service(void);
extern int finish(void);

int guarded_store_then_return(void)
{
    status = initialize();
    if (status == 0) {
        welcome();
        service();
    }
    status = finish();
    return status;
}
