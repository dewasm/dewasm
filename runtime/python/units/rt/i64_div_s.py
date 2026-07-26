# requires: rt/s64, rt/trap
@staticmethod
def i64_div_s(a, b):
    sa = Rt.s64(a)
    sb = Rt.s64(b)
    if sb == 0:
        Rt.trap("integer divide by zero")
    q = abs(sa) // abs(sb)
    if (sa < 0) ^ (sb < 0):
        q = -q
    if q > 0x7FFFFFFFFFFFFFFF:
        Rt.trap("integer overflow")
    return q & Rt.M64
