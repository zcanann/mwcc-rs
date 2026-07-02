/* A leading-dot float literal (`.5`) omits the integer part — C allows it;
 * the lexer must not split it into Dot + IntegerLiteral. */
double f(double x)
{
	return x * .5;
}

double g(double x)
{
	return x + .25;
}
