// Register negation, signed-zero behavior, and contraction selection.
// flags: -Cpp_exceptions off -pragma "cats off" -fp_contract off
float mul_left_f(float a,float b,float c) {return -a*b;}
float mul_right_f(float a,float b,float c) {return a*-b;}
float mul_both_f(float a,float b,float c) {return -a*-b;}
float mul_outer_f(float a,float b,float c) {return -(a*b);}
float add_left_f(float a,float b,float c) {return -a+b;}
float add_right_f(float a,float b,float c) {return a+-b;}
float add_both_f(float a,float b,float c) {return -a+-b;}
float negprod_add_f(float a,float b,float c) {return -(a*b)+c;}
double mul_left_d(double a,double b,double c) {return -a*b;}
double mul_right_d(double a,double b,double c) {return a*-b;}
double mul_both_d(double a,double b,double c) {return -a*-b;}
double mul_outer_d(double a,double b,double c) {return -(a*b);}
double add_left_d(double a,double b,double c) {return -a+b;}
double add_right_d(double a,double b,double c) {return a+-b;}
double add_both_d(double a,double b,double c) {return -a+-b;}
double negprod_add_d(double a,double b,double c) {return -(a*b)+c;}
