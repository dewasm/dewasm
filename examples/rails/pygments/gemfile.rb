# frozen_string_literal: true

# `dewasm_pygments` for a Gemfile:
#
#     `require_relative "pygments/gemfile"`
#     `dewasm_pygments`
#
# It builds the gem's converted CPython under `bundle install` (the rule `dewasm_gem` follows),
# and gives Bundler the gem as a `path:` gem.
# At any other time an out-of-date build is an error that names the command.
require "bundler"
require "pathname"
require_relative "../cext/lib/dewasm_cext/gemfile"
require_relative "lib/dewasm/pygments/build"

module Dewasm
  module Pygments
    module Gemfile
      def dewasm_pygments(**options)
        build = Build.new
        if build.built?
          # nothing to do
        elsif DewasmCExt::Gemfile.build_allowed?
          build.run
        else
          raise Bundler::GemfileError,
                "dewasm-pygments is not built; run `bundle install` (or set DEWASM_CEXT_BUILD=1)"
        end
        gem "dewasm-pygments", path: Pathname.new(Build::ROOT).relative_path_from(Bundler.root).to_s, **options
      end
    end
  end
end

Bundler::Dsl.include(Dewasm::Pygments::Gemfile)
