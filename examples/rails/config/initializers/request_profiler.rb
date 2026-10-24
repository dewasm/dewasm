# frozen_string_literal: true

# The clock is outside the profiler, so the time in the page header covers the profiler's work.
Rails.application.config.middleware.insert_before 0, RequestProfiler::Middleware
Rails.application.config.middleware.insert_before 0, RequestProfiler::Clock
