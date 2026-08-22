# requires: memory/size
def grow(delta)
  old = size
  return M32 if old + delta > @max_pages
  @size += delta * PAGE_SIZE
  @buffer.resize(@size) # zero-fills the new tail
  old
end
