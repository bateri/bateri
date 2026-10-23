-- The page API, part 4: JSON, the text the page and its program write to each other in.
--
-- `json.encode(value)` writes a value: a table with the keys 1..n is a list, any other is an
-- object (keys in order, so that the same value is always the same text); an empty table is an
-- object, unless it is `setmetatable({}, json.array)`. `json.decode(text)` reads one and gives
-- the value, or nil and why: objects and lists are tables, `null` is `json.null` in a list and
-- an absent key in an object. The text came from a program the person may not know: it is read
-- with a limit on how deep it goes, and nothing in it is run.

local json = {}
bateri.json = json

json.null = setmetatable({}, { __tostring = function() return "null" end })
json.array = {}

local MAX_DEPTH = 64

local escapes = {
	['"'] = '\\"',
	["\\"] = "\\\\",
	["\b"] = "\\b",
	["\f"] = "\\f",
	["\n"] = "\\n",
	["\r"] = "\\r",
	["\t"] = "\\t",
}

local function escape(character)
	return escapes[character] or string.format("\\u%04x", string.byte(character))
end

local function is_list(value)
	if getmetatable(value) == json.array then
		return true
	end
	local count = #value
	if count == 0 then
		return false
	end
	for key in pairs(value) do
		if type(key) ~= "number" or key < 1 or key > count or key ~= math.floor(key) then
			return false
		end
	end
	return true
end

local function write(value, out, depth)
	local kind = type(value)
	if kind == "string" then
		out[#out + 1] = '"' .. string.gsub(value, '[%c"\\]', escape) .. '"'
	elseif kind == "number" then
		if value ~= value or value == math.huge or value == -math.huge then
			out[#out + 1] = "null"
		elseif value == math.floor(value) and math.abs(value) < 2 ^ 53 then
			out[#out + 1] = string.format("%d", value)
		else
			out[#out + 1] = string.format("%.17g", value)
		end
	elseif kind == "boolean" then
		out[#out + 1] = value and "true" or "false"
	elseif value == nil or value == json.null then
		out[#out + 1] = "null"
	elseif kind == "table" then
		if depth > MAX_DEPTH then
			error("json.encode: the value is nested too deep", 0)
		end
		if is_list(value) then
			out[#out + 1] = "["
			for index = 1, #value do
				if index > 1 then
					out[#out + 1] = ","
				end
				write(value[index], out, depth + 1)
			end
			out[#out + 1] = "]"
		else
			local keys = {}
			for key in pairs(value) do
				keys[#keys + 1] = tostring(key)
			end
			table.sort(keys)
			out[#out + 1] = "{"
			local first = true
			for _, key in ipairs(keys) do
				local member = value[key]
				if member == nil then
					member = value[tonumber(key)]
				end
				if member ~= nil then
					if not first then
						out[#out + 1] = ","
					end
					first = false
					out[#out + 1] = '"' .. string.gsub(key, '[%c"\\]', escape) .. '":'
					write(member, out, depth + 1)
				end
			end
			out[#out + 1] = "}"
		end
	else
		error("json.encode: a " .. kind .. " cannot be written", 0)
	end
end

function json.encode(value)
	local out = {}
	write(value, out, 1)
	return table.concat(out)
end

-- --- Reading -------------------------------------------------------------------------------------

local function fail(text, at, why)
	error({ json = true, message = string.format("%s at %d", why, at) }, 0)
end

local function skip(text, at)
	return string.find(text, "[^ \t\r\n]", at) or #text + 1
end

local read

local function read_string(text, at)
	local out = {}
	local from = at + 1
	while true do
		local stop = string.find(text, '["\\%c]', from)
		if not stop then
			fail(text, at, "a string that does not end")
		end
		out[#out + 1] = string.sub(text, from, stop - 1)
		local character = string.sub(text, stop, stop)
		if character == '"' then
			return table.concat(out), stop + 1
		elseif character ~= "\\" then
			fail(text, stop, "a control character in a string")
		end
		local escaped = string.sub(text, stop + 1, stop + 1)
		local plain = { ['"'] = '"', ["\\"] = "\\", ["/"] = "/", b = "\b", f = "\f", n = "\n", r = "\r", t = "\t" }
		if plain[escaped] then
			out[#out + 1] = plain[escaped]
			from = stop + 2
		elseif escaped == "u" then
			local digits = string.match(text, "^%x%x%x%x", stop + 2)
			if not digits then
				fail(text, stop, "a bad \\u escape")
			end
			local code = tonumber(digits, 16)
			from = stop + 6
			if code >= 0xD800 and code < 0xDC00 then
				local low = string.match(text, "^\\u(%x%x%x%x)", from)
				local lower = low and tonumber(low, 16)
				if lower and lower >= 0xDC00 and lower < 0xE000 then
					code = 0x10000 + (code - 0xD800) * 0x400 + (lower - 0xDC00)
					from += 6
				else
					code = 0xFFFD
				end
			elseif code >= 0xDC00 and code < 0xE000 then
				code = 0xFFFD
			end
			out[#out + 1] = utf8.char(code)
		else
			fail(text, stop, "a bad escape")
		end
	end
end

function read(text, at, depth)
	if depth > MAX_DEPTH then
		fail(text, at, "nested too deep")
	end
	at = skip(text, at)
	local character = string.sub(text, at, at)
	if character == "{" then
		local object = {}
		at = skip(text, at + 1)
		if string.sub(text, at, at) == "}" then
			return object, at + 1
		end
		while true do
			if string.sub(text, at, at) ~= '"' then
				fail(text, at, "a key was expected")
			end
			local key
			key, at = read_string(text, at)
			at = skip(text, at)
			if string.sub(text, at, at) ~= ":" then
				fail(text, at, "a colon was expected")
			end
			local value
			value, at = read(text, at + 1, depth + 1)
			if value ~= json.null then
				object[key] = value
			end
			at = skip(text, at)
			local after = string.sub(text, at, at)
			if after == "}" then
				return object, at + 1
			elseif after ~= "," then
				fail(text, at, "a comma or a brace was expected")
			end
			at = skip(text, at + 1)
		end
	elseif character == "[" then
		local list = {}
		at = skip(text, at + 1)
		if string.sub(text, at, at) == "]" then
			return list, at + 1
		end
		while true do
			local value
			value, at = read(text, at, depth + 1)
			list[#list + 1] = value
			at = skip(text, at)
			local after = string.sub(text, at, at)
			if after == "]" then
				return list, at + 1
			elseif after ~= "," then
				fail(text, at, "a comma or a bracket was expected")
			end
			at += 1
		end
	elseif character == '"' then
		return read_string(text, at)
	elseif string.sub(text, at, at + 3) == "true" then
		return true, at + 4
	elseif string.sub(text, at, at + 4) == "false" then
		return false, at + 5
	elseif string.sub(text, at, at + 3) == "null" then
		return json.null, at + 4
	end
	local number = string.match(text, "^-?%d+%.?%d*[eE]?[+-]?%d*", at)
	if not number or number == "" then
		fail(text, at, "a value was expected")
	end
	local value = tonumber(number)
	if not value then
		fail(text, at, "a bad number")
	end
	return value, at + #number
end

function json.decode(text)
	local ok, value, at = pcall(read, text, 1, 1)
	if not ok then
		if type(value) == "table" and value.json then
			return nil, value.message
		end
		error(value, 0)
	end
	if skip(text, at) <= #text then
		return nil, "more after the value at " .. at
	end
	if value == json.null then
		return nil
	end
	return value
end
