--!strict
--[[
    RusTudio Built-in Suite: Alignment Tool (Luau / Vide)
    1:1 recreation of Roblox Studio's AlignmentTool.rbxm UI & behavior.
]]

local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source

local ICON_BASE = "rbxasset://studio_svg_textures/Lua/AlignTool/Dark/Large/"

local toolbar = plugin:CreateToolbar("align")
local toggleButton = toolbar:CreateButton(
	"align",
	"Align selected parts and models along X, Y, or Z axes",
	"",
	"Align Tool"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"AlignmentTool",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Left, false, false, 300, 250, 175, 250)
)
widget.Title = "Align Tool"
widget.Name = "RusTudioAlignmentTool"

-- Studio theme roles (matching AlignmentTool.Src.Resources.MakeTheme)
local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BTN = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_BTN_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.Button, Enum.StudioStyleGuideModifier.Hover)
local C_BTN_DIS = StudioTheme.color(Enum.StudioStyleGuideColor.Button, Enum.StudioStyleGuideModifier.Disabled)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)

local MODE_ACTIVE_BG = Color3.fromRGB(106, 109, 118)

type AlignMode = "Min" | "Center" | "Max"
type RelativeTo = "Selection" | "Active"
type CoordSpace = "World" | "Local"

local CORNERS = {
	Vector3.new(-0.5, -0.5, -0.5),
	Vector3.new(0.5, -0.5, -0.5),
	Vector3.new(-0.5, 0.5, -0.5),
	Vector3.new(0.5, 0.5, -0.5),
	Vector3.new(-0.5, -0.5, 0.5),
	Vector3.new(0.5, -0.5, 0.5),
	Vector3.new(-0.5, 0.5, 0.5),
	Vector3.new(0.5, 0.5, 0.5),
}

local function getAlignableObjects(): { Instance }
	local out = {}
	for _, inst in ipairs(Selection:Get()) do
		if inst:IsA("BasePart") or inst:IsA("Model") then
			table.insert(out, inst)
		end
	end
	return out
end

local function objectBoxInBasis(inst: Instance, basis: CFrame): (Vector3?, Vector3?)
	local cf: CFrame
	local sz: Vector3
	if inst:IsA("BasePart") then
		cf = inst.CFrame
		sz = inst.Size
	elseif inst:IsA("Model") then
		local ok, mcf, msz = pcall(function()
			return inst:GetBoundingBox()
		end)
		if not ok or not mcf or not msz or msz.Magnitude == 0 then
			return nil, nil
		end
		cf = mcf
		sz = msz
	else
		return nil, nil
	end

	local rel = basis:ToObjectSpace(cf)
	local minV = Vector3.new(math.huge, math.huge, math.huge)
	local maxV = Vector3.new(-math.huge, -math.huge, -math.huge)
	for _, c in ipairs(CORNERS) do
		local p = rel:PointToWorldSpace(c * sz)
		minV = Vector3.new(math.min(minV.X, p.X), math.min(minV.Y, p.Y), math.min(minV.Z, p.Z))
		maxV = Vector3.new(math.max(maxV.X, p.X), math.max(maxV.Y, p.Y), math.max(maxV.Z, p.Z))
	end
	return minV, maxV
end

local function translateObject(inst: Instance, worldDelta: Vector3)
	if worldDelta.Magnitude < 1e-6 then
		return
	end
	if inst:IsA("BasePart") then
		inst.CFrame = inst.CFrame + worldDelta
	elseif inst:IsA("Model") then
		local ok = pcall(function()
			inst:PivotTo(inst:GetPivot() + worldDelta)
		end)
		if not ok then
			for _, d in ipairs(inst:GetDescendants()) do
				if d:IsA("BasePart") then
					d.CFrame = d.CFrame + worldDelta
				end
			end
		end
	end
end

