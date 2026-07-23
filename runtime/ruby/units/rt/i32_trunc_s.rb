# requires: rt/trunc_checked
def i32_trunc_s(x) = trunc_checked(x, -0x8000_0000, 0x7fff_ffff) & M32
