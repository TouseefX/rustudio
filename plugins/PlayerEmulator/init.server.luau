--!strict
--[[
    RusTudio Built-in Suite: Player Emulator (Luau / Vide)

    1:1 RE recreation of Roblox Studio's PlayerEmulator built-in plugin
    (MainView, MakeTheme, MainSwitchSection, LanguageSection,
    PseudolocalizationSection, CountryRegionSection,
    CustomPolicySwitchSection, PolicySection with ToggleItemModule &
    expandable ListItemsModule for AllowedExternalLinkReferences).
]]

local PlayerEmulatorService = game:GetService("PlayerEmulatorService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local action = Vide.action

local toolbar = plugin:CreateToolbar("player_emulator")
local toggleButton = toolbar:CreateButton(
	"player_emulator",
	"Emulate player language, pseudolocalization, country/region, and policies",
	"",
	"Player Emulator"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"PlayerEmulator",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Left, false, false, 320, 520, 260, 300)
)
widget.Title = "Player Emulator"
widget.Name = "RusTudioPlayerEmulator"

-- Exact Studio MakeTheme roles + MakeTheme Green = Color3.fromRGB(64, 166, 81)
local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SELECTED = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Selected)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)
local C_GREEN = Color3.fromRGB(64, 166, 81) -- Exact MakeTheme.luac.s Green
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)

type ChoiceItem = { code: string, label: string }

local LANGUAGES: { ChoiceItem } = {
	{ code = "en-us", label = "English (en-us)" },
	{ code = "es-es", label = "Spanish (es-es)" },
	{ code = "fr-fr", label = "French (fr-fr)" },
	{ code = "de-de", label = "German (de-de)" },
	{ code = "pt-br", label = "Portuguese (pt-br)" },
	{ code = "ja-jp", label = "Japanese (ja-jp)" },
	{ code = "ko-kr", label = "Korean (ko-kr)" },
	{ code = "zh-cn", label = "Chinese (zh-cn)" },
	{ code = "zh-tw", label = "Chinese Traditional (zh-tw)" },
	{ code = "ru-ru", label = "Russian (ru-ru)" },
	{ code = "it-it", label = "Italian (it-it)" },
}

local COUNTRIES: { ChoiceItem } = {
	{ code = "US", label = "United States (US)" },
	{ code = "GB", label = "United Kingdom (GB)" },
	{ code = "CA", label = "Canada (CA)" },
	{ code = "DE", label = "Germany (DE)" },
	{ code = "FR", label = "France (FR)" },
	{ code = "NL", label = "Netherlands (NL)" },
	{ code = "BE", label = "Belgium (BE)" },
	{ code = "BR", label = "Brazil (BR)" },
	{ code = "JP", label = "Japan (JP)" },
	{ code = "KR", label = "South Korea (KR)" },
	{ code = "AU", label = "Australia (AU)" },
	{ code = "CN", label = "China (CN)" },
	{ code = "MX", label = "Mexico (MX)" },
}

local POLICY_TOGGLES: { { key: string, defaultVal: boolean } } = {
	{ key = "AreAdsAllowed", defaultVal = true },
	{ key = "ArePaidRandomItemsRestricted", defaultVal = false },
	{ key = "IsContentSharingAllowed", defaultVal = true },
	{ key = "IsEligibleToPurchaseSubscription", defaultVal = true },
	{ key = "IsPaidItemTradingAllowed", defaultVal = true },
	{ key = "IsSubjectToChinaPolicies", defaultVal = false },
}

local EXTERNAL_LINKS = { "Discord", "YouTube", "Twitch", "Facebook", "Guilded" }

local function readProp(name: string, fallback: any): any
	local ok, v = pcall(function()
		return (PlayerEmulatorService :: any)[name]
	end)
	if ok and v ~= nil then
		if type(fallback) == "string" and v == "" then
			return fallback
		end
		return v
	end
	return fallback
end

local function writeProp(name: string, val: any)
	pcall(function()
		(PlayerEmulatorService :: any)[name] = val
	end)
end

local function readPolicyTable(): { [string]: any }
	local ok, t = pcall(function()
		return (PlayerEmulatorService :: any):GetEmulatedPolicyInfo()
	end)
	local out: { [string]: any } = {}
	if ok and type(t) == "table" then
		for k, v in pairs(t) do
			out[k] = v
		end
	end
	for _, def in ipairs(POLICY_TOGGLES) do
		if out[def.key] == nil then
			out[def.key] = def.defaultVal
		end
	end
	if type(out.AllowedExternalLinkReferences) ~= "table" then
		out.AllowedExternalLinkReferences = { "Discord", "YouTube", "Twitch", "Facebook", "Guilded" }
	end
	return out
