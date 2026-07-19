// flags: -sdata 0 -sdata2 0
// A definition larger than the small-data threshold still exists when small
// data is disabled. Only its section name/addressing changes to ordinary .data.
short absolute_large_data[8] = { 0, 4500, 0, 900, -1125, -25, -281, 563 };
