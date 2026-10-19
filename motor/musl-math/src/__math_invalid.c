#include "libm.h"

double __musl_math_invalid(double x)
{
	return (x - x) / (x - x);
}
