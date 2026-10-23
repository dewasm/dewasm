# frozen_string_literal: true

# `dewasm_gem` for a Gemfile:
#
#     `require_relative "cext/lib/dewasm_cext/gemfile"`
#     `gem "sqlite3"`
#     `group :dewasm do`
#     `  dewasm_gem "sqlite3", "2.10.1"`
#     `end`
#
# The gem is fetched at that version, its C extension is compiled to wasm against the dewasm
# `ruby.h` and converted to Ruby, and Bundler gets the result as a `path:` gem under
# `vendor/dewasm/`.
# The line replaces a `gem` line above it that names the same gem, since Bundler refuses one gem
# from two sources; a gem no line names, such as a dependency of `rails`, is added.
# So `rails` depends on it as usual, and `require "sqlite3"` loads the converted extension.
#
# In a group that `bundle config set --local without <group>` excludes, the line does nothing.
# The application then runs on the gem's own native extension, as any Rails application does.
#
# The build runs under `bundle install`, `bundle update`, and `bundle lock`, or with
# `DEWASM_CEXT_BUILD=1`; at any other time an out-of-date build is an error that names the command.
# So an application boot never compiles.
require "bundler"
require "pathname"
require_relative "builder"

module DewasmCExt
  module Gemfile
    BUILD_COMMANDS = %w[install update lock].freeze

    def dewasm_gem(name, version, **options)
      return if (@groups & Bundler.settings[:without]).any?

      root = Bundler.root
      builder = Builder.new(name, version, cache_dir: Builder.default_cache(root),
                                           lock_path: File.join(root, "dewasm-gems.lock"))
      if builder.built?
        # nothing to do
      elsif DewasmCExt::Gemfile.build_allowed?
        builder.ensure_built
      else
        raise Bundler::GemfileError,
              "#{name} #{version} is not built for dewasm; run `bundle install` (or set DEWASM_CEXT_BUILD=1)"
      end
      # The gem keeps the groups of the line it replaces, so `Bundler.require` loads it as before.
      replaced = @dependencies.find { |dependency| dependency.name == name }
      @dependencies.delete(replaced) if replaced
      groups = Array(options.delete(:group)) + Array(options.delete(:groups)) + (replaced&.groups || [:default])
      gem name, version, path: Pathname.new(builder.gem_dir).relative_path_from(root).to_s, group: groups, **options
    end

    def self.build_allowed?
      return true if ENV["DEWASM_CEXT_BUILD"] == "1"
      File.basename($PROGRAM_NAME) == "bundle" && BUILD_COMMANDS.include?(ARGV.first)
    end
  end
end

Bundler::Dsl.include(DewasmCExt::Gemfile)
