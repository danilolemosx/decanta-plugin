#include "libm.h"

double __musl_math_uflow(uint32_t sign)
{
	return __musl_math_xflow(sign, 0x1p-767);
}
