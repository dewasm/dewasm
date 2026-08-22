# requires: rt/trunc_checked
@staticmethod
def i32_trunc_u(x):
    return Rt.trunc_checked(x, 0, Rt.M32)
