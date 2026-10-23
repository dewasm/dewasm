# frozen_string_literal: true

# The converted `websocket_mask` against the gem's Ruby `WebSocket::Mask`, per payload size.
# Usage: `ruby websocket_driver.rb <unpacked gem dir>`.
require "benchmark"

gem_dir = File.expand_path(ARGV.fetch(0))
require_relative "../../lib/dewasm_cext"
require File.join(gem_dir, "lib/websocket_mask_wasm")
DewasmCExt::Runtime.new(WebsocketDriverExt, "Init_websocket_mask")
converted = WebSocket::Mask.method(:mask)
WebSocket.send(:remove_const, :Mask)
load File.join(gem_dir, "lib/websocket/mask.rb")
pure = WebSocket::Mask.method(:mask)

mask = "\x12\x34\x56\x78".b
puts "yjit=#{RubyVM::YJIT.enabled?}"
[128, 4096, 65_536].each do |size|
  payload = Random.bytes(size)
  raise "mismatch" unless converted.call(payload.dup, mask) == pure.call(payload.dup, mask)
  n = [2_000_000 / size, 20].max
  tc = Benchmark.realtime { n.times { converted.call(payload, mask) } } / n
  tp = Benchmark.realtime { n.times { pure.call(payload.dup, mask) } } / n
  puts format("%6d B: converted %8.1f us, Ruby fallback %8.1f us", size, tc * 1e6, tp * 1e6)
end
