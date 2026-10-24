require "test_helper"

# The application driven through its own stack: the index, a sign-in, a post with HTML in its
# body, a comment, a profiled request with its flame graph, and the `/stats` report.
# On the converted gems, `/stats` must show no native extension beyond Ruby's own.
# With the `dewasm` group excluded, the four gems' own extensions must be the ones loaded.
class BlogTest < ActionDispatch::IntegrationTest
  GEMS = %w[sqlite3 bcrypt herb nokogiri].freeze # the gems with a native extension to find

  setup { Rails.application.load_seed }

  def converted? = defined?(DewasmCExt::Runtime) ? true : false

  def sign_in
    post session_path, params: { email_address: "author@example.com", password: "secret" }
    assert_not_equal new_session_url, response.location, "sign-in failed"
  end

  test "the index renders the seeded posts through Herb, with their bodies sanitized" do
    get posts_path
    assert_response :success
    assert_includes response.body, "Hello from pure Ruby"
    assert_includes response.body, "日本語の投稿"
    assert_not_includes response.body, "<script>"
  end

  test "the index is paged and searched" do
    get posts_path
    assert_select "article.post", PostsController::PER_PAGE
    assert_select "nav.pagination a[rel=next]"
    get posts_path(page: 3)
    assert_select "article.post", Post.count - 2 * PostsController::PER_PAGE
    assert_select "nav.pagination a[rel=next]", 0

    get posts_path(q: "日本語")
    assert_select "article.post", 1
    assert_select "p.meta", text: /1 post matching/
    get posts_path(q: "no such post")
    assert_select "article.post", 0
  end

  test "profiles are searched by path and mode" do
    RequestProfile.create!(http_method: "GET", path: "/posts/7", mode: "native", status: 200)
    RequestProfile.create!(http_method: "POST", path: "/session", mode: "dewasm", status: 302)
    get profiles_path(q: "GET /posts")
    assert_select "tbody tr", 1
    get profiles_path(q: "dewasm")
    assert_select "tbody tr", 1
    get profiles_path
    assert_select "tbody tr", 2
  end

  test "bcrypt checks the password" do
    post session_path, params: { email_address: "author@example.com", password: "wrong" }
    assert_redirected_to new_session_path
    sign_in
  end

  test "a post and a comment are stored and rendered" do
    sign_in
    post posts_path, params: { post: { title: "Posted over HTTP",
                                       body: "<p>Hello <em>there</em></p><img src=x onerror=alert(1)>" } }
    created = Post.find_by!(title: "Posted over HTTP")
    assert_redirected_to post_path(created)

    post post_comments_path(created), params: { comment: { author: "reader", body: "Nice <b>post</b>" } }
    assert_redirected_to post_path(created)

    get post_path(created)
    body = css_select("article .body").first.to_html
    assert_includes body, "Hello <em>there</em>"
    assert_not_includes body, "onerror"
    assert_includes response.body, "Nice <b>post</b>"
  end

  # The request the ReActionView client makes when the `code` state of the post page is on.
  SLOTS_ACCEPT = "application/vnd.herb.slots+json".freeze

  def code_slots_headers
    { "Accept" => SLOTS_ACCEPT, "Herb-State" => { "app/views/posts/show.html.erb" => { "code" => true } }.to_json }
  end

  # Every string in a `slots` response, joined: the values the client would put into the page.
  def slot_text(body)
    strings = []
    walk = lambda do |value|
      case value
      when Hash then value.each_value { |v| walk.call(v) }
      when Array then value.each { |v| walk.call(v) }
      when String then strings << value
      end
    end
    walk.call(JSON.parse(body))
    strings.join("\n")
  end

  test "the post page's View HTML state shows the served HTML, highlighted by Pygments" do
    post = Post.find_by!(title: "Hello from pure Ruby")
    get post_path(post)
    assert_select "label.check input[type=checkbox]:not([checked])"
    assert_select "article .body pre.highlight", 0
    assert_select "head style", /\.highlight \.nt/

    get post_path(post), params: { format: :slots }, headers: code_slots_headers
    assert_response :success
    slots = slot_text(response.body)
    assert_includes slots, %(<span class="nt">b</span>), "the source is not highlighted"
    assert_not_includes slots, "nt\">script", "sanitize left the script tag in"
  end

  test "a profiled request records a flame graph" do
    post toggle_profiles_path
    get posts_path
    record = RequestProfile.last
    assert_includes response.body, profile_path(record), "the profiled page does not link to its profile"
    assert_equal converted? ? "dewasm" : "native", record.mode
    assert_match(/\d+ ms/, css_select(".timing").text, "the header does not show the time")
    assert_no_difference("RequestProfile.count", "a profile page was recorded") do
      get profiles_path
      get profile_path(record)
    end
    post toggle_profiles_path

    get profiles_path
    assert_select "table code", text: %r{GET /posts}

    get profile_path(record)
    assert_response :success
    assert_includes response.body, "<svg"
    if converted?
      assert_includes response.body, "Sqlite3Ext#", "no converted SQLite frame"
    else
      assert_not_includes response.body, "Sqlite3Ext#"
      assert_match(/native gem: (sqlite3|nokogiri|herb|bcrypt)/, response.body, "no native extension frame")
    end

    get post_path(Post.first), params: { format: :slots, profile: 1 }, headers: code_slots_headers
    post_record = RequestProfile.last
    get profile_path(post_record)
    if converted?
      assert_includes response.body, "PygmentsRbExt#", "no converted CPython frame in the HTML view's profile"
    else
      assert_not_includes response.body, "PygmentsRbExt#"
    end
  ensure
    FileUtils.rm_f(RequestProfiler.path_for(record)) if record
    FileUtils.rm_f(RequestProfiler.path_for(post_record)) if post_record
  end

  test "/stats reports which native extensions the process holds" do
    get stats_path
    stats = response.parsed_body
    assert_includes stats["erb_handler"], "ReActionView", "templates do not go through ReActionView"
    assert stats["erb_implementation"].end_with?("::Herb"), "templates are not compiled by Herb"
    outside = stats["native_extensions"]["outside_ruby"]
    if converted?
      assert_empty outside, "native extensions outside Ruby"
    else
      missing = GEMS.reject { |gem| outside.any? { |path| path.include?("/#{gem}-") } }
      assert_empty missing, "these gems' own extensions are not loaded"
    end
  end
end
