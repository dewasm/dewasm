require_relative "boot"

require "rails"
require "active_model/railtie"
require "active_job/railtie"
require "active_record/railtie"
require "active_storage/engine"
require "action_controller/railtie"
require "action_mailer/railtie"
require "action_text/engine"
require "action_view/railtie"
require "action_cable/engine"
require "rails/test_unit/railtie"

Bundler.require(*Rails.groups)

module App
  class Application < Rails::Application
    config.load_defaults 8.1

    # `lib/` is in the Rack stack, so it is not reloaded.
    config.autoload_lib_once(ignore: %w[assets tasks fallbacks])

    config.generators.system_tests = nil
  end
end
