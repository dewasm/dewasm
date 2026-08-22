# requires: rt/trap
@staticmethod
def i64_rem_u(a, b):
    if b == 0:
        Rt.trap("integer divide by zero")
    return a % b
