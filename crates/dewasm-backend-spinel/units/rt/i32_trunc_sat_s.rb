# requires: rt/trunc_sat
def i32_trunc_sat_s(x) = trunc_sat(x, -2147483649.0, 2147483648.0, -0x8000_0000, 0x7fff_ffff) & M32
