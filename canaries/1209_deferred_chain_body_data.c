// flags: -O4,p -inline auto,deferred
// builds: GC/2.6

int chain_body_data;

void chain_target(void) {
    chain_body_data = 1;
}

void later_empty_function(void) {
}

__declspec(section ".dtors") static void *const chain_reference = chain_target;
