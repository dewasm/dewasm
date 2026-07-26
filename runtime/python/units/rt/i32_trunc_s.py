# requires: rt/trunc_checked
@staticmethod
def i32_trunc_s(x):
    return Rt.trunc_checked(x, -0x80000000, 0x7FFFFFFF) & Rt.M32
