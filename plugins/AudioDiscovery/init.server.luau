--!strict
--[[
    RusTudio Built-in Suite: Audio Discovery (Luau / Vide)

    1:1 recreation of Roblox Studio's AudioDiscovery built-in plugin
    (Window, LeftPanel 5-column table [OK, Id, Name, Time, Creator],
    RightPanel 2-column table [Instance, Path]) with SoundService preview.
]]

local Selection = game:GetService("Selection")
local SoundService = game:GetService("SoundService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local changed = Vide.changed

local toolbar = plugin:CreateToolbar("audio_discovery")
local toggleButton = toolbar:CreateButton(
	"audio_discovery",
	"Inspect, filter, and preview all Sound and AudioPlayer instances in the place",
	"",
	"View Sounds"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"AudioDiscovery",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Bottom, false, false, 720, 320, 360, 180)
)
widget.Title = "Audio Discovery"
widget.Name = "RusTudioAudioDiscovery"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SELECTED = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Selected)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

local function isAudioInstance(inst: Instance): boolean
	return inst:IsA("Sound") or inst:IsA("AudioPlayer")
end

local function audioAssetId(inst: Instance): string
	if inst:IsA("Sound") then
		return tostring(inst.SoundId or "")
	end
	local ok, id = pcall(function()
		return (inst :: any).Asset
	end)
	if ok and type(id) == "string" then
		return id
	end
	return ""
end

local function audioDuration(inst: Instance): string
	local ok, len = pcall(function()
		return (inst :: any).TimeLength
	end)
	if ok and type(len) == "number" and len > 0 then
		return string.format("%.1fs", len)
	end
	return "—"
end

local function scanAudio(): { Instance }
	local out = {}
	for _, svc in ipairs(game:GetChildren()) do
		local ok, desc = pcall(function()
			return svc:GetDescendants()
		end)
		if ok and type(desc) == "table" then
			for _, inst in ipairs(desc) do
				if isAudioInstance(inst) then
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

