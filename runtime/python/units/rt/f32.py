# Round a double to single precision. struct.pack("<f") raises OverflowError for out-of-float-range doubles instead of returning infinity, and values below the rounding boundary (2^128 - 2^103) must map back to the largest finite f32.
F32_MAX = 3.4028234663852886e38
F32_OVERFLOW = 2.0 ** 128 - 2.0 ** 103

@staticmethod
def f32(x):
    try:
        r = struct.unpack("<f", struct.pack("<f", x))[0]
    except OverflowError:
        r = math.inf if x > 0 else -math.inf
    if math.isinf(r) and math.isfinite(x) and abs(x) < Rt.F32_OVERFLOW:
        r = -Rt.F32_MAX if x < 0 else Rt.F32_MAX
    return r
