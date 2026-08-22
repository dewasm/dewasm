# requires: rt/s32, rt/trap
@staticmethod
def i32_rem_s(a, b):
    sa = Rt.s32(a)
    sb = Rt.s32(b)
    if sb == 0:
        Rt.trap("integer divide by zero")
    r = abs(sa) % abs(sb)
    if sa < 0:
        r = -r
    return r & Rt.M32
