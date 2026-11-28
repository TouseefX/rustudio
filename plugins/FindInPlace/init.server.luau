--!strict
--[[
    RusTudio Built-in Suite: Find All / Replace All (Luau / Vide)

    1:1 recreation of Roblox Studio's FindReplaceAll built-in plugin
    (FindReplaceWidgetHeader, SearchBox, ReplaceControls, NavigationControls,
    FindReplaceWidgetBody) using Studio's real FindReplaceAll SVG icons.
]]

local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local StudioService = game:GetService("StudioService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local changed = Vide.changed
local action = Vide.action

local toolbar = plugin:CreateToolbar("find_in_place")
local toggleButton = toolbar:CreateButton(
	"find_in_place",
	"Search and replace across all scripts in the place",
	"",
	"Find in Place"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"FindInPlace",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Left, false, false, 360, 480, 250, 200)
)
widget.Title = "Find All / Replace All"
widget.Name = "RusTudioFindInPlace"

local ICON_BASE = "rbxasset://studio_svg_textures/Lua/FindReplaceAll/Dark/Standard/"
local ICON_SEARCH = ICON_BASE .. "Search.png"
local ICON_CASE = ICON_BASE .. "MatchCase.png"
local ICON_WORD = ICON_BASE .. "MatchWholeWord.png"
local ICON_REGEX = ICON_BASE .. "RegularExpression.png"
local ICON_CLEAR = ICON_BASE .. "ClearInput.png"
local ICON_MORE = ICON_BASE .. "ThreeDots.png"
local ICON_PREV = ICON_BASE .. "FindPrevious.png"
local ICON_NEXT = ICON_BASE .. "FindNext.png"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SELECTED = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Selected)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

type MatchItem = {
	id: string,
	script: LuaSourceContainer,
	className: string,
	path: string,
	line: number,
	preview: string,
}

local function allScripts(): { LuaSourceContainer }
	local out = {}
	for _, svc in ipairs(game:GetChildren()) do
		local ok, desc = pcall(function()
			return svc:GetDescendants()
		end)
		if ok and type(desc) == "table" then
			for _, inst in ipairs(desc) do
				if inst:IsA("LuaSourceContainer") then
					table.insert(out, inst)
				end
			end
		end
	end
	table.sort(out, function(a, b)
		return a:GetFullName() < b:GetFullName()
	end)
	return out
end

local function isWordChar(ch: string): boolean
	return string.match(ch, "^[%w_]$") ~= nil
end

