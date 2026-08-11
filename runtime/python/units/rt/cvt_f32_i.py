# requires: rt/f32
# Convert a signed integer to f32 with correct rounding.
# Values beyond
# 2**53 are pre-rounded to odd so the double->single step cannot double-round.
@staticmethod
def cvt_f32_i(v):
    a = abs(v)
    if a < (1 << 53):
        return Rt.f32(float(v))
    sh = a.bit_length() - 53
    hi = a >> sh
    if a != (hi << sh):
        hi |= 1
    if v < 0:
        hi = -hi
    return Rt.f32(float(hi) * (2.0 ** sh))
