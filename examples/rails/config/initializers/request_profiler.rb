# frozen_string_literal: true

# The on-demand request profiler behind `/profiles`; see `lib/request_profiler.rb`.
# It sits first in the Rack stack, so a profile covers everything down to the converted SQLite.
# The clock sits outside it, so the time in the page header covers the profiler's own work too.
Rails.application.config.middleware.insert_before 0, RequestProfiler::Middleware
Rails.application.config.middleware.insert_before 0, RequestProfiler::Clock
