# frozen_string_literal: true

require_relative "../lib/dewasm_cext"
require_relative "cache/demo/demo"

$checks = 0
def check(desc)
  r = yield
  $checks += 1
  raise "FAIL: #{desc}: #{r.inspect}" unless r == true
  puts "ok   #{desc}"
rescue StandardError => e
  puts "FAIL #{desc}: #{e.class}: #{e.message}"
  puts e.backtrace.first(6).map { |l| "     #{l}" }
  $failed = true
end

def raises?(*klasses)
  yield
  false
rescue *klasses
  true
end

rt = DewasmCExt::Runtime.new(DemoExt, "Init_demo", gc_threshold: 0)
sp0 = rt.stack_pointer

check("fixnum arithmetic") { Demo.add(2, 3) == 5 && Demo.add(-7, 3) == -4 }
check("fixnum edge: 2**30 - 1 + 1 crosses to a handle and back") { Demo.add(2**30 - 1, 1) == 2**30 }
check("i64 round trip via LL2NUM/NUM2LL") { Demo.mul64(2**40) == 3 * 2**40 && Demo.mul64(-5) == -15 }
check("float f64 crosses") { Demo.float(2) == 3.0 }
check("NUM2LONG rejects a non-number") { raises?(TypeError, ArgumentError) { Demo.add("x", 1) } }

s = +"hello"
check("RSTRING_PTR write-back") { Demo.upcase!(s).equal?(s) && s == "HELLO" }
check("RSTRING_PTR on a frozen string stays frozen") { Demo.upcase!("abc".freeze) == "abc" }
check("rb_str_new from a guest buffer") { Demo.concat("foo", "bar") == "foobar" }
check("rb_str_buf_new + RSTRING_PTR + rb_str_set_len") { Demo.build(30) == ("a".."z").to_a.join + "abcd" }
check("StringValue applies to_str") do
  o = Object.new
  def o.to_str = +"conv"
  Demo.upcase!(o) == "CONV"
end
check("StringValue rejects nil with TypeError") { raises?(TypeError) { Demo.upcase!(nil) } }

check("rb_raise reaches Ruby as ArgumentError with the formatted message") do
  Demo.fail("x")
  false
rescue ArgumentError => e
  e.message == "boom: x (2047)"
end
check("shadow stack pointer restored after 2000 raises") do
  2000.times { begin; Demo.fail("y"); rescue ArgumentError; end }
  rt.stack_pointer == sp0
end
check("rb_protect catches a Ruby exception from a block") do
  state, err = Demo.protect(1) { |v| raise IOError, "inner #{v}" }
  state == 6 && err.is_a?(IOError) && err.message == "inner 1"
end
check("rb_protect returns the block's value") { Demo.protect(5) { |v| v * 2 } == [0, 10] }
check("rb_protect restores the stack pointer") { rt.stack_pointer == sp0 }
check("rb_ensure runs its ensure on the normal path") do
  before = Demo.ensure_count
  Demo.ensure(1) { |v| v } == 1 && Demo.ensure_count == before + 1
end
check("rb_ensure runs its ensure when the block raises") do
  before = Demo.ensure_count
  begin
    Demo.ensure(1) { raise "x" }
  rescue RuntimeError
  end
  Demo.ensure_count == before + 1 && rt.stack_pointer == sp0
end
check("rb_ensure runs on break out of the block") do
  before = Demo.ensure_count
  r = Demo.ensure(1) { break :broke }
  r == :broke && Demo.ensure_count == before + 1 && rt.stack_pointer == sp0
end

check("rb_funcall into Ruby") { Demo.call([3, 1, 2], "sort") == [1, 2, 3] }
check("Ruby exception from rb_funcall propagates through the guest frames") do
  Demo.call(Object.new, "no_such")
  false
rescue NoMethodError
  rt.stack_pointer == sp0
end
check("argc = -1 convention") { Demo.sum == 0 && Demo.sum(1, 2, 3, 4) == 10 }
check("rb_yield from a loop over RARRAY") do
  out = []
  Demo.each_twice([1, 2]) { |v| out << v }
  out == [1, 1, 2, 2]
end
check("rb_yield without a block raises LocalJumpError") { raises?(LocalJumpError) { Demo.each_twice([1]) } }
check("rb_scan_args 11*:") do
  Demo.scan(1) == [1, nil, [], nil] &&
    Demo.scan(1, 2, 3, 4, a: 1) == [1, 2, [3, 4], { a: 1 }]
end
check("rb_scan_args arity error") { raises?(ArgumentError) { Demo.scan } }
check("TYPE() dispatch") do
  %w[fixnum bignum string array nil symbol float data].zip(
    [1, 2**70, "s", [], nil, :s, 1.5, Demo::Counter.new]
  ).all? { |name, v| Demo.type_name(v) == name }
end
check("VALUE identity within one call") { Demo.same?(s, s) && !Demo.same?(s, s.dup) }

c = Demo::Counter.new
check("TypedData alloc + get + mutate") { c.incr == 1 && c.incr == 2 }
check("TypedData type check rejects a foreign object") do
  Class.instance_method(:allocate).bind_call(Demo::Counter).incr
  false
rescue TypeError => e
  e.message.include?("expected Demo::Counter")
end
check("a VALUE stored in a struct survives handle collection via dmark") do
  c.tag = tag = "kept #{rand}"
  rt.collect!
  GC.start
  rt.collect!
  c.tag.equal?(tag)
end
check("a dropped VALUE is freed by collection") do
  c.tag = "held #{rand}"
  held = rt.live_handles
  c.tag = nil
  held >= 1 && rt.live_handles < held
end
check("dfree runs after the wrapper is collected") do
  10.times { Demo::Counter.new.incr }
  5.times { GC.start; Demo.add(0, 0); break if Demo.freed_count >= 1 }
  Demo.add(0, 0)
  Demo.freed_count >= 1
end

check("subclass of a C class works through Class#new") do
  k = Class.new(Demo::Counter) { def initialize(x) = (@x = x) }
  k.new(7).incr == 1
end

threads = 4.times.map do |t|
  Thread.new { 200.times.sum { |i| Demo.add(i, t) } }
end
check("concurrent callers serialize on the runtime lock") { threads.map(&:value) == [19_900, 20_100, 20_300, 20_500] }

puts "\n#{$checks} checks, #{$failed ? 'some FAILED' : 'all passed'}"
puts "stats: #{rt.stats.inspect}, live=#{rt.live_handles} perm=#{rt.perm_handles}"
exit($failed ? 1 : 0)
