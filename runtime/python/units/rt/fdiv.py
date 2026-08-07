# requires: rt/quiet_nan
# Python raises ZeroDivisionError on float division by zero, so IEEE
# division must go through a helper (wasm requires inf/nan, never a trap).
@staticmethod
def fdiv(a, b):
    if b == 0.0:
        # A NaN dividend must yield a quiet NaN (arithmetic-NaN result); the
        # short-circuit skips the hardware division that would quiet it, so
        # quiet it explicitly, mirroring Ruby's FPU-quieted path.
        if a != a:
            return Rt.quiet_nan(a)
        if a == 0.0:
            return float("nan")
        return math.copysign(math.inf, a) * math.copysign(1.0, b)
    return a / b
