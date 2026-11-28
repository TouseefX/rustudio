--!strict
--[[
    Reactive CollectionService + Selection store for RusTudio's Vide Tag Editor.
    All DOM tag mutations are wrapped in ChangeHistoryService recordings so
    Ctrl+Z / Ctrl+Y undo and redo cleanly.
]]

local CollectionService = game:GetService("CollectionService")
local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)

local PALETTE: { Color3 } = {
	Color3.fromRGB(0, 162, 255), -- Studio Blue
	Color3.fromRGB(62, 207, 142), -- Emerald
	Color3.fromRGB(245, 166, 35), -- Amber
	Color3.fromRGB(235, 87, 87), -- Coral Red
	Color3.fromRGB(168, 118, 255), -- Violet
	Color3.fromRGB(45, 212, 191), -- Teal
	Color3.fromRGB(244, 114, 182), -- Rose
	Color3.fromRGB(250, 204, 21), -- Gold
	Color3.fromRGB(96, 165, 250), -- Sky
	Color3.fromRGB(156, 163, 175), -- Slate
}

local SETTINGS_TAGS_KEY = "rustudio_TagEditor_savedTags"
local SETTINGS_COLORS_KEY = "rustudio_TagEditor_tagColors"

export type CheckState = "none" | "some" | "all"

export type TagRowInfo = {
	name: string,
	count: number,
	check: CheckState,
	color: Color3,
}

