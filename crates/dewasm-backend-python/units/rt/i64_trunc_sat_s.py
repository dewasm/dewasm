# requires: rt/trunc_sat
@staticmethod
def i64_trunc_sat_s(x):
    return Rt.trunc_sat(x, -0x8000000000000000, 0x7FFFFFFFFFFFFFFF) & Rt.M64
