# Python raises ZeroDivisionError on float division by zero, so IEEE
# division must go through a helper (wasm requires inf/nan, never a trap).
@staticmethod
def fdiv(a, b):
    if b == 0.0:
        if a != a:
            return a
        if a == 0.0:
            return float("nan")
        return math.copysign(math.inf, a) * math.copysign(1.0, b)
    return a / b
