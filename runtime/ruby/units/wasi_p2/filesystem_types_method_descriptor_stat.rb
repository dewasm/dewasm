def p2_filesystem_types_method_descriptor_stat(h)
  st = File.stat(res(h).host_path)
  type =
    if st.directory?
      :directory
    elsif st.file?
      :"regular-file"
    else
      :unknown
    end
  [:ok, {
    "type" => type,
    "link-count" => st.nlink,
    "size" => st.size,
    "data-access-timestamp" => nil,
    "data-modification-timestamp" => nil,
    "status-change-timestamp" => nil,
  }]
rescue SystemCallError
  [:err, :access]
end
