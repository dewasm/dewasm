# requires: rt/trunc_sat
@staticmethod
def i64_trunc_sat_u(x):
    return Rt.trunc_sat(x, 0, Rt.M64)
