# requires: rt/trunc_checked
@staticmethod
def i64_trunc_s(x):
    return Rt.trunc_checked(x, -0x8000000000000000, 0x7FFFFFFFFFFFFFFF) & Rt.M64
