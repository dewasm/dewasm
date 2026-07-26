# requires: rt/trunc_checked
@staticmethod
def i64_trunc_u(x):
    return Rt.trunc_checked(x, 0, Rt.M64)
