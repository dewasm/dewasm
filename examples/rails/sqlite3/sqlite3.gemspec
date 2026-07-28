Gem::Specification.new do |s|
  s.name = "sqlite3"
  s.version = "2.9.5"
  s.summary = "sqlite3 gem API shim backed by dewasm-converted libsqlite3.wasm"
  s.description = "Implements the subset of the sqlite3-ruby API that Rails uses, " \
                  "on top of SQLite compiled to wasm32-wasi and converted to pure Ruby by dewasm."
  s.authors = ["dewasm examples"]
  s.homepage = "https://github.com/makenowjust/dewasm"
  s.license = "MIT"
  s.files = Dir["lib/**/*.rb"]
  s.require_paths = ["lib"]
  s.required_ruby_version = ">= 3.4"
end
