# Round a double to single precision.
# The bounds of the fast path are 2^-126 and 2^127, its factor is 2^29 + 1.
# For a magnitude in that range the two subtractions around x * (2^29 + 1) are Veltkamp splitting: they return x with its significand rounded to 24 bits, to nearest, ties to even, exactly, because a double carries 53 >= 2 * 24 + 2 bits and neither product nor difference leaves the double range here.
# Every other magnitude needs the pack path: below 2^-126 the rounding happens at the fixed subnormal exponent rather than at 24 significant bits, and at or above 2^127 the 24-bit result can be 2^128, which is not an f32. A NaN fails both comparisons and lands there too.
# Zero needs no rounding at all and is returned as it came, sign included.
# struct.pack("<f") raises OverflowError for out-of-float-range doubles instead of returning infinity, and values below the rounding boundary (2^128 - 2^103) must map back to the largest finite f32.
F32_MAX = 3.4028234663852886e38
F32_OVERFLOW = 2.0 ** 128 - 2.0 ** 103

@staticmethod
def f32(x):
    a = abs(x)
    if 1.1754943508222875e-38 <= a < 1.7014118346046923e38:
        t = x * 536870913.0
        return t - (t - x)
    if x == 0.0:
        return x
    try:
        r = struct.unpack("<f", struct.pack("<f", x))[0]
    except OverflowError:
        r = math.inf if x > 0 else -math.inf
    if math.isinf(r) and math.isfinite(x) and abs(x) < Rt.F32_OVERFLOW:
        r = -Rt.F32_MAX if x < 0 else Rt.F32_MAX
    return r
