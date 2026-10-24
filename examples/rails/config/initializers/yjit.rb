# frozen_string_literal: true

# Rails 8.1 leaves YJIT off in development; the converted extensions run about twice as fast on it.
RubyVM::YJIT.enable if defined?(RubyVM::YJIT) && !RubyVM::YJIT.enabled?
