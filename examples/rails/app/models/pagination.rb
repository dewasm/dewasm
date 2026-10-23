# One page of a list: the page number from the query, the page size, and the list's total.
# `slice` applies it to a relation; the `shared/pagination` partial draws the links.
class Pagination
  attr_reader :number, :per, :total

  def initialize(param, per:, total:)
    @per = per
    @total = total
    @number = [ [ param.to_i, 1 ].max, pages ].min
  end

  def pages = [ (total.to_f / per).ceil, 1 ].max
  def offset = (number - 1) * per
  def first? = number == 1
  def last? = number == pages
  def slice(relation) = relation.offset(offset).limit(per)

  # The 1-based positions of the first and last item on this page: `11` and `20` of `26`.
  def from = total.zero? ? 0 : offset + 1
  def to = [ offset + per, total ].min
end
