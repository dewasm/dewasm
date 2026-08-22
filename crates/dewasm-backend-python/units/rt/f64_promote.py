# requires: rt/quiet_nan
@staticmethod
def f64_promote(x):
    return Rt.quiet_nan(x) if x != x else x