local function runAlign(
	mode: AlignMode,
	axes: { X: boolean, Y: boolean, Z: boolean },
	space: CoordSpace,
	relTo: RelativeTo
): boolean
	if not (axes.X or axes.Y or axes.Z) then
		return false
	end
	local objs = getAlignableObjects()
	if #objs < 2 then
		return false
	end

	local basis = CFrame.identity
	if space == "Local" then
		local first = objs[1]
		if first:IsA("BasePart") then
			basis = first.CFrame.Rotation
		elseif first:IsA("Model") then
			local ok, mcf = pcall(function()
				return first:GetBoundingBox()
			end)
			if ok and mcf then
				basis = mcf.Rotation
			end
		end
	end

	local boxes: { { inst: Instance, minV: Vector3, maxV: Vector3 } } = {}
	local totalMin = Vector3.new(math.huge, math.huge, math.huge)
	local totalMax = Vector3.new(-math.huge, -math.huge, -math.huge)
	for _, inst in ipairs(objs) do
		local mn, mx = objectBoxInBasis(inst, basis)
		if mn and mx then
			table.insert(boxes, { inst = inst, minV = mn, maxV = mx })
			totalMin = Vector3.new(math.min(totalMin.X, mn.X), math.min(totalMin.Y, mn.Y), math.min(totalMin.Z, mn.Z))
			totalMax = Vector3.new(math.max(totalMax.X, mx.X), math.max(totalMax.Y, mx.Y), math.max(totalMax.Z, mx.Z))
		end
	end
	if #boxes < 2 then
		return false
	end

	local refMin = if relTo == "Active" then boxes[1].minV else totalMin
	local refMax = if relTo == "Active" then boxes[1].maxV else totalMax

	local function targetCoord(mn: number, mx: number): number
		if mode == "Min" then
			return mn
		elseif mode == "Max" then
			return mx
		else
			return (mn + mx) * 0.5
		end
	end

	local target = Vector3.new(
		targetCoord(refMin.X, refMax.X),
		targetCoord(refMin.Y, refMax.Y),
		targetCoord(refMin.Z, refMax.Z)
	)

	local rec = nil
	pcall(function()
		rec = ChangeHistoryService:TryBeginRecording("AlignTool", "Align Objects")
	end)
	local ok = pcall(function()
		for i, b in ipairs(boxes) do
			if relTo == "Active" and i == 1 then
				continue
			end
			local cur = Vector3.new(
				targetCoord(b.minV.X, b.maxV.X),
				targetCoord(b.minV.Y, b.maxV.Y),
				targetCoord(b.minV.Z, b.maxV.Z)
			)
			local dx = if axes.X then (target.X - cur.X) else 0
			local dy = if axes.Y then (target.Y - cur.Y) else 0
			local dz = if axes.Z then (target.Z - cur.Z) else 0
			translateObject(b.inst, basis:VectorToWorldSpace(Vector3.new(dx, dy, dz)))
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
			ChangeHistoryService:SetWaypoint("Align Objects")
		end)
	end
	return ok
end

-- Studio-style circular radio button (16px ring + inner dot)
local function StudioRadio(label: string, selected: () -> boolean, onClick: () -> (), widthPx: number, order: number)
	return create "TextButton" {
		Name = "Radio_" .. label,
		LayoutOrder = order,
		Size = UDim2.fromOffset(widthPx, 20),
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		AutoButtonColor = false,
		Text = "",
		Activated = onClick,
		{
			create "Frame" {
				Position = UDim2.new(0, 0, 0.5, -8),
				Size = UDim2.fromOffset(16, 16),
				BackgroundColor3 = function()
					return if selected() then Color3.fromRGB(240, 240, 242) else Color3.fromRGB(34, 36, 40)
				end,
				BorderSizePixel = 0,
				{
					create "UICorner" { CornerRadius = UDim.new(1, 0) },
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = function()
							return if selected() then Color3.fromRGB(240, 240, 242) else Color3.fromRGB(90, 93, 102)
						end,
					},
					create "Frame" {
						AnchorPoint = Vector2.new(0.5, 0.5),
						Position = UDim2.fromScale(0.5, 0.5),
						Size = UDim2.fromOffset(6, 6),
						Visible = selected,
						BackgroundColor3 = Color3.fromRGB(28, 30, 34),
						BorderSizePixel = 0,
						{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
					},
				},
			},
			create "TextLabel" {
				Position = UDim2.fromOffset(22, 0),
				Size = UDim2.new(1, -22, 1, 0),
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSans,
				TextSize = 14,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_TEXT,
				Text = label,
			},
		},
	}
end

