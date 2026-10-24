ENV["BUNDLE_GEMFILE"] ||= File.expand_path("../Gemfile", __dir__)

require "bundler/setup"

# The Ruby sides of `nio4r`, `websocket-driver`, and `racc`, in place of their C extensions.
ENV["NIO4R_PURE"] = "true"
$LOAD_PATH.unshift File.expand_path("../lib/fallbacks", __dir__)
