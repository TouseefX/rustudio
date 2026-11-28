--!strict
--[[
    RusTudio Built-in Suite: Event Emulator (Luau / Vide)

    1:1 RE recreation of Roblox Studio's EventEmulator built-in plugin:
      - InputPane with Event Type Dropdown (Theme, RBXEvent, MemStorage),
        NameBox, active View (ThemeView / RBXEventView / MemStorageEventView),
        and ButtonArray (Save / Send).
      - RepopulatableHistory below: clicking any saved/sent history item
        repopulates the InputPane & switches to its view so it can be re-sent.
]]

local MemStorageService = game:GetService("MemStorageService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local changed = Vide.changed
local effect = Vide.effect
local action = Vide.action

local toolbar = plugin:CreateToolbar("event_emulator")
local toggleButton = toolbar:CreateButton(
	"event_emulator",
	"Emulate Theme, RBXEvent, and MemStorage events with repopulatable history",
	"",
	"Event Emulator"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"EventEmulator",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 320, 480, 240, 300)
)
widget.Title = "Event Emulator"
widget.Name = "RusTudioEventEmulator"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SELECTED = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Selected)
local C_BLUE = Color3.fromRGB(0, 162, 255) -- Exact EventEmulator MakeTheme blue
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

type ViewKind = "Theme" | "RBXEvent" | "MemStorage"
local VIEW_OPTIONS: { ViewKind } = { "Theme", "RBXEvent", "MemStorage" }

type HistoryItem = {
	id: number,
	view: ViewKind,
	name: string,
	data: { [string]: string },
}

local function trim(s: string): string
	return (string.gsub(s, "^%s*(.-)%s*$", "%1"))
end

