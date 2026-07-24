def p2_cli_stdin_get_stdin
  res_new(InStream.new($stdin, false))
end
