-- The page inspector's own page: it shows the page of another pane, element by element. It is
-- bateri's, not a program's: it asks what it shows of the responder that stands where a program
-- would (`native_view::inspector`), with the kit's own components and its query module, so that
-- the kit is tried by the tool that is meant to look at it.
--
-- What the responder says: the topic `target` pushes `{ pane = , page = , gone = }` (the name of
-- the pane looked at, whether it shows a page, whether it is gone), and the call `close` asks
-- for this pane to be closed.

local ui = require("@pencere/ui")
local query = require("@pencere/query")

local u = ui.new()
local q = query.new()

pencere.subscribe("target", nil, function(value)
	q:set("target", nil, value)
	return true
end)

-- What the program said last of the pane, from what the context knows of the key (nil before the
-- first push).
local function target_of(read)
	return read and read.data
end

-- What the page says of what it looks at, before it has anything to show of it.
local function words_of(target)
	if target == nil then
		return "Connecting to the pane…"
	elseif target.gone then
		return "The pane is gone."
	elseif target.page then
		return "Reading the page of " .. target.pane .. "."
	end
	return "No page in " .. target.pane .. "."
end

return {
	render = function(size, now, reduce_motion)
		u:begin(size, now, reduce_motion)
		local root = pencere.div():size_full():bg("background"):color("foreground"):p(12):gap(6)
			:child(pencere.text("Page inspector"):text_size(15):weight(600))
			:child(pencere.text(words_of(target_of(q:peek("target")))):color("dim"):text_size(12.5))
		return u:finish(root)
	end,

	settle = function()
		return u:settle()
	end,

	handle = function(event)
		if event.type == "key" and event.key == "escape" then
			pencere.call("close", nil, nil)
			return true
		end
		return (u:handle(event))
	end,
}
