class StatsController < ApplicationController
  allow_unauthenticated_access

  def show
    render json: {
      ruby: RUBY_DESCRIPTION,
      rails: Rails.version,
      erb_handler: ActionView::Template.registered_template_handler(:erb).then { |h| h.is_a?(Class) ? h.name : h.class.name },
      erb_implementation: ReActionView::Template::Handlers::Herb.erb_implementation.name,
      sqlite_version: ActiveRecord::Base.lease_connection.select_value("SELECT sqlite_version()"),
      sqlite3_gem: SQLite3::VERSION,
      herb: Herb::VERSION,
      nokogiri: Nokogiri::VERSION,
      bcrypt: BCrypt::Engine.cost,
      pygments: "#{Pygments.pygments_version} (pygments.rb #{Gem.loaded_specs["pygments.rb"].version})",
      posts: Post.count,
      native_extensions: NativeExtensions.report
    }
  end
end
