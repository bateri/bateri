-- The page API, part 3: what a page remembers between frames.
--
-- A page is built afresh for every frame, so what the pointer is over, where the keyboard is,
-- how far a list is scrolled and how far a highlight has faded are not in the elements: they
-- are kept by name (the element's id) in these, and the page reads them when it builds.

local layout = bateri.layout
local f32 = bateri.f32

-- --- Highlights that come and go ---------------------------------------------------------------

-- A highlight takes this long to come to full strength or to go, seconds. The levels are single
-- precision, as the engine's own are: a highlight that comes up and goes in equal steps comes
-- to exactly 0 and is forgotten, where doubles would leave a level of 1e-17 that is drawn.
local FADE_SECONDS = f32(0.12)
-- The longest step one frame may take: a frame after a long sleep moves the highlights a little,
-- not to their end, so the change is seen.
local STEP_MAX = 0.05

local Fades = {}
Fades.__index = Fades
bateri.Fades = Fades
Fades.SECONDS = FADE_SECONDS

function Fades.new()
	return setmetatable({ levels = {}, last = nil }, Fades)
end

-- Sets where `id`'s level goes (0 to 1). A level not known yet starts at 0 and comes up.
function Fades:to(id, target)
	target = math.clamp(target, 0, 1)
	local fade = self.levels[id]
	if fade then
		fade.target = target
	elseif target > 0 then
		self.levels[id] = { level = 0, target = target }
	end
end

-- Sets where `id`'s level goes and puts it there at once.
function Fades:snap(id, level)
	level = math.clamp(level, 0, 1)
	if level > 0 then
		self.levels[id] = { level = level, target = level }
	else
		self.levels[id] = nil
	end
end

function Fades:level(id)
	local fade = self.levels[id]
	return fade and fade.level or 0
end

-- Whether anything is on the way to its target.
function Fades:moving()
	for _, fade in pairs(self.levels) do
		if fade.level ~= fade.target then
			return true
		end
	end
	return false
end

-- Takes the step to `now` (seconds on the application's clock). Reduce Motion puts every level
-- at its target. True while anything is still moving afterwards: the page asks for another frame.
function Fades:advance(now, reduce_motion)
	local levels = self.levels
	if not self:moving() then
		self.last = nil
		for id, fade in pairs(levels) do
			if not (fade.level > 0) then
				levels[id] = nil
			end
		end
		return false
	end
	local dt = 0
	if self.last then
		dt = math.clamp(now - self.last, 0, STEP_MAX)
	end
	local step = reduce_motion and 1 or f32(f32(dt) / FADE_SECONDS)
	for _, fade in pairs(levels) do
		if fade.level < fade.target then
			fade.level = math.min(f32(fade.level + step), fade.target)
		elseif fade.level > fade.target then
			fade.level = math.max(f32(fade.level - step), fade.target)
		end
	end
	local moving = self:moving()
	-- A level that came to rest at 0 is not kept.
	for id, fade in pairs(levels) do
		if not (fade.level > 0 or fade.target > 0) then
			levels[id] = nil
		end
	end
	self.last = moving and now or nil
	if reduce_motion then
		return false
	end
	return moving
end

-- --- A long list's arithmetic ------------------------------------------------------------------

-- Where each row of a list stands, which rows a window of it shows and how far to scroll to
-- see one. Rows are counted from 1. A page that shows a long list builds only the rows in view
-- and keeps the rest of the list's length with a spacer above and one below.
local Rows = {}
Rows.__index = Rows
bateri.Rows = Rows

-- A list whose rows are `heights` high (a list of numbers).
function Rows.new(heights)
	local tops = { 0 }
	local sum = 0
	for index, height in ipairs(heights) do
		sum += math.max(height, 0)
		tops[index + 1] = sum
	end
	return setmetatable({ tops = tops, trail = 0 }, Rows)
end

-- The same list with `lead` points of room before the first row and `trail` after the last.
function Rows:padded(lead, trail)
	for index, top in ipairs(self.tops) do
		self.tops[index] = top + lead
	end
	self.trail = trail
	return self
end

function Rows:len()
	return #self.tops - 1
end

function Rows:is_empty()
	return #self.tops == 1
end

-- The length of the whole list, with the room around the rows.
function Rows:total()
	return self.tops[#self.tops] + self.trail
end

-- Where row `index` starts (the list's length for a row past the end).
function Rows:top(index)
	return self.tops[math.min(index, #self.tops)]
end

function Rows:height(index)
	return self:top(index + 1) - self:top(index)
end

-- How many of the tops are not past `y` (`inclusive`) or are before it.
local function count_before(tops, y, inclusive)
	local low, high = 1, #tops + 1
	while low < high do
		local middle = (low + high) // 2
		local top = tops[middle]
		if top < y or (inclusive and top == y) then
			low = middle + 1
		else
			high = middle
		end
	end
	return low - 1
end

-- The row at `y` from the list's start (the last row for a point past the end).
function Rows:at(y)
	if self:is_empty() then
		return nil
	end
	local after = count_before(self.tops, y, true)
	return math.min(math.max(after, 1), self:len())
end

-- The rows that touch the window `offset .. offset + view`, `overscan` points more at each end:
-- the first and the last (the last is before the first when there are none).
function Rows:window(offset, view, overscan)
	if self:is_empty() or view <= 0 then
		return 1, 0
	end
	local from = math.max(offset - overscan, 0)
	local to = offset + view + overscan
	if from >= self:total() then
		return self:len() + 1, self:len()
	end
	local first = self:at(from)
	local last = math.min(count_before(self.tops, to, false), self:len())
	return first, math.max(last, first - 1)
end

-- The offset that shows all of row `index` in a window of `view` points that is at `offset`
-- now: `offset` itself when the row is in view, the nearest edge otherwise. The first row is
-- shown from the start of the list and the last to its end, room around them and all.
function Rows:reveal(index, offset, view)
	local top = index == 1 and 0 or self:top(index)
	local bottom = index >= self:len() and self:total() or self:top(index + 1)
	if top < offset then
		return top
	elseif bottom > offset + view then
		return math.min(math.max(bottom - view, 0), top)
	end
	return offset
end

-- --- What the pointer and the keyboard are on ----------------------------------------------------

-- The remembered state of a page: what the pointer is over and has pressed, where the keyboard
-- is, how far each list is scrolled. Every method that takes the pointer or the wheel answers
-- whether anything it keeps *changed* (a pointer moving inside the row it was already over
-- changes nothing, and no frame is drawn for it). `laid` is `bateri.layout`.
local Ui = {}
Ui.__index = Ui
bateri.Ui = Ui

function Ui.new()
	return setmetatable({
		-- The pressable element the pointer is over, the one a button is held on, and the
		-- element the keyboard is on.
		hover = nil,
		pressed = nil,
		focus = nil,
		-- How far each scrolling box is scrolled, points.
		scroll = {},
		-- The highlights coming and going.
		fades = Fades.new(),
	}, Ui)
end

-- The pointer is at `at` (nil: it left the page). Whether the element it is over changed.
function Ui:point(laid, at)
	local over = nil
	if at then
		over = laid.hit(at)
	end
	local changed = self.hover ~= over
	self.hover = over
	return changed
end

-- A button went down at `at`: the element under it is the pressed one, and the keyboard moves
-- to the nearest element around it that takes it. The element pressed.
function Ui:press(laid, at)
	local hit = laid.hit(at)
	self.pressed = hit
	local takes = {}
	for _, id in ipairs(laid.focus_order()) do
		takes[id] = true
	end
	for _, id in ipairs(laid.ids_at(at)) do
		if takes[id] then
			self.focus = id
			break
		end
	end
	return hit
end

-- The button came up at `at`. A click is a release over the element the press was on: that element.
function Ui:release(laid, at)
	local pressed = self.pressed
	self.pressed = nil
	if pressed ~= nil and laid.hit(at) == pressed then
		return pressed
	end
	return nil
end

function Ui:scroll_offset(id)
	return self.scroll[id] or 0
end

-- Scrolls the box named `id` to `offset`, kept within its reach on the page `laid`. Whether it moved.
function Ui:scroll_to(laid, id, offset)
	local extent = laid.scroll_extent(id)
	local reach = extent and extent.max_offset or math.huge
	offset = math.clamp(offset, 0, reach)
	local was = self.scroll[id] or 0
	self.scroll[id] = offset
	return was ~= offset
end

-- Sets how far the box named `id` is scrolled, without knowing yet how far it can go: the next
-- `settle` brings it within its reach.
function Ui:set_scroll(id, offset)
	self.scroll[id] = math.max(offset, 0)
end

-- The wheel turned over `at` by `dy` points (positive: down the page). The innermost scrolling
-- box under the pointer that has room to go that way moves; one already at its end passes the
-- turn on to the box around it. Whether anything moved.
function Ui:scroll_by(laid, at, dy)
	for _, id in ipairs(laid.ids_at(at)) do
		local extent = laid.scroll_extent(id)
		if extent then
			local was = self:scroll_offset(id)
			local to = math.clamp(was + dy, 0, extent.max_offset)
			if to ~= was then
				self.scroll[id] = to
				return true
			end
		end
	end
	return false
end

-- Brings every scrolled box back within its reach on the page `laid`. Whether any moved: the
-- page is laid out again then, as its rows depend on the offset.
function Ui:settle(laid)
	local moved = false
	for id, offset in pairs(self.scroll) do
		local extent = laid.scroll_extent(id)
		if not extent then
			if not (offset > 0) then
				self.scroll[id] = nil
			end
		else
			local to = math.clamp(offset, 0, extent.max_offset)
			moved = moved or to ~= offset
			if to > 0 then
				self.scroll[id] = to
			else
				self.scroll[id] = nil
			end
		end
	end
	return moved
end

-- Puts the keyboard on `id`. Whether it moved.
function Ui:focus_on(id)
	local changed = self.focus ~= id
	self.focus = id
	return changed
end

-- Moves the keyboard to the next element that takes it (the previous one when `back`), around
-- from the last to the first. The element the keyboard is on now.
function Ui:focus_step(laid, back)
	local order = laid.focus_order()
	local count = #order
	if count == 0 then
		self.focus = nil
		return nil
	end
	local at = nil
	for index, id in ipairs(order) do
		if id == self.focus then
			at = index
			break
		end
	end
	local nextAt
	if at == nil then
		nextAt = back and count or 1
	elseif back then
		nextAt = (at - 2) % count + 1
	else
		nextAt = at % count + 1
	end
	self.focus = order[nextAt]
	return self.focus
end

-- Where a key goes: the element the keyboard is on, then the named elements around it, the
-- innermost first.
function Ui:key_path(laid)
	if self.focus == nil then
		return {}
	end
	return laid.path_of(self.focus)
end
