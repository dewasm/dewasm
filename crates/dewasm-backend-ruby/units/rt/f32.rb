# Round a double to single precision.
# For a magnitude in [2^-126, 2^127) the two subtractions around x * (2^29 + 1) are Veltkamp splitting: they return x with its significand rounded to 24 bits, to nearest, ties to even, exactly, because a double carries 53 >= 2 * 24 + 2 bits and neither product nor difference leaves the double range here.
# Every other magnitude needs the pack path: below 2^-126 the rounding happens at the fixed subnormal exponent rather than at 24 significant bits, and at or above 2^127 the 24-bit result can be 2^128, which is not an f32. NaN fails both comparisons and lands there too.
# Zero needs no rounding at all and is returned as it came, sign included.
# MRI's pack("e") overflows straight to infinity for out-of-float-range doubles instead of rounding, so values below the rounding boundary (2^128 - 2^103) are mapped back to the largest finite f32.
F32_MAX = 3.4028234663852886e+38
F32_OVERFLOW = 2.0**128 - 2.0**103
F32_MIN_NORMAL = 2.0**-126
F32_SPLIT_LIMIT = 2.0**127
F32_SPLIT = 536870913.0

def f32(x)
  a = x.abs
  if a >= F32_MIN_NORMAL && a < F32_SPLIT_LIMIT
    t = x * F32_SPLIT
    return t - (t - x)
  end
  return x if x == 0.0

  r = [x].pack("e").unpack1("e")
  if r.infinite? && x.finite? && x.abs < F32_OVERFLOW
    r = x < 0 ? -F32_MAX : F32_MAX
  end
  r
end
