--!strict
--[[
    RusTudio Built-in Suite: Style Editor (Luau / Vide)

    1:1 recreation of Roblox Studio's StyleEditor built-in plugin:
      - OnboardingPage when no StyleSheet exists ("Design your UI with Styles"
        + "Create Design" primary button).
      - MainPage 2-pane Design Tree + Selector Property Table using Studio's
        real StyleEditor SVG icons when StyleSheets exist.
]]

local Selection = game:GetService("Selection")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local changed = Vide.changed
local action = Vide.action

local toolbar = plugin:CreateToolbar("style_editor")
local toggleButton = toolbar:CreateButton(
	"style_editor",
	"Create and edit UI StyleSheets, StyleRules, and StyleLinks",
	"rbxasset://studio_svg_textures/Shared/Ribbon/Dark/Medium/RibbonStyleEditor.png",
	"Style Editor"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"StyleEditor",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 480, 500, 250, 200)
)
widget.Title = "Style Editor"
widget.Name = "RusTudioStyleEditor"

local ICON_BASE = "rbxasset://studio_svg_textures/Lua/StyleEditor/Dark/Standard/"
local ICON_DESIGN = ICON_BASE .. "StyleSheetDesign.png"
local ICON_RULE = ICON_BASE .. "StyleSheetName.png"
local ICON_ADD = ICON_BASE .. "AddNoBorder.png"

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
local C_LINK = StudioTheme.color(Enum.StudioStyleGuideColor.LinkText)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

local function trim(s: string): string
	return (string.gsub(s, "^%s*(.-)%s*$", "%1"))
end

local function withRecording(id: string, title: string, fn: () -> ())
	local rec = nil
	pcall(function()
		rec = ChangeHistoryService:TryBeginRecording(id, title)
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
			ChangeHistoryService:SetWaypoint(title)
		end)
	end
	if not ok then
		warn("[StyleEditor] " .. tostring(err))
	end
end

local function scanStyleSheets(): { Instance }
	local out = {}
	for _, svc in ipairs(game:GetChildren()) do
		local ok, desc = pcall(function()
			return svc:GetDescendants()
		end)
		if ok and type(desc) == "table" then
			for _, inst in ipairs(desc) do
				if inst.ClassName == "StyleSheet" then
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

local function rulesOf(sheet: Instance?): { Instance }
	if not sheet or not sheet.Parent then
		return {}
	end
	local out = {}
	for _, c in ipairs(sheet:GetDescendants()) do
		if c.ClassName == "StyleRule" then
			table.insert(out, c)
		end
	end
	return out
end

local function parseStyleValue(raw: string): any
	local s = trim(raw)
	if s == "true" then return true
	elseif s == "false" then return false end
	local n = tonumber(s)
	if n ~= nil then return n end
	local hex = string.match(s, "^#(%x%x%x%x%x%x)$")
	if hex then return Color3.fromHex(hex) end
	local r, g, b = string.match(s, "^rgb%s*%(%s*(%d+)%s*,%s*(%d+)%s*,%s*(%d+)%s*%)$")
	if r and g and b then
		return Color3.fromRGB(tonumber(r) :: number, tonumber(g) :: number, tonumber(b) :: number)
	end
	local us, uo = string.match(s, "^UDim%s*%(%s*([%d%.%-]+)%s*,%s*([%d%.%-]+)%s*%)$")
	if us and uo then
		return UDim.new(tonumber(us) :: number, tonumber(uo) :: number)
	end
	local xs, xo, ys, yo = string.match(s, "^UDim2%s*%(%s*([%d%.%-]+)%s*,%s*([%d%.%-]+)%s*,%s*([%d%.%-]+)%s*,%s*([%d%.%-]+)%s*%)$")
	if xs and xo and ys and yo then
		return UDim2.new(tonumber(xs) :: number, tonumber(xo) :: number, tonumber(ys) :: number, tonumber(yo) :: number)
	end
	return s
end

