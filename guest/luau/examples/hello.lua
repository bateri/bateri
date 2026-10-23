-- The smallest page that does something: a button that counts its clicks.
--
-- A script returns the page: a table of the functions the application calls. `render` makes the
-- page for the size it is given; `handle` is told what the person did. What the page remembers
-- is its own, in the script's locals; `bateri.Ui` keeps what the pointer is over. The whole API
-- is described at the top of the files in `guest/luau/prelude`.

local b = bateri
local ui = b.Ui.new()
local count = 0

-- Elements are named by a string, so that the page can ask what the pointer is over.
local BUTTON = "counter button"

return {
	render = function(size, now, reduce_motion)
		local hovered = ui.hover == BUTTON
		return b.div():size_full():items_center():justify_center():gap(12)
			:child(b.text("Clicked " .. count .. " times"):text_size(18))
			:child(b.div():press(BUTTON):px(14):py(8):rounded(8)
				:bg(hovered and "raised3" or "raised2")
				:child(b.text("Click me")))
	end,

	handle = function(event)
		if event.type == "move" then
			return ui:point(b.layout, event)
		elseif event.type == "leave" then
			return ui:point(b.layout, nil)
		elseif event.type == "press" then
			ui:press(b.layout, event)
		elseif event.type == "release" then
			if ui:release(b.layout, event) == BUTTON then
				count += 1
				return true
			end
		end
	end,
}
