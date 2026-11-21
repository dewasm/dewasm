# requires: rt/trunc_checked
def i32_trunc_s(x) = trunc_checked(x, -2147483649.0, 2147483648.0) & M32
