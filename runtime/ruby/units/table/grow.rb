# table.grow: returns the old size, or 0xffffffff (u32 -1) when the new
# size would exceed the declared max or the u32 range. Sharing one `init`
# object across the new slots is correct: references are values.
def grow(delta, init)
  old = @slots.size
  return 0xffffffff if delta > 0xffffffff - old
  return 0xffffffff if @max && old + delta > @max
  @slots.concat(Array.new(delta, init))
  old
end
