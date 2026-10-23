# frozen_string_literal: true

# `websocket-driver`: the measurement behind keeping a gem's Ruby fallback over its converted
# extension (`agents/experiments.md`, `cext-vs-ruby-fallback`).
# The application does not use this build.
# `cext/test/bench/websocket_driver.rb` compares it with the gem's own Ruby.
DewasmCExt::Recipe.register "websocket-driver" do
  checksum "0.8.2", "97c556b019bf3410b4961002ac501621e9322d3f8a7bc02161a09301cc4c4146"

  def configure
    self.sources = %w[ext/websocket-driver/websocket_mask.c]
    self.extension = "websocket_mask"
  end
end
