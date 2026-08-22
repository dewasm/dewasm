# requires: rt/quiet_nan
@staticmethod
def ftrunc(x):
    if x != x:
        return Rt.quiet_nan(x)
    if not math.isfinite(x):
        return x
    if x == 0.0:
        return x
    r = math.trunc(x)
    return -0.0 if (r == 0 and x < 0.0) else float(r)
