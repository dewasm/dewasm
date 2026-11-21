# requires: rt/i64_div_u
def i64_rem_u(a, b) = a - i64_div_u(a, b) * b
