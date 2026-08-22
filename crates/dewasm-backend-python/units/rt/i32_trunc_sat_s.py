# requires: rt/trunc_sat
@staticmethod
def i32_trunc_sat_s(x):
    return Rt.trunc_sat(x, -0x80000000, 0x7FFFFFFF) & Rt.M32
