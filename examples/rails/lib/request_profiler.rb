# frozen_string_literal: true

require "securerandom"

# Record a Vernier profile of one request, on demand, for the application's profile pages.
#
# Profiling is on for a request when the `profile_requests` cookie is set (the button in the
# page header) or the query carries `profile=1`.
# The record (`RequestProfile`) is created before the request runs, so the page being served can
# link to its own profile; the profile file lands under `storage/profiles/` once the request ends.
# Vernier is loaded on the first profiled request only, so a process that never profiles holds
# no native extension beyond Ruby's own; `NativeExtensions` reports it apart as instrumentation.
module RequestProfiler
  COOKIE = "profile_requests"
  ENV_KEY = "request_profiler.record"
  STARTED_KEY = "request_profiler.started_at"
  INTERVAL_US = 200

  # The time since the request entered the Rack stack, for the page header.
  # The layout renders after the page's own template, so the figure covers the action and the view.
  def self.elapsed_ms(request)
    started = request.env[STARTED_KEY] or return nil
    ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  end

  # First in the Rack stack: notes when a request entered, for `elapsed_ms`.
  class Clock
    def initialize(app)
      @app = app
    end

    def call(env)
      env[STARTED_KEY] = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      @app.call(env)
    end
  end

  def self.directory
    dir = Rails.root.join("storage/profiles")
    FileUtils.mkdir_p(dir)
    dir
  end

  def self.path_for(record) = directory.join("#{record.id}.json")

  def self.wanted?(request)
    page?(request) && (request.cookies[COOKIE] == "1" || request.params["profile"] == "1")
  end

  # A request for a page, as against an asset, a service worker, or an image: those are the
  # requests with no file extension outside `/assets/`.
  # The profile pages themselves are left out too, so reading profiles adds none.
  def self.page?(request)
    path = request.path
    !path.start_with?("/assets/", "/profiles") && File.extname(path).empty?
  end

  class Middleware
    def initialize(app)
      @app = app
    end

    def call(env)
      request = Rack::Request.new(env)
      return @app.call(env) unless RequestProfiler.wanted?(request)

      require "vernier"
      record = Rails.application.executor.wrap do
        RequestProfile.create!(http_method: request.request_method, path: request.fullpath,
                               mode: NativeExtensions.mode)
      end
      env[ENV_KEY] = record
      started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      response = nil
      result = Vernier.profile(interval: INTERVAL_US, allocation_interval: 0) do
        response = @app.call(env)
        # The body is rendered inside the profiled window, as the server would render it.
        status, headers, body = response
        chunks = []
        body.each { |chunk| chunks << chunk }
        body.close if body.respond_to?(:close)
        response = [status, headers, chunks]
      end
      duration = (Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000
      result.write(out: RequestProfiler.path_for(record).to_s)
      Rails.application.executor.wrap do
        record.update!(status: response[0], duration_ms: duration, sample_count: result.total_samples)
      end
      response
    end
  end
end
