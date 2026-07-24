def p2_cli_environment_get_environment
  @env.map { |k, v| [k, v] }
end