local function StyleEditorApp()
	local sheets = source(scanStyleSheets())
	local activeSheet = source(sheets()[1] :: Instance?)
	local rules = source(rulesOf(activeSheet()))
	local activeRule = source(rules()[1] :: Instance?)
	local rev = source(0)
	local newSelector = source("")
	local propName = source("")
	local propVal = source("")
	local status = source("")

	local selInputRef: TextBox? = nil
	local pNameRef: TextBox? = nil
	local pValRef: TextBox? = nil

	local function refresh()
		local all = scanStyleSheets()
		local cur = activeSheet()
		if not cur or not cur.Parent then
			cur = all[1]
		end
		local rlist = rulesOf(cur)
		local curR = activeRule()
		if not curR or not curR.Parent or not cur or not curR:IsDescendantOf(cur) then
			curR = rlist[1]
		end
		Vide.batch(function()
			sheets(all)
			activeSheet(cur)
			rules(rlist)
			activeRule(curR)
			rev(rev() + 1)
		end)
	end

	Vide.cleanup(game.DescendantAdded:Connect(function(inst)
		if inst.ClassName == "StyleSheet" or inst.ClassName == "StyleRule" then
			refresh()
		end
	end))
	Vide.cleanup(game.DescendantRemoving:Connect(function(inst)
		if inst.ClassName == "StyleSheet" or inst.ClassName == "StyleRule" then
			task.defer(refresh)
		end
	end))

	local function createSheet()
		local created: Instance? = nil
		withRecording("StyleEditorNewSheet", "Create StyleSheet", function()
			local sh = Instance.new("StyleSheet")
			sh.Name = `StyleSheet{#sheets() + 1}`
			sh.Parent = ReplicatedStorage
			created = sh
			Selection:Set({ sh })
		end)
		if created then
			activeSheet(created)
		end
		refresh()
		status("Created StyleSheet in ReplicatedStorage")
	end

	local function createRule()
		local sh = activeSheet()
		if not sh then
			createSheet()
			sh = activeSheet()
		end
		if not sh then return end
		local sel = trim(newSelector())
		if sel == "" then sel = "TextButton" end
		local created: Instance? = nil
		withRecording("StyleEditorNewRule", `Add StyleRule "{sel}"`, function()
			local r: any = Instance.new("StyleRule")
			r.Name = sel
			r.Selector = sel
			r.Parent = sh
			created = r
			Selection:Set({ r })
		end)
		newSelector("")
		if selInputRef then selInputRef.Text = "" end
		if created then activeRule(created) end
		refresh()
		status(`Added rule "{sel}"`)
	end

	local function setRuleProp()
		local r = activeRule()
		local k = trim(propName())
		if not r or k == "" then return end
		local v = parseStyleValue(propVal())
		local rAny: any = r
		withRecording("StyleEditorSetProp", `Set {k}`, function()
			rAny:SetProperty(k, v)
		end)
		propName("")
		propVal("")
		if pNameRef then pNameRef.Text = "" end
		if pValRef then pValRef.Text = "" end
		rev(rev() + 1)
		status(`Set {k} = {tostring(v)}`)
	end

	local function attachStyleLink()
		local sh = activeSheet()
		if not sh then
			status("Create or select a StyleSheet first")
			return
		end
		local sel = Selection:Get()
		local targets = {}
		for _, inst in ipairs(sel) do
			if inst:IsA("GuiBase2d") or inst:IsA("StarterGui") then
				table.insert(targets, inst)
			end
		end
		if #targets == 0 then
			status("Select a ScreenGui or GuiObject to attach StyleLink")
			return
		end
		withRecording("StyleEditorLink", "Attach StyleLink", function()
			for _, t in ipairs(targets) do
				local link: any = t:FindFirstChildOfClass("StyleLink")
				if not link then
					link = Instance.new("StyleLink")
					link.Parent = t
				end
				link.StyleSheet = sh
			end
		end)
		status(`Linked "{sh.Name}" to {#targets} GUI(s)`)
	end

	local activeRuleProps = Vide.derive(function(): { { key: string, val: string } }
		local _ = rev()
		local r = activeRule()
		if not r or not r.Parent then
			return {}
		end
		local ok, map = pcall(function()
			return (r :: any):GetProperties()
		end)
		local out = {}
		if ok and type(map) == "table" then
			for k, v in pairs(map) do
				table.insert(out, { key = tostring(k), val = tostring(v) })
			end
		end
		table.sort(out, function(a, b) return a.key < b.key end)
		return out
	end)

	return create "Frame" {
		Name = "StyleEditorRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- OnboardingPage (when place has no StyleSheets yet — exact Studio OnboardingPage)
			show(function()
				return #sheets() == 0
			end, function()
				return create "Frame" {
					Name = "OnboardingPage",
					Size = UDim2.fromScale(1, 1),
					BackgroundColor3 = C_BG,
					BorderSizePixel = 0,
					{
						create "Frame" {
							AnchorPoint = Vector2.new(0.5, 0.5),
							Position = UDim2.fromScale(0.5, 0.48),
							Size = UDim2.new(1, -24, 0, 224),
							BackgroundColor3 = C_HEADER,
							BorderSizePixel = 0,
							{
								create "UICorner" { CornerRadius = UDim.new(0, 6) },
								create "UIStroke" {
									ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
									Thickness = 1,
									Color = C_BORDER,
								},
								create "UIPadding" {
									PaddingLeft = UDim.new(0, 14),
									PaddingRight = UDim.new(0, 14),
									PaddingTop = UDim.new(0, 16),
									PaddingBottom = UDim.new(0, 16),
								},
								create "ImageLabel" {
									Size = UDim2.fromOffset(36, 36),
									BackgroundTransparency = 1,
									Image = ICON_DESIGN,
									ImageColor3 = C_ACCENT,
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(0, 46),
									Size = UDim2.new(1, 0, 0, 22),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSansBold,
									TextSize = 16,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_TEXT,
									Text = "Design your UI with Styles",
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(0, 72),
									Size = UDim2.new(1, 0, 0, 64),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextWrapped = true,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextYAlignment = Enum.TextYAlignment.Top,
									TextColor3 = C_DIM,
									Text = "Create a StyleSheet to define reusable StyleRules and link them to ScreenGuis across your place.",
								},
								create "TextButton" {
									Name = "LearnMoreBtn",
									Position = UDim2.new(0, 0, 1, -28),
									Size = UDim2.fromOffset(72, 28),
									BackgroundTransparency = 1,
									BorderSizePixel = 0,
									AutoButtonColor = false,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_LINK,
									Text = "Learn More",
									Activated = function()
										pcall(function()
											plugin:OpenWikiPage("https://create.roblox.com/docs/ui/styling")
										end)
									end,
								},
								create "TextButton" {
									Name = "NewSheetBtn",
									AnchorPoint = Vector2.new(1, 1),
									Position = UDim2.fromScale(1, 1),
									Size = UDim2.fromOffset(102, 28),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Font = Enum.Font.SourceSansSemibold,
									TextSize = 13,
									BackgroundColor3 = C_ACCENT,
									TextColor3 = C_ACCENT_TEXT,
									Text = "Create Design",
									Activated = createSheet,
									{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
								},
							},
						},
					},
				}
			end),

			-- MainPage (2-pane Design Tree + Selector Property Table)
			show(function()
				return #sheets() > 0
			end, function()
				return create "Frame" {
					Name = "MainPage",
					Size = UDim2.fromScale(1, 1),
					BackgroundColor3 = C_BG,
					BorderSizePixel = 0,
					{
						-- Top toolbar
						create "Frame" {
							Name = "TopBar",
							Size = UDim2.new(1, 0, 0, 34),
							BackgroundColor3 = C_HEADER,
							BorderSizePixel = 0,
							{
								create "Frame" {
									Position = UDim2.new(0, 0, 1, -1),
									Size = UDim2.new(1, 0, 0, 1),
									BackgroundColor3 = C_BORDER,
									BorderSizePixel = 0,
								},
								create "TextButton" {
									Name = "NewSheetBtn",
									Position = UDim2.fromOffset(8, 5),
									Size = UDim2.fromOffset(88, 24),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Font = Enum.Font.SourceSansSemibold,
									TextSize = 12,
									BackgroundColor3 = C_ACCENT,
									TextColor3 = C_ACCENT_TEXT,
									Text = "+ StyleSheet",
									Activated = createSheet,
									{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
								},
								create "TextButton" {
									Name = "AttachLinkBtn",
									Position = UDim2.fromOffset(102, 5),
									Size = UDim2.fromOffset(88, 24),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Font = Enum.Font.SourceSans,
									TextSize = 12,
									BackgroundColor3 = C_BUTTON,
									TextColor3 = C_TEXT,
									Text = "Attach Link",
									Activated = attachStyleLink,
									{
										create "UICorner" { CornerRadius = UDim.new(0, 4) },
										create "UIStroke" {
											ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
											Thickness = 1,
											Color = C_BORDER,
										},
									},
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(196, 0),
									Size = UDim2.new(1, -204, 1, 0),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 11,
									TextXAlignment = Enum.TextXAlignment.Right,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = C_DIM,
									Text = status,
								},
							},
						},

						-- Left pane: StyleSheets + Rules tree
						create "Frame" {
							Name = "LeftTree",
							Position = UDim2.fromOffset(0, 34),
							Size = UDim2.new(0.44, 0, 1, -34),
							BackgroundColor3 = C_BG,
							BorderSizePixel = 0,
							{
								create "ScrollingFrame" {
									Size = UDim2.new(1, 0, 1, -36),
									BackgroundTransparency = 1,
									BorderSizePixel = 0,
									ScrollBarThickness = 5,
									ScrollBarImageColor3 = C_SCROLL,
									CanvasSize = UDim2.fromOffset(0, 0),
									AutomaticCanvasSize = Enum.AutomaticSize.Y,
									{
										create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder, Padding = UDim.new(0, 1) },
										create "UIPadding" {
											PaddingLeft = UDim.new(0, 4),
											PaddingRight = UDim.new(0, 4),
											PaddingTop = UDim.new(0, 4),
										},
										values(sheets, function(sh: Instance, order: () -> number)
											return create "TextButton" {
												LayoutOrder = order,
												Size = UDim2.new(1, 0, 0, 24),
												AutoButtonColor = false,
												BorderSizePixel = 0,
												Text = "",
												BackgroundColor3 = function()
													return if activeSheet() == sh then C_SELECTED() else C_BG()
												end,
												Activated = function()
													activeSheet(sh)
													rules(rulesOf(sh))
													activeRule(rules()[1])
													Selection:Set({ sh })
												end,
												{
													create "UICorner" { CornerRadius = UDim.new(0, 3) },
													create "ImageLabel" {
														Position = UDim2.fromOffset(5, 4),
														Size = UDim2.fromOffset(16, 16),
														BackgroundTransparency = 1,
														Image = ICON_DESIGN,
														ImageColor3 = C_TEXT,
													},
													create "TextLabel" {
														Position = UDim2.fromOffset(25, 0),
														Size = UDim2.new(1, -28, 1, 0),
														BackgroundTransparency = 1,
														Font = Enum.Font.SourceSansSemibold,
														TextSize = 12,
														TextXAlignment = Enum.TextXAlignment.Left,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_TEXT,
														Text = sh.Name,
													},
												},
											}
										end),
										values(rules, function(r: Instance, order: () -> number)
											local selTxt = ""
											pcall(function() selTxt = tostring((r :: any).Selector) end)
											if selTxt == "" then selTxt = r.Name end
											return create "TextButton" {
												LayoutOrder = function() return 1000 + order() end,
												Size = UDim2.new(1, 0, 0, 22),
												AutoButtonColor = false,
												BorderSizePixel = 0,
												Text = "",
												BackgroundColor3 = function()
													return if activeRule() == r then C_SELECTED() else C_BG()
												end,
												Activated = function()
													activeRule(r)
													Selection:Set({ r })
												end,
												{
													create "UICorner" { CornerRadius = UDim.new(0, 3) },
													create "ImageLabel" {
														Position = UDim2.fromOffset(18, 4),
														Size = UDim2.fromOffset(14, 14),
														BackgroundTransparency = 1,
														Image = ICON_RULE,
														ImageColor3 = C_DIM,
													},
													create "TextLabel" {
														Position = UDim2.fromOffset(36, 0),
														Size = UDim2.new(1, -40, 1, 0),
														BackgroundTransparency = 1,
														Font = Enum.Font.SourceSans,
														TextSize = 12,
														TextXAlignment = Enum.TextXAlignment.Left,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_TEXT,
														Text = selTxt,
													},
												},
											}
										end),
									},
								},
								-- Bottom "+ Rule" input
								create "Frame" {
									Position = UDim2.new(0, 0, 1, -36),
									Size = UDim2.new(1, 0, 0, 36),
									BackgroundColor3 = C_HEADER,
									BorderSizePixel = 0,
									{
										create "Frame" {
											Size = UDim2.new(1, 0, 0, 1),
											BackgroundColor3 = C_BORDER,
											BorderSizePixel = 0,
										},
										create "Frame" {
											Position = UDim2.fromOffset(6, 6),
											Size = UDim2.new(1, -42, 0, 24),
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
													Position = UDim2.fromOffset(6, 0),
													Size = UDim2.new(1, -10, 1, 0),
													BackgroundTransparency = 1,
													BorderSizePixel = 0,
													ClearTextOnFocus = false,
													Font = Enum.Font.SourceSans,
													TextSize = 12,
													TextXAlignment = Enum.TextXAlignment.Left,
													TextColor3 = C_TEXT,
													PlaceholderColor3 = C_DIM,
													PlaceholderText = "Selector...",
													Text = "",
													changed("Text", function(t) newSelector(t) end),
													FocusLost = function(e) if e then createRule() end end,
													action(function(inst) selInputRef = inst :: TextBox end),
												},
											},
										},
										create "ImageButton" {
											Name = "AddRuleBtn",
											Position = UDim2.new(1, -30, 0, 6),
											Size = UDim2.fromOffset(24, 24),
											AutoButtonColor = false,
											BackgroundColor3 = C_BUTTON,
											BorderSizePixel = 0,
											Image = ICON_ADD,
											ScaleType = Enum.ScaleType.Fit,
											ImageColor3 = C_TEXT,
											Activated = createRule,
											{
												create "UICorner" { CornerRadius = UDim.new(0, 4) },
												create "UIPadding" {
													PaddingLeft = UDim.new(0, 4),
													PaddingRight = UDim.new(0, 4),
													PaddingTop = UDim.new(0, 4),
													PaddingBottom = UDim.new(0, 4),
												},
											},
										},
									},
								},
							},
						},

						-- Vertical divider
						create "Frame" {
							Position = UDim2.new(0.44, 0, 0, 34),
							Size = UDim2.new(0, 1, 1, -34),
							BackgroundColor3 = C_BORDER,
							BorderSizePixel = 0,
						},

						-- Right pane: SelectorPropertyTable
						create "Frame" {
							Name = "RightProps",
							Position = UDim2.new(0.44, 1, 0, 34),
							Size = UDim2.new(0.56, -1, 1, -34),
							BackgroundColor3 = C_BG,
							BorderSizePixel = 0,
							{
								create "Frame" {
									Size = UDim2.new(1, 0, 0, 24),
									BackgroundColor3 = C_HEADER,
									BorderSizePixel = 0,
									{
										create "TextLabel" {
											Position = UDim2.fromOffset(8, 0),
											Size = UDim2.new(0.5, -8, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_DIM,
											Text = "Property",
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.5, 0),
											Size = UDim2.new(0.5, -8, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_DIM,
											Text = "Value",
										},
										create "Frame" {
											Position = UDim2.new(0, 0, 1, -1),
											Size = UDim2.new(1, 0, 0, 1),
											BackgroundColor3 = C_BORDER,
											BorderSizePixel = 0,
										},
									},
								},
								create "ScrollingFrame" {
									Position = UDim2.fromOffset(0, 24),
									Size = UDim2.new(1, 0, 1, -60),
									BackgroundTransparency = 1,
									BorderSizePixel = 0,
									ScrollBarThickness = 5,
									ScrollBarImageColor3 = C_SCROLL,
									CanvasSize = UDim2.fromOffset(0, 0),
									AutomaticCanvasSize = Enum.AutomaticSize.Y,
									{
										create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
										values(activeRuleProps, function(row: { key: string, val: string }, order: () -> number)
											return create "Frame" {
												LayoutOrder = order,
												Size = UDim2.new(1, 0, 0, 24),
												BackgroundTransparency = 1,
												{
													create "TextLabel" {
														Position = UDim2.fromOffset(8, 0),
														Size = UDim2.new(0.5, -10, 1, 0),
														BackgroundTransparency = 1,
														Font = Enum.Font.SourceSans,
														TextSize = 12,
														TextXAlignment = Enum.TextXAlignment.Left,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_TEXT,
														Text = row.key,
													},
													create "TextLabel" {
														Position = UDim2.fromScale(0.5, 0),
														Size = UDim2.new(0.5, -8, 1, 0),
														BackgroundTransparency = 1,
														Font = Enum.Font.Code,
														TextSize = 12,
														TextXAlignment = Enum.TextXAlignment.Left,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_DIM,
														Text = row.val,
													},
												},
											}
										end),
									},
								},
								-- Bottom "+ Property" row
								create "Frame" {
									Position = UDim2.new(0, 0, 1, -36),
									Size = UDim2.new(1, 0, 0, 36),
									BackgroundColor3 = C_HEADER,
									BorderSizePixel = 0,
									{
										create "Frame" {
											Size = UDim2.new(1, 0, 0, 1),
											BackgroundColor3 = C_BORDER,
											BorderSizePixel = 0,
										},
										create "Frame" {
											Position = UDim2.fromOffset(6, 6),
											Size = UDim2.new(0.45, -8, 0, 24),
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
													Position = UDim2.fromOffset(6, 0),
													Size = UDim2.new(1, -10, 1, 0),
													BackgroundTransparency = 1,
													ClearTextOnFocus = false,
													Font = Enum.Font.SourceSans,
													TextSize = 12,
													TextXAlignment = Enum.TextXAlignment.Left,
													TextColor3 = C_TEXT,
													PlaceholderColor3 = C_DIM,
													PlaceholderText = "Property",
													Text = "",
													changed("Text", function(t) propName(t) end),
													action(function(inst) pNameRef = inst :: TextBox end),
												},
											},
										},
										create "Frame" {
											Position = UDim2.new(0.45, 2, 0, 6),
											Size = UDim2.new(0.55, -38, 0, 24),
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
													Position = UDim2.fromOffset(6, 0),
													Size = UDim2.new(1, -10, 1, 0),
													BackgroundTransparency = 1,
													ClearTextOnFocus = false,
													Font = Enum.Font.SourceSans,
													TextSize = 12,
													TextXAlignment = Enum.TextXAlignment.Left,
													TextColor3 = C_TEXT,
													PlaceholderColor3 = C_DIM,
													PlaceholderText = "Value",
													Text = "",
													changed("Text", function(t) propVal(t) end),
													FocusLost = function(e) if e then setRuleProp() end end,
													action(function(inst) pValRef = inst :: TextBox end),
												},
											},
										},
										create "TextButton" {
											Name = "SetPropBtn",
											Position = UDim2.new(1, -32, 0, 6),
											Size = UDim2.fromOffset(26, 24),
											AutoButtonColor = false,
											BorderSizePixel = 0,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 12,
											BackgroundColor3 = C_ACCENT,
											TextColor3 = C_ACCENT_TEXT,
											Text = "Set",
											Activated = setRuleProp,
											{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
										},
									},
								},
							},
						},
					},
				}
			end),
		},
	}
end

local unmount: (() -> ())? = nil
local function mount()
	if unmount then return end
	unmount = Vide.mount(StyleEditorApp, widget)
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
