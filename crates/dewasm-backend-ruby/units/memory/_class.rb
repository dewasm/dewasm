# requires: rt/trap
PAGE_SIZE = 65536

attr_reader :buffer

def wasm_kind = :memory

def initialize(min_pages, max_pages)
  @size = min_pages * PAGE_SIZE
  saved = Warning[:experimental]
  begin
    Warning[:experimental] = false
    @buffer = IO::Buffer.new(@size)
  ensure
    Warning[:experimental] = saved
  end
  @max_pages = max_pages && max_pages < 65536 ? max_pages : 65536
end

# @buffer's capacity may exceed @size (grow resizes it geometrically).
# Every access bounds-checks against @size, so the tail past @size is unreachable and stays zero-filled until grow makes it visible.
def check(addr, len)
  Rt.trap("out of bounds memory access") if addr + len > @size
end
