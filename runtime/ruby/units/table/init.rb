# requires: rt/trap, table/check_range
# `elem` is an Array of `[type_key, func]` pairs (or `nil` for a `ref.null`
# item), built once at instantiation/table.init-population time.
def init(dst, elem, src, len)
  Rt.trap("out of bounds table access") if src + len > elem.size
  check_range(dst, len)
  return if len == 0
  elem[src, len].each_with_index do |item, k|
    ty, func = item
    @types[dst + k] = ty
    @funcs[dst + k] = func
  end
end
