# requires: memory/size
def grow(delta)
  old = size
  return M32 if old + delta > @max_pages
  @bytes << ("\x00".b * (delta * PAGE_SIZE))
  old
end
