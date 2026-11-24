-- The page inspector's own page: it shows the page of another pane, element by element. It is
-- bateri's, not a program's: it asks what it shows of the responder that stands where a program
-- would (`native_view::inspector`), with the kit's own components and its query module, so that
-- the kit is tried by the tool that is meant to look at it.
--
-- What the responder says: the topic `target` pushes `{ pane = , page = , gone = , stamp = }` (the
-- name of the pane looked at, whether it shows a page, whether it is gone, and how many pages its
-- script has made), the topic `costs` what its last frames cost, and the calls `children`, `node`
-- and `select` read and choose elements by their place in the tree (`{ path = { 1, 2 } }`); `close`
-- asks for this pane to be closed.
--
-- **What is asked of the target is asked under its stamp**: the context asks again what it asked
-- under another one, so the page reads the target's new page and never the old one's.

local ui = require("@pencere/ui")
local query = require("@pencere/query")

local u = ui.new()
local q = query.new()

pencere.subscribe("target", nil, function(value)
	q:set("target", nil, value)
	return true
end)

pencere.subscribe("costs", nil, function(value)
	q:set("costs", nil, value)
	return true
end)

-- What the program said last, from what the context knows of the key (nil before the first push).
local function said_of(read)
	return read and read.data
end

-- What the page says of what it looks at, before it has anything to show of it.
local function words_of(target)
	if target == nil then
		return "Connecting to the pane…"
	elseif target.gone then
		return "The pane is gone."
	elseif not target.page then
		return "No page in " .. target.pane .. "."
	end
	return nil
end

-- The line the last frame's cost makes.
local function cost_line(costs)
	local frames = costs and costs.frames
	local last = frames and frames[#frames]
	if last == nil then
		return "No frame drawn yet."
	end
	return string.format(
		"%d elements · script %.2f ms · layout %.2f ms · paint %.2f ms",
		costs.elements,
		last.script_ms,
		last.layout_ms,
		last.paint_ms
	)
end

return {
	render = function(size, now, reduce_motion)
		u:begin(size, now, reduce_motion)
		local target = said_of(q:peek("target"))
		local root = pencere.div():size_full():bg("background"):color("foreground"):p(12):gap(6)
			:child(pencere.text("Page inspector"):text_size(15):weight(600))
		local words = words_of(target)
		if words then
			root:child(pencere.text(words):color("dim"):text_size(12.5))
		else
			local read = q:get("children", { path = {} }, { stamp = target.stamp })
			local children = read.data and read.data.children or {}
			root:child(pencere.text("Reading the page of " .. target.pane .. "."):color("dim"):text_size(12.5))
			root:child(pencere.text(#children .. " elements at the top"):color("dim"):text_size(12.5))
			root:child(pencere.text(cost_line(said_of(q:peek("costs")))):color("dim"):text_size(12.5))
		end
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
