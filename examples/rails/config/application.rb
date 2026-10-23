require_relative "boot"

require "rails"
# Pick the frameworks you want:
require "active_model/railtie"
require "active_job/railtie"
require "active_record/railtie"
require "active_storage/engine"
require "action_controller/railtie"
require "action_mailer/railtie"
# require "action_mailbox/engine"
require "action_text/engine"
require "action_view/railtie"
require "action_cable/engine"
require "rails/test_unit/railtie"

# Require the gems listed in Gemfile, including any gems
# you've limited to :test, :development, or :production.
Bundler.require(*Rails.groups)

module App
  class Application < Rails::Application
    # Initialize configuration defaults for originally generated Rails version.
    config.load_defaults 8.1

    # `lib/` holds the request profiler, which an initializer puts in the Rack stack, so it is
    # loaded once rather than reloaded; `lib/fallbacks/` is for `require` only (`config/boot.rb`).
    config.autoload_lib_once(ignore: %w[assets tasks fallbacks])

    config.generators.system_tests = nil
  end
end
