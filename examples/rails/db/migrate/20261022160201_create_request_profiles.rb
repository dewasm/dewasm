class CreateRequestProfiles < ActiveRecord::Migration[8.1]
  def change
    create_table :request_profiles do |t|
      t.string :http_method
      t.string :path
      t.integer :status
      t.float :duration_ms
      t.integer :sample_count

      t.timestamps
    end
  end
end
