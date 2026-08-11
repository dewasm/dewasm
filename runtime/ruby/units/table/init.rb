# requires: rt/trap, table/check_range
# `elem` is an Array of table slot values (`[type_key, func]` pairs, or
# `nil` for a `ref.null` item), built once at instantiation/ table.init-population time.
def init(dst, elem, src, len)
  Rt.trap("out of bounds table access") if src + len > elem.size
  check_range(dst, len)
  return if len == 0
  @slots[dst, len] = elem[src, len]
end
