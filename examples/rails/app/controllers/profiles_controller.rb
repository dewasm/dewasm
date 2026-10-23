# The profiles the request profiler recorded, and the flame graph of each.
class ProfilesController < ApplicationController
  allow_unauthenticated_access
  helper_method :profiling?

  PER_PAGE = 25

  def index
    @query = params[:q]
    profiles = RequestProfile.search(@query).order(id: :desc)
    @page = Pagination.new(params[:page], per: PER_PAGE, total: profiles.count)
    @profiles = @page.slice(profiles)
  end

  def show
    @profile = RequestProfile.find(params.expect(:id))
    path = RequestProfiler.path_for(@profile)
    return render plain: "This profile is still being recorded; reload in a moment.", status: :accepted unless File.exist?(path)

    respond_to do |format|
      format.html do
        @graph = FlameGraph.from_vernier(File.read(path))
        @focus = params[:focus].presence
        @svg = @graph.svg(
          width: 1200, focus: @focus,
          title: "#{@profile.http_method} #{@profile.path} on the #{@profile.mode} gems: #{@graph.total} samples",
          link: ->(focus) { profile_path(@profile, focus: focus) }
        )
      end
      format.json { send_file path, type: "application/json", disposition: "attachment" }
    end
  end

  # Turn profiling of every following request on or off (a cookie the Rack layer reads).
  def toggle
    if profiling?
      cookies.delete(RequestProfiler::COOKIE)
    else
      cookies[RequestProfiler::COOKIE] = { value: "1", httponly: true }
    end
    redirect_back_or_to profiles_path
  end
end
