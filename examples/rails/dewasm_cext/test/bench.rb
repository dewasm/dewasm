require "benchmark"
require_relative "../lib/dewasm_cext"
require_relative "cache/demo/demo"
rt = DewasmCExt::Runtime.new(DemoExt, "Init_demo")
n = 200_000
s = +"hello world"
t1 = Benchmark.realtime { n.times { Demo.add(1, 2) } }
t2 = Benchmark.realtime { n.times { Demo.upcase!(s) } }
t3 = Benchmark.realtime { n.times { Demo.call(s, "size") } }
puts "Demo.add (2 fixnums, no handle):      #{(t1 / n * 1e6).round(2)} us/call"
puts "Demo.upcase! (string shadow + sync):  #{(t2 / n * 1e6).round(2)} us/call"
puts "Demo.call (rb_intern + rb_funcall):   #{(t3 / n * 1e6).round(2)} us/call"
puts "yjit=#{RubyVM::YJIT.enabled?} stats=#{rt.stats.inspect}"
