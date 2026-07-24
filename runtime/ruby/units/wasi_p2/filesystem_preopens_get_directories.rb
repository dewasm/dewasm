def p2_filesystem_preopens_get_directories
  @preopens.map { |guest, host| [res_new(P2Dir.new(host)), guest] }
end
