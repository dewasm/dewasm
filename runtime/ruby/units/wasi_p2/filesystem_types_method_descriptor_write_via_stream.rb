def p2_filesystem_types_method_descriptor_write_via_stream(h, offset)
  io = File.open(res(h).host_path, "r+b")
  io.seek(offset)
  [:ok, res_new(OutStream.new(io, true))]
rescue SystemCallError
  [:err, :access]
end
