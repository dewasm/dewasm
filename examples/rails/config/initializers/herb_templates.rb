# frozen_string_literal: true

# Compile HTML+ERB templates through Herb instead of Erubi.
# Rails 8.2 ships this class as `ActionView::Template::Handlers::ERB::Herb`.
# It is selected there with `config.action_view.erb_implementation = :herb`.
# This file is that class, taken from the Rails main branch (MIT license), for Rails 8.1.
require "herb"
require "herb/engine"

module ActionView
  class Template
    module Handlers
      class ERB
        class Herb < ::Herb::Engine
          def initialize(input, properties = {})
            @newline_pending = 0

            properties = Hash[properties]

            properties[:bufvar]     ||= "@output_buffer"
            properties[:preamble]   ||= ""
            properties[:postamble]  ||= properties[:bufvar].to_s

            properties[:freeze_template_literals] = !Template.frozen_string_literal

            # Added for this example: the converted parser is slower than Herb's 1 second limit.
            properties[:parser_options] = { timeout: 120 }.merge(properties[:parser_options] || {})

            # Escaping is left to `ActionView::OutputBuffer`.
            properties[:escapefunc] = ""
            properties[:attrfunc]   = nil
            properties[:jsfunc]     = nil
            properties[:cssfunc]    = nil

            super
          end

          private
            def add_text(text)
              return if text.empty?

              if text == "\n"
                @newline_pending += 1
              else
                with_buffer do
                  src << ".safe_append='"
                  src << "\n" * @newline_pending if @newline_pending > 0
                  src << text.gsub(/['\\]/, '\\\\\&') << @text_end
                end
                @newline_pending = 0
              end
            end

            def add_expression_block(indicator, code)
              add_expression(indicator, code, block: true)
            end

            def add_expression(indicator, code, block: false)
              flush_newline_if_pending(src)

              with_buffer do
                if (indicator == "==") || @escape
                  src << ".safe_expr_append="
                else
                  src << ".append="
                end

                if block
                  src << " " << code
                else
                  src << "(" << code << trailing_newline(code) << ")"
                end
              end
            end

            def add_code(code)
              flush_newline_if_pending(src)
              super
            end

            def add_postamble(_)
              flush_newline_if_pending(src)
              super
            end

            def flush_newline_if_pending(src)
              if @newline_pending > 0
                with_buffer { src << ".safe_append='#{"\n" * @newline_pending}" << @text_end }
                @newline_pending = 0
              end
            end
        end
      end
    end
  end
end

ActionView::Template::Handlers::ERB.erb_implementation = ActionView::Template::Handlers::ERB::Herb
