--!strict
--[[
    RusTudio Built-in Suite: Force Visualization Control (Luau / Vide)

    1:1 RE recreation of Roblox Studio's ForceVisualizationControl built-in
    plugin (ForceVisualizationControl/Src/Components/MainGui.luac.s):
      - Renders as a compact 220px-wide CoreGui ScreenGui overlay panel
        at Position (50, 50) (matching MainGui.luac.s), draggable by header.
      - Exact Studio sections, order, labels, RGB vector legend swatches,
        and slider ranges:
          Constraints:  Forces (0,1,0.5) · Torque (0.612,0.169,1) · Net
          Contacts:     Forces (1,0.5,0 + 1,0,0) · Net
          Gravity:      Forces (0,0.5,1)
          Fluid forces: Forces (0,0,1)
          Net force:    Enabled
          Scaling:      Show Magnitudes · Show Instance Names
                        Forces Draw Scale [0..3]
                        Fluid Forces Draw Scale [0..10]
                        Torque Draw Scale [0..10]
          Smoothing:    Enabled · Frames # [1..32]
]]

local CoreGui = game:GetService("CoreGui")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local action = Vide.action

local toolbar = plugin:CreateToolbar("force_visualization")
local toggleButton = toolbar:CreateButton(
	"force_visualization",
	"Configure physics force, torque, contact, gravity, and fluid visualization overlays",
	"",
	"Force Visualization"
)
toggleButton.ClickableWhenViewportHidden = true

local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)

local PANEL_BG = Color3.fromRGB(46, 48, 54)
local TRACK_BG = Color3.fromRGB(72, 75, 84)

local function getPhysicsSettings(): any?
	local ok, ps = pcall(function()
		return settings().Physics
	end)
	return if ok then ps else nil
end

local function readPhys(prop: string, fallback: any): any
	local ps = getPhysicsSettings()
	if not ps then
		return fallback
	end
	local ok, v = pcall(function()
		return ps[prop]
	end)
	if ok and v ~= nil then
		return v
	end
	return fallback
end

local function writePhys(prop: string, val: any)
	local ps = getPhysicsSettings()
	if not ps then
		return
	end
	pcall(function()
		ps[prop] = val
	end)
end

local enabled = false
local unmount: (() -> ())? = nil
local closeHud: () -> () = function() end