local function lineMatches(hay: string, query: string, plain: boolean, wholeWord: boolean): boolean
	local pos = 1
	while pos <= #hay do
		local ok, sIdx, eIdx = pcall(string.find, hay, query, pos, plain)
		if not ok or sIdx == nil or eIdx == nil then
			return false
		end
		if not wholeWord then
			return true
		end
		local beforeOk = (sIdx == 1) or not isWordChar(string.sub(hay, sIdx - 1, sIdx - 1))
		local afterOk = (eIdx >= #hay) or not isWordChar(string.sub(hay, eIdx + 1, eIdx + 1))
		if beforeOk and afterOk then
			return true
		end
		pos = math.max(sIdx + 1, eIdx + 1)
	end
	return false
end

local function searchPlace(
	needle: string,
	caseSensitive: boolean,
	wholeWord: boolean,
	usePattern: boolean
): ({ MatchItem }, number)
	if needle == "" then
		return {}, 0
	end
	local matches: { MatchItem } = {}
	local scriptCount = 0
	local query = if caseSensitive then needle else string.lower(needle)
	local plain = not usePattern

	for _, scr in ipairs(allScripts()) do
		local ok, src = pcall(function()
			return (scr :: any).Source
		end)
		if not ok or type(src) ~= "string" then
			continue
		end
		local path = scr:GetFullName()
		local hitHere = false
		local lineNo = 0
		for line in string.gmatch(src .. "\n", "([^\n]*)\n") do
			lineNo += 1
			local hay = if caseSensitive then line else string.lower(line)
			if lineMatches(hay, query, plain, wholeWord) then
				hitHere = true
				local trimmed = (string.gsub(line, "^%s+", ""))
				table.insert(matches, {
					id = `{path}:{lineNo}`,
					script = scr,
					className = scr.ClassName,
					path = path,
					line = lineNo,
					preview = trimmed,
				})
				if #matches >= 250 then
					break
				end
			end
		end
		if hitHere then
			scriptCount += 1
		end
		if #matches >= 250 then
			break
		end
	end
	return matches, scriptCount
end

local function replaceInSource(
	src: string,
	needle: string,
	repl: string,
	caseSensitive: boolean,
	wholeWord: boolean,
	usePattern: boolean
): (string, number)
	local query = if caseSensitive then needle else string.lower(needle)
	local hay = if caseSensitive then src else string.lower(src)
	local plain = not usePattern
	local out = {}
	local pos = 1
	local count = 0
	local len = #src
	while pos <= len do
		local ok, sIdx, eIdx = pcall(string.find, hay, query, pos, plain)
		if not ok or sIdx == nil or eIdx == nil or eIdx < sIdx then
			table.insert(out, string.sub(src, pos))
			break
		end
		local wordOk = true
		if wholeWord then
			local beforeOk = (sIdx == 1) or not isWordChar(string.sub(src, sIdx - 1, sIdx - 1))
			local afterOk = (eIdx >= len) or not isWordChar(string.sub(src, eIdx + 1, eIdx + 1))
			wordOk = beforeOk and afterOk
		end
		if wordOk then
			if sIdx > pos then
				table.insert(out, string.sub(src, pos, sIdx - 1))
			end
			table.insert(out, repl)
			count += 1
			pos = eIdx + 1
		else
			table.insert(out, string.sub(src, pos, sIdx))
			pos = sIdx + 1
		end
	end
	if count == 0 then
		return src, 0
	end
	return table.concat(out), count
end

local function ToggleIconBtn(name: string, icon: string, rightOffset: number, active: () -> boolean, onClick: () -> ())
	local hovered = source(false)
	return create "ImageButton" {
		Name = name,
		AnchorPoint = Vector2.new(1, 0.5),
		Position = UDim2.new(1, -rightOffset, 0.5, 0),
		Size = UDim2.fromOffset(20, 20),
		AutoButtonColor = false,
		BorderSizePixel = 0,
		BackgroundColor3 = function()
			if active() then
				return C_ACCENT()
			elseif hovered() then
				return C_BUTTON()
			end
			return C_INPUT()
		end,
		BackgroundTransparency = function()
			return if active() or hovered() then 0 else 1
		end,
		Image = icon,
		ScaleType = Enum.ScaleType.Fit,
		ImageColor3 = function()
			return if active() then Color3.fromRGB(255, 255, 255) else C_TEXT()
		end,
		MouseEnter = function() hovered(true) end,
		MouseLeave = function() hovered(false) end,
		Activated = onClick,
		{ create "UICorner" { CornerRadius = UDim.new(0, 3) } },
	}
end

local function FindInPlaceApp()
	local findQuery = source("")
	local replaceText = source("")
	local showReplace = source(true)
	local caseSensitive = source(false)
	local wholeWord = source(false)
	local usePattern = source(false)
	local results = source({} :: { MatchItem })
	local matchedScripts = source(0)
	local selectedIdx = source(0)
	local status = source("")

	local findBoxRef: TextBox? = nil
	local replaceBoxRef: TextBox? = nil

	local function runSearch()
		local m, sc = searchPlace(findQuery(), caseSensitive(), wholeWord(), usePattern())
		Vide.batch(function()
			results(m)
			matchedScripts(sc)
			selectedIdx(if #m > 0 then 1 else 0)
			status("")
		end)
	end

	local function openMatch(idx: number)
		local list = results()
		if idx < 1 or idx > #list then
			return
		end
		selectedIdx(idx)
		local item = list[idx]
		Selection:Set({ item.script })
		pcall(function()
			plugin:OpenScript(item.script, item.line)
		end)
	end

	local function stepMatch(delta: number)
		local list = results()
		if #list == 0 then
			return
		end
		local cur = selectedIdx()
		local nextIdx = ((cur - 1 + delta) % #list) + 1
		openMatch(nextIdx)
	end

	local function runReplaceAll()
		local needle = findQuery()
		if needle == "" then
			return
		end
		local repl = replaceText()
		local cs = caseSensitive()
		local ww = wholeWord()
		local pat = usePattern()
		local totalReplaced = 0
		local touchedScripts = 0

		local rec = nil
		pcall(function()
			rec = ChangeHistoryService:TryBeginRecording("FindReplaceAll", "Replace All in Place")
		end)
		local ok, err = pcall(function()
			for _, scr in ipairs(allScripts()) do
				local sOk, src = pcall(function()
					return (scr :: any).Source
				end)
				if sOk and type(src) == "string" then
					local nextSrc, count = replaceInSource(src, needle, repl, cs, ww, pat)
					if count > 0 then
						local scrAny: any = scr
						scrAny.Source = nextSrc
						totalReplaced += count
						touchedScripts += 1
					end
				end
			end
		end)
		if rec then
			pcall(function()
				ChangeHistoryService:FinishRecording(
					rec,
					if ok then Enum.FinishRecordingOperation.Commit else Enum.FinishRecordingOperation.Cancel
				)
			end)
		elseif ok then
			pcall(function()
				ChangeHistoryService:SetWaypoint("Replace All in Place")
			end)
		end

		runSearch()
		if ok then
			status(`Replaced {totalReplaced} match(es) in {touchedScripts} script(s)`)
		else
			status("Replace failed: " .. tostring(err))
		end
	end

	local headerHeight = Vide.derive(function()
		return if showReplace() then 92 else 60
	end)

	return create "Frame" {
		Name = "FindInPlaceRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- FindReplaceWidgetHeader
			create "Frame" {
				Name = "Header",
				Size = function()
					return UDim2.new(1, 0, 0, headerHeight())
				end,
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					-- Row 1: Studio SearchBox with inline Aa / ab / .* toggles + ThreeDots
					create "Frame" {
						Name = "SearchBarFrame",
						Position = UDim2.fromOffset(8, 8),
						Size = UDim2.new(1, -38, 0, 26),
						BackgroundColor3 = C_INPUT,
						BorderSizePixel = 0,
						{
							create "UICorner" { CornerRadius = UDim.new(0, 4) },
							create "UIStroke" {
								ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
								Thickness = 1,
								Color = C_BORDER,
							},
							create "ImageLabel" {
								AnchorPoint = Vector2.new(0, 0.5),
								Position = UDim2.new(0, 7, 0.5, 0),
								Size = UDim2.fromOffset(14, 14),
								BackgroundTransparency = 1,
								Image = ICON_SEARCH,
								ImageColor3 = C_DIM,
							},
							create "TextBox" {
								Name = "FindInput",
								Position = UDim2.fromOffset(26, 0),
								Size = UDim2.new(1, -112, 1, 0),
								BackgroundTransparency = 1,
								BorderSizePixel = 0,
								ClearTextOnFocus = false,
								Font = Enum.Font.SourceSans,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = C_TEXT,
								PlaceholderColor3 = C_DIM,
								PlaceholderText = "Find...",
								Text = "",
								changed("Text", function(t)
									findQuery(t)
									runSearch()
								end),
								FocusLost = function(enter)
									if enter then
										runSearch()
									end
								end,
								action(function(inst)
									findBoxRef = inst :: TextBox
								end),
							},
							show(function()
								return #findQuery() > 0
							end, function()
								return create "ImageButton" {
									Name = "ClearTextButton",
									AnchorPoint = Vector2.new(1, 0.5),
									Position = UDim2.new(1, -69, 0.5, 0),
									Size = UDim2.fromOffset(16, 16),
									BackgroundTransparency = 1,
									Image = ICON_CLEAR,
									ImageColor3 = C_DIM,
									Activated = function()
										findQuery("")
										if findBoxRef then
											findBoxRef.Text = ""
										end
										runSearch()
									end,
								}
							end),
							ToggleIconBtn("CaseBtn", ICON_CASE, 46, caseSensitive, function()
								caseSensitive(not caseSensitive())
								runSearch()
							end),
							ToggleIconBtn("WholeWordBtn", ICON_WORD, 25, wholeWord, function()
								wholeWord(not wholeWord())
								runSearch()
							end),
							ToggleIconBtn("RegexBtn", ICON_REGEX, 4, usePattern, function()
								usePattern(not usePattern())
								runSearch()
							end),
						},
					},

					-- ThreeDots menu button (toggles Replace row)
					create "ImageButton" {
						Name = "OptionsBtn",
						Position = UDim2.new(1, -26, 0, 9),
						Size = UDim2.fromOffset(22, 24),
						AutoButtonColor = false,
						BorderSizePixel = 0,
						BackgroundColor3 = function()
							return if showReplace() then C_BUTTON() else C_BG()
						end,
						Image = ICON_MORE,
						ScaleType = Enum.ScaleType.Fit,
						ImageColor3 = C_TEXT,
						Activated = function()
							showReplace(not showReplace())
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},

					-- Row 2: Replace box + Replace All button
					show(showReplace, function()
						return create "Frame" {
							Name = "ReplaceRow",
							Position = UDim2.fromOffset(8, 38),
							Size = UDim2.new(1, -16, 0, 26),
							BackgroundTransparency = 1,
							{
								create "Frame" {
									Size = UDim2.new(1, -84, 1, 0),
									BackgroundColor3 = C_INPUT,
									BorderSizePixel = 0,
									{
										create "UICorner" { CornerRadius = UDim.new(0, 4) },
										create "UIStroke" {
											ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
											Thickness = 1,
											Color = C_BORDER,
										},
										create "TextBox" {
											Name = "ReplaceInput",
											Position = UDim2.fromOffset(8, 0),
											Size = UDim2.new(1, -14, 1, 0),
											BackgroundTransparency = 1,
											BorderSizePixel = 0,
											ClearTextOnFocus = false,
											Font = Enum.Font.SourceSans,
											TextSize = 13,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_TEXT,
											PlaceholderColor3 = C_DIM,
											PlaceholderText = "Replace...",
											Text = "",
											changed("Text", function(t)
												replaceText(t)
											end),
											action(function(inst)
												replaceBoxRef = inst :: TextBox
											end),
										},
									},
								},
								create "TextButton" {
									Name = "ReplaceAllBtn",
									Position = UDim2.new(1, -78, 0, 0),
									Size = UDim2.fromOffset(78, 26),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Font = Enum.Font.SourceSans,
									TextSize = 12,
									BackgroundColor3 = C_BUTTON,
									TextColor3 = C_TEXT,
									Text = "Replace All",
									Activated = runReplaceAll,
									{
										create "UICorner" { CornerRadius = UDim.new(0, 4) },
										create "UIStroke" {
											ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
											Thickness = 1,
											Color = C_BORDER,
										},
									},
								},
							},
						}
					end),

					-- Row 3: NavigationControls (summary + Prev / Next arrows)
					create "Frame" {
						Name = "ControlsContainer",
						Position = function()
							return UDim2.fromOffset(0, headerHeight() - 24)
						end,
						Size = UDim2.new(1, 0, 0, 24),
						BackgroundColor3 = C_BG,
						BorderSizePixel = 0,
						{
							create "TextLabel" {
								Name = "Summary",
								Position = UDim2.fromOffset(10, 0),
								Size = UDim2.new(1, -66, 1, -1),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 12,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextTruncate = Enum.TextTruncate.AtEnd,
								TextColor3 = C_DIM,
								Text = function()
									if status() ~= "" then
										return status()
									end
									if findQuery() == "" then
										return `{#allScripts()} script(s) in place`
									end
									local n = #results()
									local sc = matchedScripts()
									return if n == 0
										then "No results found"
										else `{n} results in {sc} scripts`
								end,
							},
							create "ImageButton" {
								Name = "FindPreviousBtn",
								Position = UDim2.new(1, -50, 0, 2),
								Size = UDim2.fromOffset(20, 20),
								BackgroundTransparency = 1,
								Image = ICON_PREV,
								ScaleType = Enum.ScaleType.Fit,
								ImageColor3 = C_TEXT,
								Activated = function()
									stepMatch(-1)
								end,
							},
							create "ImageButton" {
								Name = "FindNextBtn",
								Position = UDim2.new(1, -26, 0, 2),
								Size = UDim2.fromOffset(20, 20),
								BackgroundTransparency = 1,
								Image = ICON_NEXT,
								ScaleType = Enum.ScaleType.Fit,
								ImageColor3 = C_TEXT,
								Activated = function()
									stepMatch(1)
								end,
							},
							create "Frame" {
								Name = "Divider",
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},
				},
			},

			-- Results body
			create "ScrollingFrame" {
				Name = "ResultsList",
				Position = function()
					return UDim2.fromOffset(0, headerHeight())
				end,
				Size = function()
					return UDim2.new(1, 0, 1, -headerHeight())
				end,
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				ScrollBarThickness = 6,
				ScrollBarImageColor3 = C_SCROLL,
				CanvasSize = UDim2.fromOffset(0, 0),
				AutomaticCanvasSize = Enum.AutomaticSize.Y,
				{
					create "UIListLayout" {
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 1),
					},
					create "UIPadding" {
						PaddingLeft = UDim.new(0, 4),
						PaddingRight = UDim.new(0, 4),
						PaddingTop = UDim.new(0, 4),
						PaddingBottom = UDim.new(0, 6),
					},

					values(results, function(item: MatchItem, order: () -> number)
						local hovered = source(false)
						local iconInfo: any = nil
						pcall(function()
							iconInfo = StudioService:GetClassIcon(item.className)
						end)
						return create "TextButton" {
							Name = "Match_" .. item.id,
							LayoutOrder = order,
							Size = UDim2.new(1, 0, 0, 38),
							AutoButtonColor = false,
							BorderSizePixel = 0,
							Text = "",
							BackgroundColor3 = function()
								if selectedIdx() == order() then
									return C_SELECTED()
								elseif hovered() then
									return C_HOVER()
								end
								return C_BG()
							end,
							BackgroundTransparency = function()
								return if selectedIdx() == order() or hovered() then 0 else 1
							end,
							MouseEnter = function() hovered(true) end,
							MouseLeave = function() hovered(false) end,
							Activated = function()
								openMatch(order())
							end,
							{
								create "UICorner" { CornerRadius = UDim.new(0, 3) },
								create "ImageLabel" {
									Position = UDim2.fromOffset(6, 3),
									Size = UDim2.fromOffset(14, 14),
									BackgroundTransparency = 1,
									Image = if iconInfo and iconInfo.Image then iconInfo.Image else "",
									ImageRectOffset = if iconInfo and iconInfo.ImageRectOffset then iconInfo.ImageRectOffset else Vector2.zero,
									ImageRectSize = if iconInfo and iconInfo.ImageRectSize then iconInfo.ImageRectSize else Vector2.zero,
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(24, 2),
									Size = UDim2.new(1, -28, 0, 16),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSansSemibold,
									TextSize = 12,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = C_TEXT,
									Text = `{item.path}  ({item.line})`,
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(24, 18),
									Size = UDim2.new(1, -28, 0, 16),
									BackgroundTransparency = 1,
									Font = Enum.Font.Code,
									TextSize = 12,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = C_DIM,
									Text = item.preview,
								},
							},
						}
					end),

					show(function()
						return #results() == 0
					end, function()
						return create "TextLabel" {
							LayoutOrder = 999999,
							Size = UDim2.new(1, 0, 0, 44),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 13,
							TextColor3 = C_DIM,
							Text = function()
								return if findQuery() == ""
									then "Enter text above to search across all scripts."
									else `No matches for "{findQuery()}"`
							end,
						}
					end),
				},
			},
		},
	}
end

local unmount: (() -> ())? = nil
local function mount()
	if unmount then return end
	unmount = Vide.mount(FindInPlaceApp, widget)
end
local function teardown()
	if unmount then
		local fn = unmount
		unmount = nil
		fn()
	end
end
local function sync()
	toggleButton:SetActive(widget.Enabled)
	if widget.Enabled then mount() else teardown() end
end

toggleButton.Click:Connect(function()
	widget.Enabled = not widget.Enabled
	sync()
end)
widget:GetPropertyChangedSignal("Enabled"):Connect(sync)
plugin.Unloading:Connect(teardown)
sync()
