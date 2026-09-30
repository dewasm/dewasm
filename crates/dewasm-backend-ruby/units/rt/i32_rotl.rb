# The left shift can leave MRI's fixnum range.
# Masking it before the OR confines the bignum to that one operand.
def i32_rotl(a, b)
  r = b & 31
  ((a << r) & M32) | (a >> (32 - r))
end
