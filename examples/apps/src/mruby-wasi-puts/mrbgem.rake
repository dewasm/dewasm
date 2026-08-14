MRuby::Gem::Specification.new('mruby-wasi-puts') do |spec|
  spec.license = 'MIT'
  spec.author = 'dewasm'
  spec.summary = "Kernel#puts on top of core Kernel#print, standing in for mruby-io's " \
                  '(excluded on wasi) $stdout.puts'
end
