# The left shift can leave MRI's fixnum range; masking it before the OR confines the bignum to that one operand.
def i32_rotr(a, b)
  r = b & 31
  (a >> r) | ((a << (32 - r)) & M32)
end
