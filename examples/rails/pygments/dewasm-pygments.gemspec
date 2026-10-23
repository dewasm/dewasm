# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "dewasm-pygments"
  spec.version = "0.1.0"
  spec.authors = ["Hiroya Fujinami"]
  spec.summary = "Pygments on a CPython converted to Ruby by dewasm"
  spec.description = "Syntax highlighting by Pygments, run by a CPython that dewasm converted to " \
                     "Ruby and Wizer pre-initialized: Ruby code only, no native extension."
  spec.license = "MIT"
  spec.required_ruby_version = ">= 3.4"
  spec.files = Dir["lib/**/*.rb", "ext/*.c"]
  spec.require_paths = ["lib"]
end
