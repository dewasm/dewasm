# requires: rt/trap
# `type_key` is a symbol interned from the function type's shape (not a
# type index): tables can be shared across modules, whose index spaces
# differ.
def call(i, type_key, *args)
  Rt.trap("undefined element") if i >= @funcs.size
  func = @funcs[i]
  Rt.trap("uninitialized element") if func.nil?
  Rt.trap("indirect call type mismatch") unless @types[i] == type_key
  func.call(*args)
end
