# open-flags bit order follows the WIT declaration:
# create=1, directory=2, exclusive=4, truncate=8.
def p2_filesystem_types_method_descriptor_open_at(h, _path_flags, path, open_flags, _desc_flags)
  dir = res(h)
  return [:err, :"not-directory"] unless dir.is_a?(P2Dir)
  full = p2_resolve(dir, path)
  return [:err, :"not-permitted"] if full.nil?
  create = open_flags & 1 != 0
  want_dir = open_flags & 2 != 0
  excl = open_flags & 4 != 0
  trunc = open_flags & 8 != 0
  if want_dir
    return [:err, :"no-entry"] unless File.exist?(full)
    return [:err, :"not-directory"] unless File.directory?(full)
    return [:ok, res_new(P2Dir.new(full))]
  end
  return [:err, :exist] if excl && File.exist?(full)
  if create
    File.open(full, trunc ? "wb" : "ab") {}
  else
    return [:err, :"no-entry"] unless File.exist?(full)
    File.open(full, "wb") {} if trunc
  end
  node = File.directory?(full) ? P2Dir.new(full) : P2Node.new(full)
  [:ok, res_new(node)]
rescue SystemCallError
  [:err, :access]
end