local function hashColorIndex(tag: string): number
	local h = 5381
	for i = 1, #tag do
		h = (h * 33 + string.byte(tag, i)) % 2147483647
	end
	return (h % #PALETTE) + 1
end

local function trim(s: string): string
	return (string.gsub(s, "^%s*(.-)%s*$", "%1"))
end

local function withRecording(name: string, display: string, fn: () -> ())
	local rec = nil
	pcall(function()
		rec = ChangeHistoryService:TryBeginRecording(name, display)
	end)
	local ok, err = pcall(fn)
	if rec then
		pcall(function()
			ChangeHistoryService:FinishRecording(
				rec,
				if ok then Enum.FinishRecordingOperation.Commit else Enum.FinishRecordingOperation.Cancel
			)
		end)
	elseif ok then
		pcall(function()
			ChangeHistoryService:SetWaypoint(display)
		end)
	end
	if not ok then
		warn("[TagEditor] " .. tostring(err))
	end
end

local TagStore = {}

function TagStore.new(pluginObj: Plugin)
	local searchQuery = Vide.source("")
	local selectionCount = Vide.source(0)
	local tagNames = Vide.source({} :: { string })
	local rev = Vide.source(0)

	-- Saved user-created tags (persisted) + session tags (tags seen or unchecked
	-- in this place so unchecking the last tagged object keeps the row visible
	-- at count 0 until explicitly deleted with ×).
	local savedTags: { [string]: boolean } = {}
	local sessionTags: { [string]: boolean } = {}
	local colorOverrides: { [string]: number } = {}

	do
		local ok, rawTags = pcall(function()
			return pluginObj:GetSetting(SETTINGS_TAGS_KEY)
		end)
		if ok and type(rawTags) == "table" then
			for _, t in ipairs(rawTags) do
				if type(t) == "string" and #t > 0 then
					savedTags[t] = true
				end
			end
		end
		local ok2, rawColors = pcall(function()
			return pluginObj:GetSetting(SETTINGS_COLORS_KEY)
		end)
		if ok2 and type(rawColors) == "table" then
			for k, v in pairs(rawColors) do
				if type(k) == "string" and type(v) == "number" then
					local idx = math.floor(v)
					if idx >= 1 and idx <= #PALETTE then
						colorOverrides[k] = idx
					end
				end
			end
		end
	end

	local function persistSavedTags()
		local list = {}
		for t in pairs(savedTags) do
			table.insert(list, t)
		end
		table.sort(list)
		pcall(function()
			pluginObj:SetSetting(SETTINGS_TAGS_KEY, list)
		end)
	end

	local function persistColors()
		pcall(function()
			pluginObj:SetSetting(SETTINGS_COLORS_KEY, colorOverrides)
		end)
	end

	local tagSignals: { [string]: { RBXScriptConnection } } = {}
	local refresh: () -> ()

	local function syncTagWatchers(allNames: { string })
		local present: { [string]: boolean } = {}
		for _, name in ipairs(allNames) do
			present[name] = true
			if not tagSignals[name] then
				local c1 = CollectionService:GetInstanceAddedSignal(name):Connect(function()
					refresh()
				end)
				local c2 = CollectionService:GetInstanceRemovedSignal(name):Connect(function()
					refresh()
				end)
				tagSignals[name] = { c1, c2 }
			end
		end
		for name, conns in pairs(tagSignals) do
			if not present[name] then
				for _, c in ipairs(conns) do
					c:Disconnect()
				end
				tagSignals[name] = nil
			end
		end
	end

	local function computeCheckState(tag: string, sel: { Instance }): CheckState
		if #sel == 0 then
			return "none"
		end
		local tagged = 0
		for _, inst in ipairs(sel) do
			if CollectionService:HasTag(inst, tag) then
				tagged += 1
			end
		end
		if tagged == 0 then
			return "none"
		elseif tagged == #sel then
			return "all"
		else
			return "some"
		end
	end

	refresh = function()
		local sel = Selection:Get()
		local merged: { [string]: boolean } = {}
		for t in pairs(savedTags) do
			merged[t] = true
		end
		for t in pairs(sessionTags) do
			merged[t] = true
		end
		local ok, all = pcall(function()
			return CollectionService:GetAllTags()
		end)
		if ok and type(all) == "table" then
			for _, t in ipairs(all) do
				if type(t) == "string" and #t > 0 then
					merged[t] = true
					sessionTags[t] = true
				end
			end
		end

		local names = {}
		for t in pairs(merged) do
			table.insert(names, t)
		end
		table.sort(names, function(a, b)
			local la, lb = string.lower(a), string.lower(b)
			if la == lb then
				return a < b
			end
			return la < lb
		end)

		syncTagWatchers(names)

		Vide.batch(function()
			selectionCount(#sel)
			tagNames(names)
			rev(rev() + 1)
		end)
	end

	local filteredTags = Vide.derive(function(): { string }
		local all = tagNames()
		local q = string.lower(trim(searchQuery()))
		if q == "" then
			return all
		end
		local out = {}
		for _, t in ipairs(all) do
			if string.find(string.lower(t), q, 1, true) ~= nil then
				table.insert(out, t)
			end
		end
		return out
	end)

	local function getTagInfo(tag: string): TagRowInfo
		-- Read `rev()` so any caller inside a Vide effect reacts to tag/selection changes.
		local _ = rev()
		local sel = Selection:Get()
		local tagged = CollectionService:GetTagged(tag)
		local colorIdx = colorOverrides[tag] or hashColorIndex(tag)
		return {
			name = tag,
			count = #tagged,
			check = computeCheckState(tag, sel),
			color = PALETTE[colorIdx] or PALETTE[1],
		}
	end

	local function toggleTagOnSelection(tag: string)
		local sel = Selection:Get()
		if #sel == 0 then
			return
		end
		sessionTags[tag] = true
		local state = computeCheckState(tag, sel)
		local adding = state ~= "all"
		withRecording(
			"TagEditorToggle",
			if adding then `Add tag "{tag}"` else `Remove tag "{tag}"`,
			function()
				for _, inst in ipairs(sel) do
					if adding then
						CollectionService:AddTag(inst, tag)
					else
						CollectionService:RemoveTag(inst, tag)
					end
				end
			end
		)
		refresh()
	end

	local function createTag(rawName: string): boolean
		local name = trim(rawName)
		if name == "" then
			return false
		end
		savedTags[name] = true
		persistSavedTags()

		local sel = Selection:Get()
		if #sel > 0 then
			withRecording("TagEditorCreate", `Add tag "{name}"`, function()
				for _, inst in ipairs(sel) do
					CollectionService:AddTag(inst, name)
				end
			end)
		end
		refresh()
		return true
	end

	local function deleteTag(tag: string)
		savedTags[tag] = nil
		sessionTags[tag] = nil
		colorOverrides[tag] = nil
		persistSavedTags()
		persistColors()

		local tagged = CollectionService:GetTagged(tag)
		if #tagged > 0 then
			withRecording("TagEditorDelete", `Delete tag "{tag}"`, function()
				for _, inst in ipairs(tagged) do
					CollectionService:RemoveTag(inst, tag)
				end
			end)
		end
		refresh()
	end

	local function selectTagged(tag: string)
		local tagged = CollectionService:GetTagged(tag)
		Selection:Set(tagged)
		refresh()
	end

	local function cycleColor(tag: string)
		local cur = colorOverrides[tag] or hashColorIndex(tag)
		colorOverrides[tag] = (cur % #PALETTE) + 1
		persistColors()
		rev(rev() + 1)
	end

	-- Attach live DataModel subscriptions inside the current Vide scope.
	local function subscribe()
		Vide.cleanup(Selection.SelectionChanged:Connect(refresh))
		pcall(function()
			Vide.cleanup(CollectionService.TagAdded:Connect(refresh))
		end)
		pcall(function()
			Vide.cleanup(CollectionService.TagRemoved:Connect(refresh))
		end)
		pcall(function()
			Vide.cleanup(ChangeHistoryService.OnUndo:Connect(refresh))
			Vide.cleanup(ChangeHistoryService.OnRedo:Connect(refresh))
		end)
		Vide.cleanup(function()
			for _, conns in pairs(tagSignals) do
				for _, c in ipairs(conns) do
					c:Disconnect()
				end
			end
			table.clear(tagSignals)
		end)
		refresh()
	end

	return {
		searchQuery = searchQuery,
		selectionCount = selectionCount,
		tagNames = tagNames,
		filteredTags = filteredTags,
		getTagInfo = getTagInfo,
		toggleTagOnSelection = toggleTagOnSelection,
		createTag = createTag,
		deleteTag = deleteTag,
		selectTagged = selectTagged,
		cycleColor = cycleColor,
		refresh = refresh,
		subscribe = subscribe,
	}
end

return TagStore
