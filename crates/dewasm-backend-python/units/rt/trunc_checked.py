# requires: rt/trap
@staticmethod
def trunc_checked(x, lo, hi):
    if x != x:
        Rt.trap("invalid conversion to integer")
    if math.isinf(x):
        Rt.trap("integer overflow")
    t = math.trunc(x)
    if t < lo or t > hi:
        Rt.trap("integer overflow")
    return t
