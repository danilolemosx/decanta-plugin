#include "libm.h"

float __musl_math_invalidf(float x)
{
	return (x - x) / (x - x);
}
