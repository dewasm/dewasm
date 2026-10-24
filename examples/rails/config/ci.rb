CI.run do
  step "Setup", "bin/setup --skip-server"
  step "Security: Gem audit", "bin/bundler-audit"
  step "Security: Importmap vulnerability audit", "bin/importmap audit"
end
