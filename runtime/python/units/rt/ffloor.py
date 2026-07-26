# requires: rt/quiet_nan
@staticmethod
def ffloor(x):
    if x != x:
        return Rt.quiet_nan(x)
    if not math.isfinite(x):
        return x
    if x == 0.0:
        return x
    return float(math.floor(x))