end

local function writePolicyTable(t: { [string]: any })
	pcall(function()
		(PlayerEmulatorService :: any):SetEmulatedPolicyInfo(t)
	end)
end

local function lookupLabel(list: { ChoiceItem }, code: string): string
	for _, it in ipairs(list) do
		if string.lower(it.code) == string.lower(code) then
			return it.label
		end
	end
	return code
end

-- Studio pill toggle (exact MakeTheme Green = 64,166,81 when ON)
local function StudioSwitch(name: string, isOn: () -> boolean, isEnabled: () -> boolean, onClick: () -> ())
	return create "TextButton" {
		Name = name,
		AnchorPoint = Vector2.new(1, 0.5),
		Position = UDim2.new(1, 0, 0.5, 0),
		Size = UDim2.fromOffset(36, 18),
		AutoButtonColor = false,
		BorderSizePixel = 0,
		Text = "",
		BackgroundColor3 = function()
			if not isEnabled() then
				return Color3.fromRGB(54, 56, 62)
			end
			return if isOn() then C_GREEN else Color3.fromRGB(84, 86, 94)
		end,
		Activated = function()
			if isEnabled() then
				onClick()
			end
		end,
		{
			create "UICorner" { CornerRadius = UDim.new(1, 0) },
			create "Frame" {
				AnchorPoint = Vector2.new(0, 0.5),
				Position = function()
					return if isOn() then UDim2.new(1, -16, 0.5, 0) else UDim2.new(0, 2, 0.5, 0)
				end,
				Size = UDim2.fromOffset(14, 14),
				BackgroundColor3 = function()
					return if isEnabled() then Color3.fromRGB(255, 255, 255) else Color3.fromRGB(150, 152, 158)
				end,
				BorderSizePixel = 0,
				{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
			},
		},
	}
end

local function PlayerEmulatorApp()
	local mainOn = source(readProp("PlayerEmulationEnabled", false) == true)
	local locale = source(tostring(readProp("EmulatedGameLocale", "en-us")))
	local pseudoOn = source(readProp("PseudolocalizationEnabled", false) == true)
	local elongationPct = source(math.floor((tonumber(readProp("TextElongationFactor", 0)) or 0) * 100 + 0.5))
	local country = source(tostring(readProp("EmulatedCountryCode", "US")))
	local customPolicyOn = source(readProp("CustomPoliciesEnabled", false) == true)
	local policies = source(readPolicyTable())
	local linksExpanded = source(false)
	local openMenu = source(nil :: ("Language" | "Country")?)

	local elongRef: TextBox? = nil

	local function setLocale(code: string)
		locale(code)
		writeProp("EmulatedGameLocale", code)
		openMenu(nil)
	end

	local function setCountry(code: string)
		country(code)
		writeProp("EmulatedCountryCode", code)
		openMenu(nil)
	end

	local function togglePolicyKey(key: string)
		local nextT = {}
		for k, v in pairs(policies()) do
			nextT[k] = v
		end
		nextT[key] = not (nextT[key] == true)
		policies(nextT)
		writePolicyTable(nextT)
	end

	local function hasLink(linkName: string): boolean
		local list = policies().AllowedExternalLinkReferences
		if type(list) ~= "table" then
			return false
		end
		for _, v in ipairs(list) do
			if v == linkName then
				return true
			end
		end
		return false
	end

	local function toggleLink(linkName: string)
		local nextT = {}
		for k, v in pairs(policies()) do
			nextT[k] = v
		end
		local nextLinks = {}
		local existed = false
		if type(nextT.AllowedExternalLinkReferences) == "table" then
			for _, v in ipairs(nextT.AllowedExternalLinkReferences) do
				if v == linkName then
					existed = true
				else
					table.insert(nextLinks, v)
				end
			end
		end
		if not existed then
			table.insert(nextLinks, linkName)
		end
		nextT.AllowedExternalLinkReferences = nextLinks
		policies(nextT)
		writePolicyTable(nextT)
	end

	local function DropdownRow(
		sectionName: string,
		label: string,
		menuKey: "Language" | "Country",
		items: { ChoiceItem },
		curCode: () -> string,
		onPick: (string) -> (),
		order: number
	)
		local isOpen = function()
			return openMenu() == menuKey
		end
		return create "Frame" {
			Name = sectionName,
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 0),
			AutomaticSize = Enum.AutomaticSize.Y,
			BackgroundTransparency = 1,
			{
				create "UIListLayout" {
					FillDirection = Enum.FillDirection.Vertical,
					SortOrder = Enum.SortOrder.LayoutOrder,
					Padding = UDim.new(0, 4),
				},
				-- 25px flat row: Left Label + Right responsive DropdownButton (exact MakeTheme)
				create "Frame" {
					LayoutOrder = 1,
					Size = UDim2.new(1, 0, 0, 26),
					BackgroundTransparency = 1,
					{
						create "TextLabel" {
							Size = UDim2.new(0, 88, 1, 0),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 14,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextColor3 = function()
								return if mainOn() then C_TEXT() else C_DIM()
							end,
							Text = label,
						},
						create "TextButton" {
							Name = menuKey .. "DropdownBtn",
							AnchorPoint = Vector2.new(1, 0.5),
							Position = UDim2.new(1, 0, 0.5, 0),
							Size = UDim2.new(1, -94, 0, 25),
							AutoButtonColor = false,
							BorderSizePixel = 0,
							BackgroundColor3 = C_INPUT,
							Text = "",
							Activated = function()
								if isOpen() then
									openMenu(nil)
								else
									openMenu(menuKey)
								end
							end,
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
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = function()
										return if mainOn() then C_TEXT() else C_DIM()
									end,
									Text = function() return lookupLabel(items, curCode()) end,
								},
								create "TextLabel" {
									AnchorPoint = Vector2.new(1, 0.5),
									Position = UDim2.new(1, -7, 0.5, 0),
									Size = UDim2.fromOffset(12, 12),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSansBold,
									TextSize = 11,
									TextColor3 = C_DIM,
									Text = function() return if isOpen() then "▴" else "▾" end,
								},
							},
						},
					},
				},
				-- Inline dropdown list directly below button
				show(isOpen, function()
					return create "ScrollingFrame" {
						Name = menuKey .. "MenuList",
						LayoutOrder = 2,
						Size = UDim2.new(1, 0, 0, 140),
						BackgroundColor3 = C_INPUT,
						BorderSizePixel = 0,
						ScrollBarThickness = 5,
						ScrollBarImageColor3 = C_SCROLL,
						CanvasSize = UDim2.fromOffset(0, 0),
						AutomaticCanvasSize = Enum.AutomaticSize.Y,
						{
							create "UICorner" { CornerRadius = UDim.new(0, 3) },
							create "UIStroke" {
								ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
								Thickness = 1,
								Color = C_BORDER,
							},
							create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
							values(function() return items end, function(it: ChoiceItem, idx: () -> number)
								local hovered = source(false)
								return create "TextButton" {
									Name = menuKey .. "_" .. it.code,
									LayoutOrder = idx,
									Size = UDim2.new(1, 0, 0, 22),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Text = "",
									BackgroundColor3 = function()
										if string.lower(curCode()) == string.lower(it.code) then
											return C_SELECTED()
										elseif hovered() then
											return C_HOVER()
										end
										return C_INPUT()
									end,
									MouseEnter = function() hovered(true) end,
									MouseLeave = function() hovered(false) end,
									Activated = function() onPick(it.code) end,
									{
										create "TextLabel" {
											Position = UDim2.fromOffset(8, 0),
											Size = UDim2.new(1, -12, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 13,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = it.label,
										},
									},
								}
							end),
						},
					}
				end),
			},
		}
	end

	return create "ScrollingFrame" {
		Name = "MainView",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		CanvasSize = UDim2.fromOffset(0, 0),
		AutomaticCanvasSize = Enum.AutomaticSize.Y,
		ScrollBarThickness = 6,
		ScrollBarImageColor3 = C_SCROLL,
		{
			create "UIPadding" {
				PaddingTop = UDim.new(0, 15), -- Exact MAINVIEW_PADDING_TOP
				PaddingLeft = UDim.new(0, 10), -- Exact MAINVIEW_PADDING_LEFT
				PaddingRight = UDim.new(0, 10),
				PaddingBottom = UDim.new(0, 15),
			},
			create "UIListLayout" {
				FillDirection = Enum.FillDirection.Vertical,
				SortOrder = Enum.SortOrder.LayoutOrder,
				Padding = UDim.new(0, 10),
			},

			-- 1. MainSwitchSection (exact Studio MainView order)
			create "Frame" {
				Name = "MainSwitchSection",
				LayoutOrder = 1,
				Size = UDim2.new(1, 0, 0, 25),
				BackgroundTransparency = 1,
				{
					create "TextLabel" {
						Size = UDim2.new(1, -44, 1, 0),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 14,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_TEXT,
						Text = "Test in Studio",
					},
					StudioSwitch("MainSwitchToggle", mainOn, function() return true end, function()
						local nv = not mainOn()
						mainOn(nv)
						writeProp("PlayerEmulationEnabled", nv)
					end),
				},
			},

			-- 2. Separator
			create "Frame" {
				Name = "Separator",
				LayoutOrder = 2,
				Size = UDim2.new(1, 0, 0, 1),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- 3. LanguageSection + InstructionText
			DropdownRow("LanguageSection", "Language", "Language", LANGUAGES, locale, setLocale, 3),
			create "TextLabel" {
				Name = "LanguageInstructionText",
				LayoutOrder = 4,
				Size = UDim2.new(1, 0, 0, 16),
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSans,
				TextSize = 12,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_DIM,
				Text = "Emulate player language during playtesting",
			},

			-- 4. PseudolocalizationSection (exact Studio toggle + 0-100% elongation)
			create "Frame" {
				Name = "PseudolocalizationSection",
				LayoutOrder = 5,
				Size = UDim2.new(1, 0, 0, 56),
				BackgroundTransparency = 1,
				{
					create "Frame" {
						Size = UDim2.new(1, 0, 0, 25),
						BackgroundTransparency = 1,
						{
							create "TextLabel" {
								Size = UDim2.new(1, -44, 1, 0),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 14,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = function() return if mainOn() then C_TEXT() else C_DIM() end,
								Text = "Pseudolocalization",
							},
							StudioSwitch("PseudoToggle", pseudoOn, mainOn, function()
								local nv = not pseudoOn()
								pseudoOn(nv)
								writeProp("PseudolocalizationEnabled", nv)
							end),
						},
					},
					create "Frame" {
						Position = UDim2.fromOffset(0, 30),
						Size = UDim2.new(1, 0, 0, 24),
						BackgroundTransparency = 1,
						{
							create "TextLabel" {
								Size = UDim2.new(1, -94, 1, 0),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = function() return if mainOn() and pseudoOn() then C_TEXT() else C_DIM() end,
								Text = "Text Elongation (%)",
							},
							create "Frame" {
								AnchorPoint = Vector2.new(1, 0.5),
								Position = UDim2.new(1, 0, 0.5, 0),
								Size = UDim2.fromOffset(84, 24),
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
										Name = "ElongationInput",
										Position = UDim2.fromOffset(6, 0),
										Size = UDim2.new(1, -10, 1, 0),
										BackgroundTransparency = 1,
										ClearTextOnFocus = false,
										Font = Enum.Font.SourceSans,
										TextSize = 13,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = C_TEXT,
										Text = tostring(elongationPct()),
										FocusLost = function()
											if not elongRef then
												return
											end
											local n = tonumber(elongRef.Text)
											if n then
												local cl = math.clamp(math.floor(n + 0.5), 0, 100)
												elongationPct(cl)
												elongRef.Text = tostring(cl)
												writeProp("TextElongationFactor", cl / 100)
											else
												elongRef.Text = tostring(elongationPct())
											end
										end,
										action(function(inst) elongRef = inst :: TextBox end),
									},
								},
							},
						},
					},
				},
			},

			-- 5. CountryRegionSection (exact Studio dropdown "%s (%s)")
			DropdownRow("CountryRegionSection", "Country/Region", "Country", COUNTRIES, country, setCountry, 6),

			create "Frame" {
				LayoutOrder = 7,
				Size = UDim2.new(1, 0, 0, 1),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- 6. CustomPolicySwitchSection
			create "Frame" {
				Name = "CustomPolicySwitchSection",
				LayoutOrder = 8,
				Size = UDim2.new(1, 0, 0, 25),
				BackgroundTransparency = 1,
				{
					create "TextLabel" {
						Size = UDim2.new(1, -44, 1, 0),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 14,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = function() return if mainOn() then C_TEXT() else C_DIM() end,
						Text = "Custom Policies",
					},
					StudioSwitch("CustomPoliciesToggle", customPolicyOn, mainOn, function()
						local nv = not customPolicyOn()
						customPolicyOn(nv)
						writeProp("CustomPoliciesEnabled", nv)
					end),
				},
			},

			-- 7. PolicySection (exact Studio ToggleItemModule rows + expandable ListItemsModule)
			create "Frame" {
				Name = "PolicySection",
				LayoutOrder = 9,
				Size = UDim2.new(1, 0, 0, 0),
				AutomaticSize = Enum.AutomaticSize.Y,
				BackgroundTransparency = 1,
				{
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 6),
					},
					values(function() return POLICY_TOGGLES end, function(def, idx: () -> number)
						local active = function()
							return mainOn() and customPolicyOn()
						end
						return create "Frame" {
							Name = "Policy_" .. def.key,
							LayoutOrder = idx,
							Size = UDim2.new(1, 0, 0, 22),
							BackgroundTransparency = 1,
							{
								create "TextLabel" {
									Position = UDim2.fromOffset(4, 0), -- Exact TEXT_INDENT_PADDING
									Size = UDim2.new(1, -48, 1, 0),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = function() return if active() then C_TEXT() else C_DIM() end,
									Text = def.key,
								},
								StudioSwitch("Toggle_" .. def.key, function()
									return policies()[def.key] == true
								end, active, function()
									togglePolicyKey(def.key)
								end),
							},
						}
					end),

					-- Expandable AllowedExternalLinkReferences (exact Studio ListItemsModule)
					create "TextButton" {
						Name = "ExternalLinksHeader",
						LayoutOrder = 100,
						Size = UDim2.new(1, 0, 0, 22),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						AutoButtonColor = false,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = function() return if mainOn() and customPolicyOn() then C_TEXT() else C_DIM() end,
						Text = function()
							return (if linksExpanded() then "▾ " else "▸ ") .. "AllowedExternalLinkReferences"
						end,
						Activated = function() linksExpanded(not linksExpanded()) end,
					},
					show(linksExpanded, function()
						return create "Frame" {
							Name = "ExternalLinksList",
							LayoutOrder = 101,
							Size = UDim2.new(1, 0, 0, #EXTERNAL_LINKS * 22),
							BackgroundTransparency = 1,
							{
								create "UIPadding" { PaddingLeft = UDim.new(0, 16) },
								create "UIListLayout" {
									FillDirection = Enum.FillDirection.Vertical,
									SortOrder = Enum.SortOrder.LayoutOrder,
									Padding = UDim.new(0, 2),
								},
								values(function() return EXTERNAL_LINKS end, function(linkName: string, idx: () -> number)
									local active = function() return mainOn() and customPolicyOn() end
									local checked = function() return hasLink(linkName) end
									return create "TextButton" {
										Name = "ExtLink_" .. linkName,
										LayoutOrder = idx,
										Size = UDim2.new(1, 0, 0, 20),
										BackgroundTransparency = 1,
										BorderSizePixel = 0,
										AutoButtonColor = false,
										Text = "",
										Activated = function()
											if active() then
												toggleLink(linkName)
											end
										end,
										{
											create "Frame" {
												Position = UDim2.new(0, 0, 0.5, -7),
												Size = UDim2.fromOffset(14, 14),
												BackgroundColor3 = function()
													if not active() then return C_BUTTON() end
													return if checked() then C_ACCENT() else C_INPUT()
												end,
												BorderSizePixel = 0,
												{
													create "UICorner" { CornerRadius = UDim.new(0, 3) },
													create "UIStroke" {
														ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
														Thickness = 1,
														Color = C_BORDER,
													},
													create "TextLabel" {
														Size = UDim2.fromScale(1, 1),
														BackgroundTransparency = 1,
														Font = Enum.Font.SourceSansBold,
														TextSize = 11,
														TextColor3 = Color3.fromRGB(255, 255, 255),
														Text = function() return if checked() then "✔" else "" end,
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
												TextColor3 = function() return if active() then C_TEXT() else C_DIM() end,
												Text = linkName,
											},
										},
									}
								end),
							},
						}
					end),
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
		unmount = Vide.mount(PlayerEmulatorApp, widget)
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
