# frozen_string_literal: true

require "bundled_gems"
require "set"

module NativeExtensions
  SHIPPED_GEMS = (Gem::Specification.default_stubs.map(&:name) + Gem::BUNDLED_GEMS::SINCE.keys).to_set
  ARCHDIR = "#{RbConfig::CONFIG["archdir"]}/"
  INSTRUMENTATION_GEMS = %w[vernier].to_set

  def self.loaded
    $LOADED_FEATURES.grep(/\.(bundle|so|dylib)\z/)
  end

  def self.gem_of(path)
    Gem::Specification.find do |spec|
      path.start_with?("#{spec.full_gem_path}/") || path.start_with?("#{spec.extension_dir}/")
    end
  end

  def self.outside_ruby
    loaded.reject do |path|
      next true if path.start_with?(ARCHDIR) || !path.include?("/")
      spec = gem_of(path)
      spec && (SHIPPED_GEMS.include?(spec.name) || INSTRUMENTATION_GEMS.include?(spec.name))
    end
  end

  def self.mode = defined?(DewasmCExt::Runtime) ? "dewasm" : "native"
end