local function ForceVisOverlay()
	local panelPos = source(UDim2.fromOffset(16, 16))
	local dragging = false
	local dragStart = Vector2.zero
	local startOff = Vector2.new(16, 16)

	local function SectionHeader(title: string, order: number)
		return create "TextLabel" {
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 14),
			BackgroundTransparency = 1,
			Font = Enum.Font.SourceSansBold,
			TextSize = 12,
			TextXAlignment = Enum.TextXAlignment.Left,
			TextColor3 = C_TEXT,
			Text = title,
		}
	end

	local function PhysCheck(prop: string, label: string, swatches: { Color3 }?, order: number)
		local sig = source(readPhys(prop, false) == true)
		local swatchEls = {}
		if swatches then
			for i, col in ipairs(swatches) do
				table.insert(
					swatchEls,
					create "Frame" {
						AnchorPoint = Vector2.new(1, 0.5),
						Position = UDim2.new(1, -((#swatches - i) * 14), 0.5, 0),
						Size = UDim2.fromOffset(10, 10),
						BackgroundColor3 = col,
						BorderSizePixel = 0,
						{ create "UICorner" { CornerRadius = UDim.new(0, 2) } },
					}
				)
			end
		end
		return create "TextButton" {
			Name = "Toggle_" .. prop,
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 16),
			BackgroundTransparency = 1,
			BorderSizePixel = 0,
			AutoButtonColor = false,
			Text = "",
			Activated = function()
				local nv = not sig()
				sig(nv)
				writePhys(prop, nv)
			end,
			{
				create "Frame" {
					Position = UDim2.new(0, 2, 0.5, -6),
					Size = UDim2.fromOffset(12, 12),
					BackgroundColor3 = function()
						return if sig() then C_ACCENT() else Color3.fromRGB(34, 36, 42)
					end,
					BorderSizePixel = 0,
					{
						create "UICorner" { CornerRadius = UDim.new(0, 2) },
						create "UIStroke" {
							ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
							Thickness = 1,
							Color = function() return if sig() then C_ACCENT() else C_BORDER() end,
						},
						create "TextLabel" {
							Size = UDim2.fromScale(1, 1),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSansBold,
							TextSize = 10,
							TextColor3 = Color3.fromRGB(255, 255, 255),
							Text = function() return if sig() then "✔" else "" end,
						},
					},
				},
				create "TextLabel" {
					Position = UDim2.fromOffset(20, 0),
					Size = UDim2.new(1, -50, 1, 0),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSans,
					TextSize = 12,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextTruncate = Enum.TextTruncate.AtEnd,
					TextColor3 = C_TEXT,
					Text = label,
				},
				swatchEls,
			},
		}
	end

	-- Exact Studio MainGui 200x15 slider row with live drag & numeric readout
	local function PhysSlider(prop: string, label: string, minV: number, maxV: number, isInt: boolean, defaultV: number, order: number)
		local val = source(math.clamp(tonumber(readPhys(prop, defaultV)) or defaultV, minV, maxV))
		local trackRef: Frame? = nil
		local sliding = false

		local function applyX(px: number)
			if not trackRef then
				return
			end
			local absPos = trackRef.AbsolutePosition
			local absSz = trackRef.AbsoluteSize
			if absSz.X <= 1 then
				return
			end
			local t = math.clamp((px - absPos.X) / absSz.X, 0, 1)
			local v = minV + (maxV - minV) * t
			if isInt then
				v = math.floor(v + 0.5)
			else
				v = math.floor(v * 100 + 0.5) / 100
			end
			val(v)
			writePhys(prop, v)
		end

		return create "Frame" {
			Name = "Slider_" .. prop,
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 27),
			BackgroundTransparency = 1,
			{
				create "TextLabel" {
					Size = UDim2.new(0.74, 0, 0, 13),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSans,
					TextSize = 11,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_DIM,
					Text = label,
				},
				create "TextLabel" {
					Position = UDim2.fromScale(0.74, 0),
					Size = UDim2.new(0.26, 0, 0, 13),
					BackgroundTransparency = 1,
					Font = Enum.Font.Code,
					TextSize = 11,
					TextXAlignment = Enum.TextXAlignment.Right,
					TextColor3 = C_TEXT,
					Text = function()
						local v = val()
						return if isInt then tostring(v) else string.format("%.2f", v)
					end,
				},
				create "TextButton" {
					Position = UDim2.fromOffset(0, 14),
					Size = UDim2.new(1, 0, 0, 13),
					BackgroundTransparency = 1,
					BorderSizePixel = 0,
					AutoButtonColor = false,
					Text = "",
					InputBegan = function(input: InputObject)
						if input.UserInputType == Enum.UserInputType.MouseButton1 then
							sliding = true
							applyX(input.Position.X)
						end
					end,
					InputChanged = function(input: InputObject)
						if sliding and input.UserInputType == Enum.UserInputType.MouseMovement then
							applyX(input.Position.X)
						end
					end,
					InputEnded = function(input: InputObject)
						if input.UserInputType == Enum.UserInputType.MouseButton1 then
							sliding = false
						end
					end,
					action(function(inst) trackRef = inst :: any end),
					{
						create "Frame" {
							AnchorPoint = Vector2.new(0, 0.5),
							Position = UDim2.fromScale(0, 0.5),
							Size = UDim2.new(1, 0, 0, 4),
							BackgroundColor3 = TRACK_BG,
							BorderSizePixel = 0,
							{
								create "UICorner" { CornerRadius = UDim.new(1, 0) },
								create "Frame" {
									Size = function()
										local t = math.clamp((val() - minV) / math.max(1e-6, maxV - minV), 0, 1)
										return UDim2.fromScale(t, 1)
									end,
									BackgroundColor3 = C_ACCENT,
									BorderSizePixel = 0,
									{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
								},
							},
						},
						create "Frame" {
							AnchorPoint = Vector2.new(0.5, 0.5),
							Position = function()
								local t = math.clamp((val() - minV) / math.max(1e-6, maxV - minV), 0, 1)
								return UDim2.fromScale(t, 0.5)
							end,
							Size = UDim2.fromOffset(10, 10),
							BackgroundColor3 = Color3.fromRGB(235, 237, 242),
							BorderSizePixel = 0,
							{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
						},
					},
				},
			},
		}
	end

	return create "ScreenGui" {
		Name = "RusTudioForceVisualization",
		DisplayOrder = 14,
		ResetOnSpawn = false,
		ZIndexBehavior = Enum.ZIndexBehavior.Sibling,
		{
			create "Frame" {
				Name = "MainPane",
				Position = panelPos,
				Size = UDim2.fromOffset(220, 0), -- Exact MainGui.luac.s 220px width
				AutomaticSize = Enum.AutomaticSize.Y,
				BackgroundColor3 = PANEL_BG,
				BackgroundTransparency = 0.08,
				BorderSizePixel = 0,
				{
					create "UICorner" { CornerRadius = UDim.new(0, 6) },
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = C_BORDER,
					},
					create "UIPadding" {
						PaddingLeft = UDim.new(0, 10),
						PaddingRight = UDim.new(0, 10),
						PaddingTop = UDim.new(0, 6),
						PaddingBottom = UDim.new(0, 8),
					},
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 2),
					},

					-- Draggable Title strip
					create "TextButton" {
						Name = "DragHeader",
						LayoutOrder = 0,
						Size = UDim2.new(1, 0, 0, 18),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						AutoButtonColor = false,
						Text = "",
						InputBegan = function(input: InputObject)
							if input.UserInputType == Enum.UserInputType.MouseButton1 then
								dragging = true
								dragStart = Vector2.new(input.Position.X, input.Position.Y)
								local cur = panelPos()
								startOff = Vector2.new(cur.X.Offset, cur.Y.Offset)
							end
						end,
						InputChanged = function(input: InputObject)
							if dragging and input.UserInputType == Enum.UserInputType.MouseMovement then
								local d = Vector2.new(input.Position.X, input.Position.Y) - dragStart
								panelPos(UDim2.fromOffset(math.max(8, startOff.X + d.X), math.max(8, startOff.Y + d.Y)))
							end
						end,
						InputEnded = function(input: InputObject)
							if input.UserInputType == Enum.UserInputType.MouseButton1 then
								dragging = false
							end
						end,
						{
							create "TextLabel" {
								Size = UDim2.new(1, -20, 1, 0),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSansBold,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = C_TEXT,
								Text = "Force Visualization",
							},
							create "TextButton" {
								Name = "CloseHudBtn",
								AnchorPoint = Vector2.new(1, 0.5),
								Position = UDim2.new(1, 0, 0.5, 0),
								Size = UDim2.fromOffset(18, 18),
								BackgroundTransparency = 1,
								BorderSizePixel = 0,
								AutoButtonColor = false,
								Font = Enum.Font.SourceSansBold,
								TextSize = 13,
								TextColor3 = C_DIM,
								Text = "×",
								Activated = function()
									closeHud()
								end,
							},
						},
					},

					-- Exact MainGui.luac.s section order, labels, RGB swatches & ranges
					SectionHeader("Constraints", 1),
					PhysCheck("AreConstraintForcesShownForSelectedOrHoveredInstances", "Forces", { Color3.new(0, 1, 0.5) }, 2),
					PhysCheck("AreConstraintTorquesShownForSelectedOrHoveredInstances", "Torque", { Color3.new(0.612, 0.169, 1) }, 3),
					PhysCheck("DrawConstraintsNetForce", "Net", nil, 4),

					SectionHeader("Contacts", 5),
					PhysCheck("AreContactForcesShownForSelectedOrHoveredAssemblies", "Forces", { Color3.new(1, 0.5, 0), Color3.new(1, 0, 0) }, 6),
					PhysCheck("DrawContactsNetForce", "Net", nil, 7),

					SectionHeader("Gravity", 8),
					PhysCheck("AreGravityForcesShownForSelectedOrHoveredAssemblies", "Forces", { Color3.new(0, 0.5, 1) }, 9),

					SectionHeader("Fluid forces", 10),
					PhysCheck("ShowFluidForcesForSelectedOrHoveredMechanisms", "Forces", { Color3.new(0, 0, 1) }, 11),

					SectionHeader("Net force", 12),
					PhysCheck("DrawTotalNetForce", "Enabled", nil, 13),

					SectionHeader("Scaling", 14),
					PhysCheck("AreMagnitudesShownForDrawnForcesAndTorques", "Show Magnitudes", nil, 15),
					PhysCheck("ShowInstanceNamesForDrawnForcesAndTorques", "Show Instance Names", nil, 16),
					PhysSlider("ForceDrawScale", "Forces Draw Scale", 1e-7, 3, false, 1.0, 17),
					PhysSlider("FluidForceDrawScale", "Fluid Forces Draw Scale", 1e-7, 10, false, 1.0, 18),
					PhysSlider("TorqueDrawScale", "Torque Draw Scale", 1e-7, 10, false, 1.0, 19),

					SectionHeader("Smoothing", 20),
					PhysCheck("EnableForceVisualizationSmoothing", "Enabled", nil, 21),
					PhysSlider("ForceVisualizationSmoothingSteps", "Frames #", 1, 32, true, 1, 22),
				},
			},
		},
	}
end

local function teardown()
	if unmount then
		local fn = unmount
		unmount = nil
		fn()
	end
	local stale = CoreGui:FindFirstChild("RusTudioForceVisualization")
	if stale then
		stale:Destroy()
	end
end

local function syncHud()
	toggleButton:SetActive(enabled)
	if enabled then
		if not unmount then
			local stale = CoreGui:FindFirstChild("RusTudioForceVisualization")
			if stale then
				stale:Destroy()
			end
			unmount = Vide.mount(ForceVisOverlay, CoreGui)
		end
	else
		teardown()
	end
end

closeHud = function()
	enabled = false
	syncHud()
end

toggleButton.Click:Connect(function()
	enabled = not enabled
	syncHud()
end)

plugin.Unloading:Connect(function()
	enabled = false
	teardown()
end)

syncHud()