local function HeaderCell(text: string, xScale: number, wScale: number)
	return create "TextLabel" {
		Position = UDim2.fromScale(xScale, 0),
		Size = UDim2.new(wScale, 0, 1, 0),
		BackgroundTransparency = 1,
		Font = Enum.Font.SourceSansSemibold,
		TextSize = 12,
		TextXAlignment = Enum.TextXAlignment.Left,
		TextColor3 = C_DIM,
		Text = text,
		{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
	}
end

local function AudioDiscoveryApp()
	local query = source("")
	local allAudio = source(scanAudio())
	local selectedInst = source(nil :: Instance?)

	local function refresh()
		allAudio(scanAudio())
	end

	Vide.cleanup(game.DescendantAdded:Connect(function(inst)
		if isAudioInstance(inst) then refresh() end
	end))
	Vide.cleanup(game.DescendantRemoving:Connect(function(inst)
		if isAudioInstance(inst) then task.defer(refresh) end
	end))

	local filtered = Vide.derive(function(): { Instance }
		local list = allAudio()
		local q = string.lower(string.gsub(query(), "^%s*(.-)%s*$", "%1"))
		if q == "" then
			return list
		end
		local out = {}
		for _, inst in ipairs(list) do
			if inst.Parent then
				local hay = string.lower(inst:GetFullName() .. " " .. audioAssetId(inst))
				if string.find(hay, q, 1, true) ~= nil then
					table.insert(out, inst)
				end
			end
		end
		return out
	end)

	local rightRows = Vide.derive(function(): { Instance }
		local sel = selectedInst()
		if sel and sel.Parent then
			local targetId = audioAssetId(sel)
			local out = {}
			for _, inst in ipairs(allAudio()) do
				if inst.Parent and (inst == sel or (targetId ~= "" and audioAssetId(inst) == targetId)) then
					table.insert(out, inst)
				end
			end
			return out
		end
		return filtered()
	end)

	return create "Frame" {
		Name = "AudioDiscoveryRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- LEFT PANEL (60%): OK | Id | Name | Time | Creator
			create "Frame" {
				Name = "LeftPanel",
				Size = UDim2.new(0.6, 0, 1, 0),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					-- Top bar (34px, matches Window.bin)
					create "Frame" {
						Name = "LeftTop",
						Size = UDim2.new(1, 0, 0, 34),
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						{
							create "TextButton" {
								Name = "RefreshBtn",
								Position = UDim2.fromOffset(8, 5),
								Size = UDim2.fromOffset(64, 24),
								AutoButtonColor = false,
								BorderSizePixel = 0,
								Font = Enum.Font.SourceSans,
								TextSize = 12,
								BackgroundColor3 = C_BUTTON,
								TextColor3 = C_TEXT,
								Text = "Refresh",
								Activated = refresh,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = C_BORDER,
									},
								},
							},
							create "Frame" {
								Position = UDim2.fromOffset(78, 5),
								Size = UDim2.new(1, -158, 0, 24),
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
										Name = "SearchBox",
										Position = UDim2.fromOffset(8, 0),
										Size = UDim2.new(1, -12, 1, 0),
										BackgroundTransparency = 1,
										BorderSizePixel = 0,
										ClearTextOnFocus = false,
										Font = Enum.Font.SourceSans,
										TextSize = 12,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = C_TEXT,
										PlaceholderColor3 = C_DIM,
										PlaceholderText = "Filter audio...",
										Text = "",
										changed("Text", function(t) query(t) end),
									},
								},
							},
							create "TextLabel" {
								Position = UDim2.new(1, -74, 0, 0),
								Size = UDim2.fromOffset(66, 34),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 12,
								TextXAlignment = Enum.TextXAlignment.Right,
								TextColor3 = C_DIM,
								Text = function() return `{#filtered()} found` end,
							},
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},

					-- Column header row (0.10 / 0.25 / 0.30 / 0.12 / 0.23)
					create "Frame" {
						Name = "LeftTableHeader",
						Position = UDim2.fromOffset(0, 34),
						Size = UDim2.new(1, 0, 0, 24),
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						{
							HeaderCell("OK", 0.0, 0.10),
							HeaderCell("Id", 0.10, 0.28),
							HeaderCell("Name", 0.38, 0.28),
							HeaderCell("Time", 0.66, 0.12),
							HeaderCell("Type", 0.78, 0.22),
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},

					-- Rows
					create "ScrollingFrame" {
						Name = "LeftTableBody",
						Position = UDim2.fromOffset(0, 58),
						Size = UDim2.new(1, 0, 1, -58),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						ScrollBarThickness = 6,
						ScrollBarImageColor3 = C_SCROLL,
						CanvasSize = UDim2.fromOffset(0, 0),
						AutomaticCanvasSize = Enum.AutomaticSize.Y,
						{
							create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
							values(filtered, function(inst: Instance, order: () -> number)
								local hovered = source(false)
								local assetId = audioAssetId(inst)
								local isSound = inst:IsA("Sound")
								local okAsset = assetId ~= ""
								return create "TextButton" {
									Name = "Audio_" .. inst.Name,
									LayoutOrder = order,
									Size = UDim2.new(1, 0, 0, 26),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Text = "",
									BackgroundColor3 = function()
										if selectedInst() == inst then
											return C_SELECTED()
										elseif hovered() then
											return C_HOVER()
										end
										return C_BG()
									end,
									MouseEnter = function() hovered(true) end,
									MouseLeave = function() hovered(false) end,
									Activated = function()
										selectedInst(inst)
										Selection:Set({ inst })
									end,
									{
										create "TextButton" {
											Name = "PlayBtn",
											Position = UDim2.new(0, 6, 0.5, -9),
											Size = UDim2.fromOffset(18, 18),
											AutoButtonColor = false,
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSansBold,
											TextSize = 11,
											TextColor3 = if okAsset then Color3.fromRGB(92, 198, 114) else C_DIM,
											Text = if isSound then "▶" else "•",
											Activated = function()
												selectedInst(inst)
												if isSound then
													pcall(function() SoundService:PlayLocalSound(inst :: Sound) end)
												end
											end,
										},
										create "TextLabel" {
											Name = "AssetIdLabel",
											Position = UDim2.fromScale(0.10, 0),
											Size = UDim2.new(0.28, -4, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = if assetId ~= "" then assetId else "(empty)",
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.38, 0),
											Size = UDim2.new(0.28, -4, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = inst.Name,
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.66, 0),
											Size = UDim2.new(0.12, -4, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_DIM,
											Text = audioDuration(inst),
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.78, 0),
											Size = UDim2.new(0.22, -6, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_DIM,
											Text = inst.ClassName,
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
									},
								}
							end),
							show(function() return #filtered() == 0 end, function()
								return create "TextLabel" {
									Size = UDim2.new(1, 0, 0, 44),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextColor3 = C_DIM,
									Text = "No Sound or AudioPlayer instances in this place.",
								}
							end),
						},
					},
				},
			},

			-- Vertical split divider
			create "Frame" {
				Position = UDim2.fromScale(0.6, 0),
				Size = UDim2.new(0, 1, 1, 0),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- RIGHT PANEL (40%): Instances & Scripts (Instance | Path)
			create "Frame" {
				Name = "RightPanel",
				Position = UDim2.new(0.6, 1, 0, 0),
				Size = UDim2.new(0.4, -1, 1, 0),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					create "Frame" {
						Name = "RightTop",
						Size = UDim2.new(1, 0, 0, 34),
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						{
							create "TextLabel" {
								Position = UDim2.fromOffset(10, 0),
								Size = UDim2.new(1, -20, 1, 0),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSansBold,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = C_TEXT,
								Text = "Instances & Scripts",
							},
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},
					create "Frame" {
						Name = "RightTableHeader",
						Position = UDim2.fromOffset(0, 34),
						Size = UDim2.new(1, 0, 0, 24),
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						{
							HeaderCell("Instance", 0.0, 0.34),
							HeaderCell("Path", 0.34, 0.66),
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},
					create "ScrollingFrame" {
						Name = "RightTableBody",
						Position = UDim2.fromOffset(0, 58),
						Size = UDim2.new(1, 0, 1, -58),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						ScrollBarThickness = 6,
						ScrollBarImageColor3 = C_SCROLL,
						CanvasSize = UDim2.fromOffset(0, 0),
						AutomaticCanvasSize = Enum.AutomaticSize.Y,
						{
							create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
							values(rightRows, function(inst: Instance, order: () -> number)
								local hovered = source(false)
								return create "TextButton" {
									LayoutOrder = order,
									Size = UDim2.new(1, 0, 0, 26),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Text = "",
									BackgroundColor3 = function()
										return if hovered() then C_HOVER() else C_BG()
									end,
									MouseEnter = function() hovered(true) end,
									MouseLeave = function() hovered(false) end,
									Activated = function()
										Selection:Set({ inst })
									end,
									{
										create "TextLabel" {
											Size = UDim2.new(0.34, -4, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = inst.Name,
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.34, 0),
											Size = UDim2.new(0.66, -6, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_DIM,
											Text = inst:GetFullName(),
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
									},
								}
							end),
						},
					},
				},
			},
		},
	}
end

local unmount: (() -> ())? = nil
local function mount()
	if unmount then return end
	unmount = Vide.mount(AudioDiscoveryApp, widget)
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
