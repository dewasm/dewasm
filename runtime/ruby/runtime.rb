# dewasmify Ruby runtime.
#
# Conventions:
# - i32/i64 values are represented as *unsigned* (masked) Ruby Integers.
#   Signed interpretation happens only inside the *_s helpers.
# - f32/f64 values are Ruby Floats; every f32 operation result is rounded
#   back to single precision via Dewasmify.f32.

module Dewasmify
  class Trap < StandardError; end

  class Exit < StandardError
    attr_reader :code

    def initialize(code)
      @code = code
      super("proc_exit(#{code})")
    end
  end

  M32 = 0xffff_ffff
  M64 = 0xffff_ffff_ffff_ffff

  module_function

  def trap(message)
    raise Trap, message
  end

  # -- signed views -----------------------------------------------------

  def s32(x)
    x >= 0x8000_0000 ? x - 0x1_0000_0000 : x
  end

  def s64(x)
    x >= 0x8000_0000_0000_0000 ? x - 0x1_0000_0000_0000_0000 : x
  end

  # -- integer arithmetic ------------------------------------------------

  def i32_div_s(a, b)
    sa = s32(a)
    sb = s32(b)
    trap("integer divide by zero") if sb == 0
    q = sa.abs / sb.abs
    q = -q if (sa < 0) ^ (sb < 0)
    trap("integer overflow") if q > 0x7fff_ffff
    q & M32
  end

  def i32_div_u(a, b)
    trap("integer divide by zero") if b == 0
    a / b
  end

  def i32_rem_s(a, b)
    sa = s32(a)
    sb = s32(b)
    trap("integer divide by zero") if sb == 0
    r = sa.abs % sb.abs
    r = -r if sa < 0
    r & M32
  end

  def i32_rem_u(a, b)
    trap("integer divide by zero") if b == 0
    a % b
  end

  def i64_div_s(a, b)
    sa = s64(a)
    sb = s64(b)
    trap("integer divide by zero") if sb == 0
    q = sa.abs / sb.abs
    q = -q if (sa < 0) ^ (sb < 0)
    trap("integer overflow") if q > 0x7fff_ffff_ffff_ffff
    q & M64
  end

  def i64_div_u(a, b)
    trap("integer divide by zero") if b == 0
    a / b
  end

  def i64_rem_s(a, b)
    sa = s64(a)
    sb = s64(b)
    trap("integer divide by zero") if sb == 0
    r = sa.abs % sb.abs
    r = -r if sa < 0
    r & M64
  end

  def i64_rem_u(a, b)
    trap("integer divide by zero") if b == 0
    a % b
  end

  def i32_rotl(a, b)
    r = b & 31
    ((a << r) | (a >> (32 - r))) & M32
  end

  def i32_rotr(a, b)
    r = b & 31
    ((a >> r) | (a << (32 - r))) & M32
  end

  def i64_rotl(a, b)
    r = b & 63
    ((a << r) | (a >> (64 - r))) & M64
  end

  def i64_rotr(a, b)
    r = b & 63
    ((a >> r) | (a << (64 - r))) & M64
  end

  def i32_clz(x)
    x == 0 ? 32 : 32 - x.bit_length
  end

  def i32_ctz(x)
    x == 0 ? 32 : (x & -x).bit_length - 1
  end

  def i64_clz(x)
    x == 0 ? 64 : 64 - x.bit_length
  end

  def i64_ctz(x)
    x == 0 ? 64 : (x & -x).bit_length - 1
  end

  def popcnt(x)
    x.to_s(2).count("1")
  end

  def sext(x, bits, mask)
    half = 1 << (bits - 1)
    (((x & ((1 << bits) - 1)) ^ half) - half) & mask
  end

  def i32_extend8_s(x) = sext(x, 8, M32)
  def i32_extend16_s(x) = sext(x, 16, M32)
  def i64_extend8_s(x) = sext(x, 8, M64)
  def i64_extend16_s(x) = sext(x, 16, M64)
  def i64_extend32_s(x) = sext(x, 32, M64)
  def i64_extend_i32_s(x) = s32(x) & M64

  # -- float bit conversions ----------------------------------------------

  # Round a double to single precision. MRI's pack("e") overflows straight
  # to infinity for out-of-float-range doubles instead of rounding, so
  # values below the rounding boundary (2^128 - 2^103) are mapped back to
  # the largest finite f32.
  F32_MAX = 3.4028234663852886e+38
  F32_OVERFLOW = 2.0**128 - 2.0**103

  def f32(x)
    r = [x].pack("e").unpack1("e")
    if r.infinite? && x.finite? && x.abs < F32_OVERFLOW
      r = x < 0 ? -F32_MAX : F32_MAX
    end
    r
  end

  # Quiet a NaN (set the quiet bit), preserving sign and payload. The f64
  # quiet bit maps to the f32 quiet bit for NaNs that came from f32.
  def quiet_nan(x)
    f64_from_bits(f64_bits(x) | 0x0008_0000_0000_0000)
  end

  def f64_promote(x)
    x.nan? ? quiet_nan(x) : x
  end

  # MRI's pack("e")/unpack("e") canonicalize NaNs during the double<->float
  # conversion, losing sign and payload. Take a software path for NaNs.
  def f32_from_bits(b)
    if (b & 0x7f80_0000) == 0x7f80_0000 && (b & 0x7f_ffff) != 0
      f64_from_bits(((b >> 31) << 63) | 0x7ff0_0000_0000_0000 | ((b & 0x7f_ffff) << 29))
    else
      [b].pack("L<").unpack1("e")
    end
  end

  def f32_bits(x)
    if x.nan?
      b = f64_bits(x)
      payload = (b >> 29) & 0x7f_ffff
      payload = 0x40_0000 if payload == 0
      (((b >> 63) & 1) << 31) | 0x7f80_0000 | payload
    else
      [x].pack("e").unpack1("L<")
    end
  end

  def f64_from_bits(b)
    [b].pack("Q<").unpack1("E")
  end

  def f64_bits(x)
    [x].pack("E").unpack1("Q<")
  end

  # -- float operations ----------------------------------------------------
  # abs/neg/copysign are bit-level operations: they must preserve NaN
  # payloads, which Float negation does not guarantee.

  def f32_abs(x) = f32_from_bits(f32_bits(x) & 0x7fff_ffff)
  def f32_neg(x) = f32_from_bits(f32_bits(x) ^ 0x8000_0000)
  def f64_abs(x) = f64_from_bits(f64_bits(x) & 0x7fff_ffff_ffff_ffff)
  def f64_neg(x) = f64_from_bits(f64_bits(x) ^ 0x8000_0000_0000_0000)

  def f32_copysign(a, b)
    f32_from_bits((f32_bits(a) & 0x7fff_ffff) | (f32_bits(b) & 0x8000_0000))
  end

  def f64_copysign(a, b)
    f64_from_bits((f64_bits(a) & 0x7fff_ffff_ffff_ffff) | (f64_bits(b) & 0x8000_0000_0000_0000))
  end

  def fmin(a, b)
    return Float::NAN if a.nan? || b.nan?
    if a < b
      a
    elsif b < a
      b
    elsif a == 0.0 && b == 0.0
      (1.0 / a < 0 || 1.0 / b < 0) ? -0.0 : 0.0
    else
      a
    end
  end

  def fmax(a, b)
    return Float::NAN if a.nan? || b.nan?
    if a > b
      a
    elsif b > a
      b
    elsif a == 0.0 && b == 0.0
      (1.0 / a > 0 || 1.0 / b > 0) ? 0.0 : -0.0
    else
      a
    end
  end

  def fsqrt(x)
    return quiet_nan(x) if x.nan?
    return Float::NAN if x < 0.0
    return x if x == 0.0 # preserves -0.0
    Math.sqrt(x)
  end

  def fceil(x)
    return quiet_nan(x) if x.nan?
    return x unless x.finite?
    return x if x == 0.0 # preserves -0.0
    r = x.ceil
    r == 0 && x < 0.0 ? -0.0 : r.to_f
  end

  def ffloor(x)
    return quiet_nan(x) if x.nan?
    return x unless x.finite?
    return x if x == 0.0 # preserves -0.0
    x.floor.to_f
  end

  def ftrunc(x)
    return quiet_nan(x) if x.nan?
    return x unless x.finite?
    return x if x == 0.0 # preserves -0.0
    r = x.truncate
    r == 0 && x < 0.0 ? -0.0 : r.to_f
  end

  def fnearest(x)
    return quiet_nan(x) if x.nan?
    return x unless x.finite?
    return x if x == 0.0
    r = x.round(half: :even)
    r == 0 && x < 0.0 ? -0.0 : r.to_f
  end

  # -- float <-> integer conversions ----------------------------------------

  def trunc_checked(x, min, max)
    trap("invalid conversion to integer") if x.nan?
    trap("integer overflow") if x.infinite?
    t = x.truncate
    trap("integer overflow") if t < min || t > max
    t
  end

  def i32_trunc_s(x) = trunc_checked(x, -0x8000_0000, 0x7fff_ffff) & M32
  def i32_trunc_u(x) = trunc_checked(x, 0, M32)
  def i64_trunc_s(x) = trunc_checked(x, -0x8000_0000_0000_0000, 0x7fff_ffff_ffff_ffff) & M64
  def i64_trunc_u(x) = trunc_checked(x, 0, M64)

  def trunc_sat(x, min, max)
    return 0 if x.nan?
    t = x.infinite? ? (x > 0 ? max : min) : x.truncate
    t.clamp(min, max)
  end

  def i32_trunc_sat_s(x) = trunc_sat(x, -0x8000_0000, 0x7fff_ffff) & M32
  def i32_trunc_sat_u(x) = trunc_sat(x, 0, M32)
  def i64_trunc_sat_s(x) = trunc_sat(x, -0x8000_0000_0000_0000, 0x7fff_ffff_ffff_ffff) & M64
  def i64_trunc_sat_u(x) = trunc_sat(x, 0, M64)

  # Convert a (signed) Integer to f32 with correct rounding. Values beyond
  # 2**53 are pre-rounded to odd so that the double->single step cannot
  # double-round.
  def cvt_f32_i(v)
    a = v.abs
    if a < (1 << 53)
      f32(v.to_f)
    else
      sh = a.bit_length - 53
      hi = a >> sh
      hi |= 1 if a != (hi << sh)
      hi = -hi if v < 0
      f32(hi.to_f * (2.0**sh))
    end
  end

  # MRI's Integer#to_f rounds to nearest-even, which is exactly the wasm
  # convert semantics.
  def cvt_f64_i(v)
    v.to_f
  end

  def f32_demote(x)
    x.nan? ? f32_from_bits((f32_bits(x) & 0x8000_0000) | 0x7fc0_0000) : f32(x)
  end

  def i32_reinterpret_f32(x) = f32_bits(x)
  def f32_reinterpret_i32(x) = f32_from_bits(x)
  def i64_reinterpret_f64(x) = f64_bits(x)
  def f64_reinterpret_i64(x) = f64_from_bits(x)

  # -- linear memory ---------------------------------------------------------

  class Memory
    PAGE_SIZE = 65536

    attr_reader :bytes

    def initialize(min_pages, max_pages)
      @bytes = ("\x00" * (min_pages * PAGE_SIZE)).b
      @max_pages = max_pages && max_pages < 65536 ? max_pages : 65536
    end

    def size
      @bytes.bytesize / PAGE_SIZE
    end

    def grow(delta)
      old = size
      return M32 if old + delta > @max_pages
      @bytes << ("\x00".b * (delta * PAGE_SIZE))
      old
    end

    def check(addr, len)
      Dewasmify.trap("out of bounds memory access") if addr + len > @bytes.bytesize
    end

    def i32_load(a) = (check(a, 4); @bytes.unpack1("L<", offset: a))
    def i64_load(a) = (check(a, 8); @bytes.unpack1("Q<", offset: a))
    # f32 goes through the bit-exact conversion helpers to preserve NaN
    # sign/payload (see Dewasmify.f32_bits).
    def f32_load(a) = Dewasmify.f32_from_bits(i32_load(a))
    def f64_load(a) = (check(a, 8); @bytes.unpack1("E", offset: a))

    def i32_load8_u(a) = (check(a, 1); @bytes.getbyte(a))
    def i32_load8_s(a) = Dewasmify.sext(i32_load8_u(a), 8, M32)
    def i32_load16_u(a) = (check(a, 2); @bytes.unpack1("S<", offset: a))
    def i32_load16_s(a) = Dewasmify.sext(i32_load16_u(a), 16, M32)

    def i64_load8_u(a) = (check(a, 1); @bytes.getbyte(a))
    def i64_load8_s(a) = Dewasmify.sext(i64_load8_u(a), 8, M64)
    def i64_load16_u(a) = (check(a, 2); @bytes.unpack1("S<", offset: a))
    def i64_load16_s(a) = Dewasmify.sext(i64_load16_u(a), 16, M64)
    def i64_load32_u(a) = (check(a, 4); @bytes.unpack1("L<", offset: a))
    def i64_load32_s(a) = Dewasmify.sext(i64_load32_u(a), 32, M64)

    def i32_store(a, v) = (check(a, 4); @bytes[a, 4] = [v].pack("L<"))
    def i64_store(a, v) = (check(a, 8); @bytes[a, 8] = [v].pack("Q<"))
    def f32_store(a, v) = i32_store(a, Dewasmify.f32_bits(v))
    def f64_store(a, v) = (check(a, 8); @bytes[a, 8] = [v].pack("E"))

    def i32_store8(a, v) = (check(a, 1); @bytes.setbyte(a, v & 0xff))
    def i32_store16(a, v) = (check(a, 2); @bytes[a, 2] = [v & 0xffff].pack("S<"))
    def i64_store8(a, v) = (check(a, 1); @bytes.setbyte(a, v & 0xff))
    def i64_store16(a, v) = (check(a, 2); @bytes[a, 2] = [v & 0xffff].pack("S<"))
    def i64_store32(a, v) = (check(a, 4); @bytes[a, 4] = [v & M32].pack("L<"))

    def copy(dst, src, len)
      check(dst, len)
      check(src, len)
      return if len == 0
      @bytes[dst, len] = @bytes.byteslice(src, len)
    end

    def fill(dst, val, len)
      check(dst, len)
      return if len == 0
      @bytes[dst, len] = ((val & 0xff).chr * len).b
    end

    # Also used to initialize active data segments at instantiation time.
    def init(dst, data, src, len)
      Dewasmify.trap("out of bounds memory access") if src + len > data.bytesize
      check(dst, len)
      return if len == 0
      @bytes[dst, len] = data.byteslice(src, len)
    end

    def read_string(ptr, len)
      check(ptr, len)
      @bytes.byteslice(ptr, len)
    end
  end

  # -- function table ----------------------------------------------------------

  class Table
    def initialize(size)
      @types = Array.new(size)
      @funcs = Array.new(size)
    end

    def size = @funcs.size

    # Bounds check for an (active) element segment before initializing it;
    # also catches empty segments whose offset is past the end.
    def check_range(offset, count)
      Dewasmify.trap("out of bounds table access") if offset + count > @funcs.size
    end

    def set(i, type_idx, func)
      Dewasmify.trap("out of bounds table access") if i >= @funcs.size
      @types[i] = type_idx
      @funcs[i] = func
    end

    def call(i, type_idx, *args)
      Dewasmify.trap("undefined element") if i >= @funcs.size
      func = @funcs[i]
      Dewasmify.trap("uninitialized element") if func.nil?
      Dewasmify.trap("indirect call type mismatch") unless @types[i] == type_idx
      func.call(*args)
    end
  end
end
