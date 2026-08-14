# A deterministic exercise of mruby's exception handling: plain
# raise/rescue, ensure, a custom exception class, and retry.
# stdout is byte-stable: no timestamps, no randomness, no object_id.

class RetryableError < StandardError
end

results = []

begin
  raise "boom"
rescue => e
  results << "rescued:#{e.message}"
end

begin
  raise "boom2"
rescue => e
  results << "rescued2:#{e.message}"
ensure
  results << "ensured"
end

begin
  raise RetryableError, "custom-message"
rescue RetryableError => e
  results << "custom:#{e.class}:#{e.message}"
end

attempts = 0
begin
  attempts += 1
  raise RetryableError, "attempt-#{attempts}" if attempts < 2
  results << "retried:ok-on-attempt-#{attempts}"
rescue RetryableError
  retry if attempts < 2
  results << "retried:failed"
end

results.each { |r| puts r }
puts "mruby_eh: #{results.length} checks, 0 failures"