-- Studio-style square checkbox (16px box + label)
local function StudioCheck(label: string, checked: () -> boolean, onClick: () -> (), order: number)
	return create "TextButton" {
		Name = "Axis_" .. label,
		LayoutOrder = order,
		Size = UDim2.fromOffset(42, 20),
		BackgroundTransparency = 1,
		BorderSizePixel = 0,
		AutoButtonColor = false,
		Text = "",
		Activated = onClick,
		{
			create "Frame" {
				Position = UDim2.new(0, 0, 0.5, -8),
				Size = UDim2.fromOffset(16, 16),
				BackgroundColor3 = function()
					return if checked() then C_ACCENT() else Color3.fromRGB(34, 36, 40)
				end,
				BorderSizePixel = 0,
				{
					create "UICorner" { CornerRadius = UDim.new(0, 3) },
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = function()
							return if checked() then C_ACCENT() else Color3.fromRGB(90, 93, 102)
						end,
					},
					create "TextLabel" {
						Size = UDim2.fromScale(1, 1),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansBold,
						TextSize = 12,
						TextColor3 = Color3.fromRGB(255, 255, 255),
						Text = function()
							return if checked() then "✔" else ""
						end,
					},
				},
			},
			create "TextLabel" {
				Position = UDim2.fromOffset(21, 0),
				Size = UDim2.new(1, -21, 1, 0),
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSans,
				TextSize = 14,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_TEXT,
				Text = label,
			},
		},
	}
end

-- Studio LabeledElementList row: 76px left label + right control area
local function FormRow(label: string, height: number, order: number, content: any)
	return create "Frame" {
		Name = "Row_" .. label,
		LayoutOrder = order,
		Size = UDim2.new(1, 0, 0, height),
		BackgroundTransparency = 1,
		{
			create "TextLabel" {
				Position = UDim2.fromOffset(0, 2),
				Size = UDim2.fromOffset(74, 20),
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSans,
				TextSize = 14,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_TEXT,
				Text = label,
			},
			create "Frame" {
				Position = UDim2.fromOffset(80, 0),
				Size = UDim2.new(1, -80, 1, 0),
				BackgroundTransparency = 1,
				content,
			},
		},
	}
end

