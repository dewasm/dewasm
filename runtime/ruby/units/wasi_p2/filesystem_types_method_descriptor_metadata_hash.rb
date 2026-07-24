def p2_filesystem_types_method_descriptor_metadata_hash(h)
  st = File.stat(res(h).host_path)
  [:ok, { "lower" => st.ino, "upper" => st.dev }]
rescue SystemCallError
  [:err, :access]
end
