# requires: memory/size
def grow(delta)
  old = size
  return M32 if old + delta > @max_pages
  @size += delta * PAGE_SIZE
  if @size > @buffer.size
    # Geometric capacity amortizes one-page grow loops; resize zero-fills, so the pages becoming visible are zero (see check).
    @buffer.resize([@size, [@buffer.size * 2, @max_pages * PAGE_SIZE].min].max)
  end
  old
end