local function EventEmulatorApp()
	local activeView = source("MemStorage" :: ViewKind)
	local dropdownOpen = source(false)
	local eventName = source("")

	-- ThemeView state
	local themeChoice = source("Dark")

	-- RBXEventView state (Namespace / Detail / DetailType)
	local rbxNamespace = source("UpdateNotificationBadge")
	local rbxDetail = source('{"count":1}')
	local rbxDetailType = source("Notification")

	-- MemStorageEventView state (Key / Value)
	local memKey = source("ThemeUpdate")
	local memVal = source("Dark")

	local statusMsg = source("")
	local history = source({
		{
			id = 1,
			view = "Theme" :: ViewKind,
			name = "Example",
			data = { theme = "Dark" },
		},
	} :: { HistoryItem })
	local nextId = 2

	local function LabeledInput(label: string, name: string, sig: any, order: number)
		local ref: TextBox? = nil
		effect(function()
			local v = sig()
			if ref and not ref:IsFocused() and ref.Text ~= v then
				ref.Text = v
			end
		end)
		return create "Frame" {
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 26),
			BackgroundTransparency = 1,
			{
				create "TextLabel" {
					Size = UDim2.new(0, 74, 1, 0),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSans,
					TextSize = 13,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_TEXT,
					Text = label,
				},
				create "Frame" {
					Position = UDim2.fromOffset(78, 1),
					Size = UDim2.new(1, -78, 0, 24),
					BackgroundColor3 = C_INPUT,
					BorderSizePixel = 0,
					{
						create "UICorner" { CornerRadius = UDim.new(0, 3) },
						create "UIStroke" {
							ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
							Thickness = 1,
							Color = C_BORDER,
						},
						create "TextBox" {
							Name = name,
							Position = UDim2.fromOffset(6, 0),
							Size = UDim2.new(1, -10, 1, 0),
							BackgroundTransparency = 1,
							BorderSizePixel = 0,
							ClearTextOnFocus = false,
							Font = Enum.Font.SourceSans,
							TextSize = 13,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextColor3 = C_TEXT,
							Text = sig(),
							changed("Text", function(t) sig(t) end),
							action(function(inst) ref = inst :: TextBox end),
						},
					},
				},
			},
		}
	end

	local function captureItem(): HistoryItem
		local v = activeView()
		local nm = trim(eventName())
		if v == "Theme" then
			return {
				id = nextId,
				view = v,
				name = if nm ~= "" then nm else `Use {themeChoice()}`,
				data = { theme = themeChoice() },
			}
		elseif v == "RBXEvent" then
			return {
				id = nextId,
				view = v,
				name = if nm ~= "" then nm else trim(rbxNamespace()),
				data = {
					namespace = rbxNamespace(),
					detail = rbxDetail(),
					detailType = rbxDetailType(),
				},
			}
		else
			return {
				id = nextId,
				view = v,
				name = if nm ~= "" then nm else trim(memKey()),
				data = {
					key = memKey(),
					value = memVal(),
				},
			}
		end
	end

	local function saveToHistory()
		local item = captureItem()
		if item.name == "" then
			item.name = item.view
		end
		nextId += 1
		local nextList = { item }
		for _, old in ipairs(history()) do
			table.insert(nextList, old)
		end
		history(nextList)
		statusMsg(`Saved "{item.name}" to History`)
	end

	local function sendCurrentEvent()
		local ms: any = MemStorageService
		local v = activeView()
		if v == "Theme" then
			local th = themeChoice()
			local ok, err = pcall(function()
				ms:SetItem("ThemeUpdate", th)
				ms:Fire("ThemeUpdate", th)
			end)
			if ok then
				statusMsg(`This operation was successful! (Theme = {th})`)
			else
				statusMsg(tostring(err))
			end
		elseif v == "RBXEvent" then
			local ns = trim(rbxNamespace())
			if ns == "" then
				statusMsg("Operation Invalid: Must provide Namespace")
				return
			end
			local ok, err = pcall(function()
				ms:Fire("RBXEvent:" .. ns, rbxDetail())
			end)
			if ok then
				statusMsg(`This operation was successful! ({ns})`)
			else
				statusMsg(tostring(err))
			end
		else
			local k = trim(memKey())
			if k == "" then
				statusMsg("Operation Invalid: Must provide key")
				return
			end
			local ok, err = pcall(function()
				ms:SetItem(k, memVal())
				ms:Fire(k, memVal())
			end)
			if ok then
				statusMsg(`This operation was successful! ({k})`)
			else
				statusMsg(tostring(err))
			end
		end
	end

	local function repopulateFrom(item: HistoryItem)
		Vide.batch(function()
			activeView(item.view)
			eventName(item.name)
			if item.view == "Theme" then
				themeChoice(item.data.theme or "Dark")
			elseif item.view == "RBXEvent" then
				rbxNamespace(item.data.namespace or "")
				rbxDetail(item.data.detail or "")
				rbxDetailType(item.data.detailType or "")
			else
				memKey(item.data.key or "")
				memVal(item.data.value or "")
			end
			statusMsg(`Loaded "{item.name}" ({item.view})`)
		end)
	end

	local function removeHistory(id: number)
		local nextList = {}
		for _, it in ipairs(history()) do
			if it.id ~= id then
				table.insert(nextList, it)
			end
		end
		history(nextList)
	end

	local function RadioThemeBtn(choice: "Dark" | "Light", order: number)
		local isSel = function()
			return themeChoice() == choice
		end
		return create "TextButton" {
			Name = "Use" .. choice,
			LayoutOrder = order,
			Size = UDim2.new(0.5, -4, 0, 24),
			BackgroundTransparency = 1,
			BorderSizePixel = 0,
			AutoButtonColor = false,
			Text = "",
			Activated = function() themeChoice(choice) end,
			{
				create "Frame" {
					Position = UDim2.new(0, 0, 0.5, -7),
					Size = UDim2.fromOffset(14, 14),
					BackgroundColor3 = function()
						return if isSel() then C_BLUE else C_INPUT()
					end,
					BorderSizePixel = 0,
					{
						create "UICorner" { CornerRadius = UDim.new(1, 0) },
						create "UIStroke" {
							ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
							Thickness = 1,
							Color = function() return if isSel() then C_BLUE else C_BORDER() end,
						},
						create "Frame" {
							AnchorPoint = Vector2.new(0.5, 0.5),
							Position = UDim2.fromScale(0.5, 0.5),
							Size = UDim2.fromOffset(6, 6),
							Visible = isSel,
							BackgroundColor3 = Color3.fromRGB(255, 255, 255),
							BorderSizePixel = 0,
							{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
						},
					},
				},
				create "TextLabel" {
					Position = UDim2.fromOffset(20, 0),
					Size = UDim2.new(1, -20, 1, 0),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSans,
					TextSize = 13,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_TEXT,
					Text = "Use " .. choice,
				},
			},
		}
	end

	return create "ScrollingFrame" {
		Name = "EventEmulator",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		CanvasSize = UDim2.fromOffset(0, 0),
		AutomaticCanvasSize = Enum.AutomaticSize.Y,
		ScrollBarThickness = 5,
		ScrollBarImageColor3 = C_SCROLL,
		{
			create "UIPadding" {
				PaddingLeft = UDim.new(0, 10),
				PaddingRight = UDim.new(0, 10),
				PaddingTop = UDim.new(0, 8),
				PaddingBottom = UDim.new(0, 8),
			},
			create "UIListLayout" {
				FillDirection = Enum.FillDirection.Vertical,
				SortOrder = Enum.SortOrder.LayoutOrder,
				Padding = UDim.new(0, 6),
			},

			-- 1. InputPane (exact Studio EventEmulator top section)
			create "Frame" {
				Name = "InputPane",
				LayoutOrder = 1,
				Size = UDim2.new(1, 0, 0, 0),
				AutomaticSize = Enum.AutomaticSize.Y,
				BackgroundTransparency = 1,
				{
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 5),
					},

					-- Event Type Dropdown row
					create "Frame" {
						Name = "DropdownRow",
						LayoutOrder = 1,
						Size = UDim2.new(1, 0, 0, 26),
						BackgroundTransparency = 1,
						{
							create "TextLabel" {
								Size = UDim2.new(0, 74, 1, 0),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = C_TEXT,
								Text = "Current Event",
							},
							create "TextButton" {
								Name = "EventTypeDropdown",
								Position = UDim2.fromOffset(78, 1),
								Size = UDim2.new(1, -78, 0, 24),
								AutoButtonColor = false,
								BorderSizePixel = 0,
								BackgroundColor3 = C_INPUT,
								Text = "",
								Activated = function() dropdownOpen(not dropdownOpen()) end,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 3) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = C_BORDER,
									},
									create "TextLabel" {
										Position = UDim2.fromOffset(8, 0),
										Size = UDim2.new(1, -24, 1, 0),
										BackgroundTransparency = 1,
										Font = Enum.Font.SourceSans,
										TextSize = 13,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = C_TEXT,
										Text = activeView,
									},
									create "TextLabel" {
										AnchorPoint = Vector2.new(1, 0.5),
										Position = UDim2.new(1, -7, 0.5, 0),
										Size = UDim2.fromOffset(12, 12),
										BackgroundTransparency = 1,
										Font = Enum.Font.SourceSansBold,
										TextSize = 11,
										TextColor3 = C_DIM,
										Text = function() return if dropdownOpen() then "▴" else "▾" end,
									},
								},
							},
						},
					},

					-- Inline dropdown choices
					show(dropdownOpen, function()
						return create "Frame" {
							Name = "EventTypeMenu",
							LayoutOrder = 2,
							Size = UDim2.new(1, 0, 0, #VIEW_OPTIONS * 22),
							BackgroundColor3 = C_INPUT,
							BorderSizePixel = 0,
							{
								create "UICorner" { CornerRadius = UDim.new(0, 3) },
								create "UIStroke" {
									ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
									Thickness = 1,
									Color = C_BORDER,
								},
								create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
								values(function() return VIEW_OPTIONS end, function(opt: ViewKind, idx: () -> number)
									return create "TextButton" {
										Name = "Opt_" .. opt,
										LayoutOrder = idx,
										Size = UDim2.new(1, 0, 0, 22),
										AutoButtonColor = false,
										BorderSizePixel = 0,
										Font = Enum.Font.SourceSans,
										TextSize = 13,
										TextXAlignment = Enum.TextXAlignment.Left,
										BackgroundColor3 = function()
											return if activeView() == opt then C_SELECTED() else C_INPUT()
										end,
										TextColor3 = C_TEXT,
										Text = "  " .. opt,
										Activated = function()
											activeView(opt)
											dropdownOpen(false)
										end,
									}
								end),
							},
						}
					end),

					-- NameBox
					LabeledInput("Name", "NameBox", eventName, 3),

					-- Active View body
					show(function() return activeView() == "Theme" end, function()
						return create "Frame" {
							Name = "ThemeView",
							LayoutOrder = 4,
							Size = UDim2.new(1, 0, 0, 30),
							BackgroundTransparency = 1,
							{
								create "UIListLayout" {
									FillDirection = Enum.FillDirection.Horizontal,
									SortOrder = Enum.SortOrder.LayoutOrder,
									Padding = UDim.new(0, 8),
								},
								RadioThemeBtn("Dark", 1),
								RadioThemeBtn("Light", 2),
							},
						}
					end),
					show(function() return activeView() == "RBXEvent" end, function()
						return create "Frame" {
							Name = "RBXEventView",
							LayoutOrder = 4,
							Size = UDim2.new(1, 0, 0, 90),
							BackgroundTransparency = 1,
							{
								create "UIListLayout" {
									FillDirection = Enum.FillDirection.Vertical,
									SortOrder = Enum.SortOrder.LayoutOrder,
									Padding = UDim.new(0, 6),
								},
								LabeledInput("Namespace", "RbxNamespaceInput", rbxNamespace, 1),
								LabeledInput("Detail", "RbxDetailInput", rbxDetail, 2),
								LabeledInput("DetailType", "RbxDetailTypeInput", rbxDetailType, 3),
							},
						}
					end),
					show(function() return activeView() == "MemStorage" end, function()
						return create "Frame" {
							Name = "MemStorageEventView",
							LayoutOrder = 4,
							Size = UDim2.new(1, 0, 0, 58),
							BackgroundTransparency = 1,
							{
								create "UIListLayout" {
									FillDirection = Enum.FillDirection.Vertical,
									SortOrder = Enum.SortOrder.LayoutOrder,
									Padding = UDim.new(0, 6),
								},
								LabeledInput("Key", "MemKeyInput", memKey, 1),
								LabeledInput("Value", "MemValInput", memVal, 2),
							},
						}
					end),

					-- ButtonArray: Save & Send (exact Studio Activators: OnSaveClicked / OnSendClicked)
					create "Frame" {
						Name = "ButtonArray",
						LayoutOrder = 5,
						Size = UDim2.new(1, 0, 0, 28),
						BackgroundTransparency = 1,
						{
							create "UIListLayout" {
								FillDirection = Enum.FillDirection.Horizontal,
								HorizontalAlignment = Enum.HorizontalAlignment.Center,
								SortOrder = Enum.SortOrder.LayoutOrder,
								Padding = UDim.new(0, 10),
							},
							create "TextButton" {
								Name = "SaveBtn",
								LayoutOrder = 1,
								Size = UDim2.fromOffset(86, 26),
								AutoButtonColor = false,
								BorderSizePixel = 0,
								BackgroundColor3 = C_BUTTON,
								Font = Enum.Font.SourceSans,
								TextSize = 13,
								TextColor3 = C_TEXT,
								Text = "Save",
								Activated = saveToHistory,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = C_BORDER,
									},
								},
							},
							create "TextButton" {
								Name = "SendBtn",
								LayoutOrder = 2,
								Size = UDim2.fromOffset(86, 26),
								AutoButtonColor = false,
								BorderSizePixel = 0,
								BackgroundColor3 = C_BLUE,
								Font = Enum.Font.SourceSansSemibold,
								TextSize = 13,
								TextColor3 = Color3.fromRGB(255, 255, 255),
								Text = "Send",
								Activated = sendCurrentEvent,
								{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
							},
						},
					},

					-- Operation status line (only takes height when non-empty)
					show(function() return statusMsg() ~= "" end, function()
						return create "TextLabel" {
							Name = "StatusText",
							LayoutOrder = 6,
							Size = UDim2.new(1, 0, 0, 14),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 11,
							TextXAlignment = Enum.TextXAlignment.Center,
							TextTruncate = Enum.TextTruncate.AtEnd,
							TextColor3 = C_DIM,
							Text = statusMsg,
						}
					end),
				},
			},

			-- Divider
			create "Frame" {
				LayoutOrder = 2,
				Size = UDim2.new(1, 0, 0, 1),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- 2. RepopulatableHistory (exact Studio centered Header + clickable HistoryItems)
			create "Frame" {
				Name = "RepopulatableHistory",
				LayoutOrder = 3,
				Size = UDim2.new(1, 0, 0, 0),
				AutomaticSize = Enum.AutomaticSize.Y,
				BackgroundTransparency = 1,
				{
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 4),
					},
					create "TextLabel" {
						Name = "Header",
						LayoutOrder = 0,
						Size = UDim2.new(1, 0, 0, 18),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 13,
						TextXAlignment = Enum.TextXAlignment.Center,
						TextColor3 = C_TEXT,
						Text = "History",
					},
					create "Frame" {
						Name = "ScrollingContainer",
						LayoutOrder = 1,
						Size = UDim2.new(1, 0, 0, 0),
						AutomaticSize = Enum.AutomaticSize.Y,
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						{
							create "UIListLayout" {
								FillDirection = Enum.FillDirection.Vertical,
								SortOrder = Enum.SortOrder.LayoutOrder,
								Padding = UDim.new(0, 4),
							},
							values(history, function(item: HistoryItem, idx: () -> number)
								local hovered = source(false)
								return create "TextButton" {
									Name = "HistoryItem_" .. tostring(item.id),
									LayoutOrder = idx,
									Size = UDim2.new(1, 0, 0, 28),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Text = "",
									BackgroundColor3 = function()
										return if hovered() then C_HOVER() else C_HEADER()
									end,
									MouseEnter = function() hovered(true) end,
									MouseLeave = function() hovered(false) end,
									Activated = function() repopulateFrom(item) end,
									{
										create "UICorner" { CornerRadius = UDim.new(0, 3) },
										create "UIStroke" {
											ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
											Thickness = 1,
											Color = C_BORDER,
										},
										create "TextLabel" {
											Position = UDim2.fromOffset(8, 0),
											Size = UDim2.new(0, 68, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 11,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_BLUE,
											Text = item.view,
										},
										create "TextLabel" {
											Position = UDim2.fromOffset(78, 0),
											Size = UDim2.new(1, -104, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = item.name,
										},
										create "TextButton" {
											AnchorPoint = Vector2.new(1, 0.5),
											Position = UDim2.new(1, -4, 0.5, 0),
											Size = UDim2.fromOffset(20, 20),
											BackgroundTransparency = 1,
											BorderSizePixel = 0,
											AutoButtonColor = false,
											Font = Enum.Font.SourceSansBold,
											TextSize = 13,
											TextColor3 = C_DIM,
											Text = "×",
											Activated = function() removeHistory(item.id) end,
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

local function syncMount()
	local isOpen = widget.Enabled
	toggleButton:SetActive(isOpen)
	if isOpen and not unmount then
		unmount = Vide.mount(EventEmulatorApp, widget)
	elseif not isOpen and unmount then
		local cleanup = unmount
		unmount = nil
		cleanup()
	end
end

toggleButton.Click:Connect(function()
	widget.Enabled = not widget.Enabled
	syncMount()
end)
widget:GetPropertyChangedSignal("Enabled"):Connect(syncMount)
syncMount()

plugin.Unloading:Connect(function()
	if unmount then
		local cleanup = unmount
		unmount = nil
		cleanup()
	end
	widget.Enabled = false
end)
