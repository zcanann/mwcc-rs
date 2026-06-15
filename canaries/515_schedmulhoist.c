// The instruction scheduler (Phase E) hoists an independent multiply ahead of a
// cheap dependent op to hide its latency: ((a*b)+1)*(c*d) ->
// mullw a*b; mullw c*d; addi +1; mullw, matching mwcc's pipeline schedule.
int schedmulhoist(int a, int b, int c, int d){ return ((a * b) + 1) * (c * d); }
