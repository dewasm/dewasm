ENV["BUNDLE_GEMFILE"] ||= File.expand_path("../Gemfile", __dir__)

require "bundler/setup" # Set up gems listed in the Gemfile.

# The Ruby sides of `nio4r`, `websocket-driver`, and `racc`, in place of their C extensions.
# `lib/fallbacks/` holds a stub per extension whose load fails, which is what the gems check.
ENV["NIO4R_PURE"] = "true"
$LOAD_PATH.unshift File.expand_path("../lib/fallbacks", __dir__)
