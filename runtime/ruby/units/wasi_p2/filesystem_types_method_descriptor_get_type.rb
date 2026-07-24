def p2_filesystem_types_method_descriptor_get_type(h)
  [:ok, res(h).is_a?(P2Dir) ? :directory : :"regular-file"]
end
