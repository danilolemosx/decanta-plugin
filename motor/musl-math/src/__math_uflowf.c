#include "libm.h"

float __musl_math_uflowf(uint32_t sign)
{
	return __musl_math_xflowf(sign, 0x1p-95f);
}
