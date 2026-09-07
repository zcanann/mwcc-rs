// flags: -lang c
// Branches compare promoted byte/halfword registers with word registers.
unsigned uc_u(unsigned char a, unsigned b) { if (a < b) return 17; return 9; }
unsigned uc_u_reverse(unsigned char a, unsigned b) { if (b < a) return 17; return 9; }
unsigned uc_s(unsigned char a, int b) { if (a < b) return 17; return 9; }
unsigned uc_s_reverse(unsigned char a, int b) { if (b < a) return 17; return 9; }
unsigned sc_u(signed char a, unsigned b) { if (a < b) return 17; return 9; }
unsigned sc_u_reverse(signed char a, unsigned b) { if (b < a) return 17; return 9; }
unsigned sc_s(signed char a, int b) { if (a < b) return 17; return 9; }
unsigned sc_s_reverse(signed char a, int b) { if (b < a) return 17; return 9; }
unsigned us_u(unsigned short a, unsigned b) { if (a < b) return 17; return 9; }
unsigned us_u_reverse(unsigned short a, unsigned b) { if (b < a) return 17; return 9; }
unsigned us_s(unsigned short a, int b) { if (a < b) return 17; return 9; }
unsigned us_s_reverse(unsigned short a, int b) { if (b < a) return 17; return 9; }
unsigned ss_u(short a, unsigned b) { if (a < b) return 17; return 9; }
unsigned ss_u_reverse(short a, unsigned b) { if (b < a) return 17; return 9; }
unsigned ss_s(short a, int b) { if (a < b) return 17; return 9; }
unsigned ss_s_reverse(short a, int b) { if (b < a) return 17; return 9; }
