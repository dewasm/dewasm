ENV["RAILS_ENV"] ||= "test"
require_relative "../config/environment"
require "rails/test_help"

module ActiveSupport
  class TestCase
    # The tests run in this one process: each is a sequence of requests through the full stack.
  end
end
