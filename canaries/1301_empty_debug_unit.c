// An empty translation unit emits no `.line`/`.debug` sections even when
// debug information is requested; only the ordinary comment metadata remains.
// builds: GC/2.7 GC/3.0a3
// flags: -sym on
