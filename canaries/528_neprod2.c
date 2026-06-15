// Deeper non-leaf operand: the product-sum borrows the destination as an
// internal temp while still settling into the scratch for the != idiom.
int neprod2(int a, int b, int c, int d, int x){ return x != a*b + c*d; }
