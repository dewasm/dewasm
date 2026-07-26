# requires: rt/s64, rt/trap
@staticmethod
def i64_rem_s(a, b):
    sa = Rt.s64(a)
    sb = Rt.s64(b)
    if sb == 0:
        Rt.trap("integer divide by zero")
    r = abs(sa) % abs(sb)
    if sa < 0:
        r = -r
    return r & Rt.M64
