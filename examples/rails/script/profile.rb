# frozen_string_literal: true

require "vernier"
require "action_dispatch/testing/integration"

out = ARGV.fetch(0)
rounds = (ARGV[1] || "20").to_i

ActionDispatch::IntegrationTest.app = Rails.application
session = ActionDispatch::Integration::Session.new(Rails.application)
session.host! "127.0.0.1"

def token_of(session)
  session.response.body[/name="csrf-token" content="([^"]+)"/, 1]
end

session.get "/session/new"
session.post "/session", params: { authenticity_token: token_of(session),
                                   email_address: "author@example.com", password: "secret" }
session.get "/posts/new"
session.post "/posts", params: { authenticity_token: token_of(session),
                                 post: { title: "Profiled", body: "<p>body <b>bold</b></p>" } }
post_path = session.response.location.sub(%r{\Ahttps?://[^/]+}, "")
session.get post_path
session.get "/posts"

Vernier.profile(out: out, interval: 200, allocation_interval: 0) do
  rounds.times do
    session.get "/posts"
    session.get post_path
    session.get "/session/new"
    session.post "/session", params: { authenticity_token: token_of(session),
                                       email_address: "author@example.com", password: "secret" }
  end
end

puts "profile: #{rounds} rounds written to #{out}"
