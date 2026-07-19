// flags: -O4,p -inline auto,deferred

int deferred_data_symbol;

int read_deferred_data_symbol(void) {
    return deferred_data_symbol;
}
