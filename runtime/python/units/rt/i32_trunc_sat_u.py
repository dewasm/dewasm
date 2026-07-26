# requires: rt/trunc_sat
@staticmethod
def i32_trunc_sat_u(x):
    return Rt.trunc_sat(x, 0, Rt.M32)
