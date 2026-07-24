def p2_filesystem_types_method_descriptor_append_via_stream(h)
  io = File.open(res(h).host_path, "ab")
  [:ok, res_new(OutStream.new(io, true))]
rescue SystemCallError
  [:err, :access]
end
