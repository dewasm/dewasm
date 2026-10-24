# frozen_string_literal: true

require "securerandom"

module RequestProfiler
  COOKIE = "profile_requests"
  ENV_KEY = "request_profiler.record"
  STARTED_KEY = "request_profiler.started_at"
  INTERVAL_US = 200

  def self.elapsed_ms(request)
    started = request.env[STARTED_KEY] or return nil
    ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - started) * 1000).round
  end

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

      # Loaded here, so that a process that never profiles holds no native extension beyond Ruby's.
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
