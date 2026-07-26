# requires: rt/s32, rt/trap
@staticmethod
def i32_div_s(a, b):
    sa = Rt.s32(a)
    sb = Rt.s32(b)
    if sb == 0:
        Rt.trap("integer divide by zero")
    q = abs(sa) // abs(sb)
    if (sa < 0) ^ (sb < 0):
        q = -q
    if q > 0x7FFFFFFF:
        Rt.trap("integer overflow")
    return q & Rt.M32