local function AlignmentApp()
	local mode = source("Center" :: AlignMode)
	local axisX = source(true)
	local axisY = source(false)
	local axisZ = source(false)
	local space = source("World" :: CoordSpace)
	local relTo = source("Selection" :: RelativeTo)
	local alignableCount = source(#getAlignableObjects())

	Vide.cleanup(Selection.SelectionChanged:Connect(function()
		alignableCount(#getAlignableObjects())
	end))

	local canAlign = Vide.derive(function()
		return alignableCount() >= 2 and (axisX() or axisY() or axisZ())
	end)

	local function ModeBtn(m: AlignMode, iconFile: string, order: number)
		local hovered = source(false)
		return create "TextButton" {
			Name = "Mode_" .. m,
			LayoutOrder = order,
			Size = UDim2.fromOffset(32, 32),
			AutoButtonColor = false,
			BorderSizePixel = 0,
			Text = "",
			BackgroundColor3 = function()
				if mode() == m then
					return MODE_ACTIVE_BG
				elseif hovered() then
					return C_BTN_HOVER()
				end
				return C_BG()
			end,
			BackgroundTransparency = function()
				return if mode() == m or hovered() then 0 else 1
			end,
			MouseEnter = function() hovered(true) end,
			MouseLeave = function() hovered(false) end,
			Activated = function() mode(m) end,
			{
				create "UICorner" { CornerRadius = UDim.new(0, 4) },
				create "ImageLabel" {
					AnchorPoint = Vector2.new(0.5, 0.5),
					Position = UDim2.fromScale(0.5, 0.5),
					Size = UDim2.fromOffset(24, 24),
					BackgroundTransparency = 1,
					Image = ICON_BASE .. iconFile,
				},
			},
		}
	end

	return create "ScrollingFrame" {
		Name = "AlignmentRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		CanvasSize = UDim2.fromOffset(0, 0),
		AutomaticCanvasSize = Enum.AutomaticSize.Y,
		ScrollBarThickness = 6,
		{
			create "UIPadding" {
				PaddingLeft = UDim.new(0, 10),
				PaddingRight = UDim.new(0, 10),
				PaddingTop = UDim.new(0, 12),
				PaddingBottom = UDim.new(0, 12),
			},
			create "UIListLayout" {
				FillDirection = Enum.FillDirection.Vertical,
				SortOrder = Enum.SortOrder.LayoutOrder,
				Padding = UDim.new(0, 15), -- Exact SectionPadding from MakeTheme.bin
			},

			-- Mode: [AlignMin] [AlignCenter] [AlignMax] (32x32 buttons, 24x24 icons)
			FormRow("Mode", 32, 1, {
				create "UIListLayout" {
					FillDirection = Enum.FillDirection.Horizontal,
					SortOrder = Enum.SortOrder.LayoutOrder,
					Padding = UDim.new(0, 4),
				},
				ModeBtn("Min", "AlignMin.png", 1),
				ModeBtn("Center", "AlignCenter.png", 2),
				ModeBtn("Max", "AlignMax.png", 3),
			}),

			-- Align In: World / Local radios + X / Y / Z checkboxes directly beneath
			FormRow("Align In", 50, 2, {
				create "UIListLayout" {
					FillDirection = Enum.FillDirection.Vertical,
					SortOrder = Enum.SortOrder.LayoutOrder,
					Padding = UDim.new(0, 8),
				},
				create "Frame" {
					LayoutOrder = 1,
					Size = UDim2.new(1, 0, 0, 20),
					BackgroundTransparency = 1,
					{
						create "UIListLayout" {
							FillDirection = Enum.FillDirection.Horizontal,
							SortOrder = Enum.SortOrder.LayoutOrder,
							Padding = UDim.new(0, 12),
						},
						StudioRadio("World", function() return space() == "World" end, function() space("World") end, 64, 1),
						StudioRadio("Local", function() return space() == "Local" end, function() space("Local") end, 64, 2),
					},
				},
				create "Frame" {
					LayoutOrder = 2,
					Size = UDim2.new(1, 0, 0, 20),
					BackgroundTransparency = 1,
					{
						create "UIListLayout" {
							FillDirection = Enum.FillDirection.Horizontal,
							SortOrder = Enum.SortOrder.LayoutOrder,
							Padding = UDim.new(0, 6),
						},
						StudioCheck("X", axisX, function() axisX(not axisX()) end, 1),
						StudioCheck("Y", axisY, function() axisY(not axisY()) end, 2),
						StudioCheck("Z", axisZ, function() axisZ(not axisZ()) end, 3),
					},
				},
			}),

			-- Relative To: Selection Bounds / Active Object vertical radios
			FormRow("Relative To", 48, 3, {
				create "UIListLayout" {
					FillDirection = Enum.FillDirection.Vertical,
					SortOrder = Enum.SortOrder.LayoutOrder,
					Padding = UDim.new(0, 8),
				},
				StudioRadio("Selection Bounds", function() return relTo() == "Selection" end, function() relTo("Selection") end, 140, 1),
				StudioRadio("Active Object", function() return relTo() == "Active" end, function() relTo("Active") end, 140, 2),
			}),

			-- Centered Primary Align Button (160x32 from MakeTheme.bin)
			create "Frame" {
				LayoutOrder = 4,
				Size = UDim2.new(1, 0, 0, 36),
				BackgroundTransparency = 1,
				{
					create "TextButton" {
						Name = "AlignButton",
						AnchorPoint = Vector2.new(0.5, 0),
						Position = UDim2.new(0.5, 0, 0, 4),
						Size = UDim2.fromOffset(148, 30),
						AutoButtonColor = false,
						BorderSizePixel = 0,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 14,
						BackgroundColor3 = function()
							return if canAlign() then C_ACCENT() else C_BTN_DIS()
						end,
						TextColor3 = function()
							return if canAlign() then C_ACCENT_TEXT() else C_DIM()
						end,
						Text = "Align",
						Activated = function()
							if canAlign() then
								runAlign(mode(), { X = axisX(), Y = axisY(), Z = axisZ() }, space(), relTo())
							end
						end,
						{
							create "UICorner" { CornerRadius = UDim.new(0, 4) },
							create "UIStroke" {
								ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
								Thickness = 1,
								Color = function()
									return if canAlign() then C_ACCENT() else C_BORDER()
								end,
							},
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
		unmount = Vide.mount(AlignmentApp, widget)
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
