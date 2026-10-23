class ApplicationController < ActionController::Base
  include Authentication
  helper_method :profiling?

  allow_browser versions: :modern

  private
    def profiling?
      cookies[RequestProfiler::COOKIE] == "1"
    end
end
