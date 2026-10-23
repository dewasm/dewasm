# frozen_string_literal: true

# Turn a Vernier profile (`script/profile.rb` output, or a `storage/profiles/<id>.json` the
# application recorded) into a flame graph and a summary.
# Usage: `ruby script/flamegraph.rb <profile.json> <out.svg>`.
# The drawing itself is `lib/flame_graph.rb`, which the application's profile pages share.
require_relative "../lib/flame_graph"

profile_path = ARGV.fetch(0)
svg_path = ARGV.fetch(1)

graph = FlameGraph.from_vernier(File.read(profile_path))
puts "samples: #{graph.total}"
puts
puts "self time by kind of code:"
graph.by_kind.each { |kind, w| puts "  #{graph.pct(w).rjust(6)}  #{kind}" }
puts
puts "time with an extension on the stack:"
graph.extensions_on_stack.each { |label, w| puts "  #{graph.pct(w).rjust(6)}  #{label}" }
puts
puts "top self time:"
graph.top_self.each { |f, w| puts "  #{graph.pct(w).rjust(6)}  #{graph.display_name(f)}  (#{graph.label(f)})" }
cfuncs = graph.cfuncs_seen
puts
puts "distinct C functions of Ruby itself seen: #{cfuncs.size}"
puts "  #{cfuncs.first(12).map { |f| graph.func_names[f] }.join(', ')}"

kind = graph.native? ? "the gems' native extensions" : "dewasm-converted extensions"
title = "Rails request handling on #{kind} (#{graph.total} samples)"
File.write(svg_path, graph.svg(title: title))
puts
puts "flame graph: #{svg_path} (#{File.size(svg_path) / 1024} KiB, depth #{graph.depth})"
