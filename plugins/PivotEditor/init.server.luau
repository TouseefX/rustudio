--!strict
--[[
    RusTudio Built-in Suite: Pivot Editor (Luau / Vide)

    Inspect, snap, and reset PivotOffset / WorldPivot on selected BaseParts and
    Models. Exposes both the Model → Pivot panel and the one-click Model → Reset
    action (undoable via ChangeHistoryService).
]]

local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local action = Vide.action

local toolbar = plugin:CreateToolbar("pivot")
local editButton = toolbar:CreateButton(
	"pivot",
	"Inspect and edit pivot offset for selected parts and models",
	"rbxasset://studio_svg_textures/Shared/Ribbon/Dark/Medium/RibbonEditPivot.png",
	"Edit Pivot"
)
editButton.ClickableWhenViewportHidden = true

local resetButton = toolbar:CreateButton(
	"pivot_reset",
	"Reset PivotOffset / WorldPivot of selected parts and models",
	"rbxasset://studio_svg_textures/Shared/Ribbon/Dark/Medium/RibbonResetPivot.png",
	"Reset Pivot"
)
resetButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"PivotEditor",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, false, false, 270, 310, 220, 220)
)
widget.Title = "Pivot Editor"
widget.Name = "RusTudioPivotEditor"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)

local function round2(n: number): number
	return math.floor(n * 100 + 0.5) / 100
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
		warn("[PivotEditor] " .. tostring(err))
	end
end

local function pivotTargets(): { Instance }
	local out = {}
	for _, inst in ipairs(Selection:Get()) do
		if inst:IsA("BasePart") or inst:IsA("Model") then
			table.insert(out, inst)
		end
	end
	return out
end

local function resetSelectedPivots(): number
	local targets = pivotTargets()
	if #targets == 0 then
		return 0
	end
	withRecording("PivotEditorReset", "Reset Pivot", function()
		for _, inst in ipairs(targets) do
			if inst:IsA("BasePart") then
				inst.PivotOffset = CFrame.identity
			elseif inst:IsA("Model") then
				if inst.PrimaryPart then
					inst.PrimaryPart.PivotOffset = CFrame.identity
				else
					local ok, cf = pcall(function()
						return inst:GetBoundingBox()
					end)
					if ok and cf then
						inst.WorldPivot = cf
					end
				end
			end
		end
	end)
	return #targets
end

local function applyOffsetToTargets(offsetBuilder: (Instance) -> CFrame, label: string): number
	local targets = pivotTargets()
	if #targets == 0 then
		return 0
	end
	withRecording("PivotEditorEdit", label, function()
		for _, inst in ipairs(targets) do
			local cf = offsetBuilder(inst)
			if inst:IsA("BasePart") then
				inst.PivotOffset = cf
			elseif inst:IsA("Model") then
				if inst.PrimaryPart then
					inst.PrimaryPart.PivotOffset = cf
				else
					local ok, boxCf = pcall(function()
						return inst:GetBoundingBox()
					end)
					if ok and boxCf then
						inst.WorldPivot = boxCf * cf
					end
				end
			end
		end
	end)
	return #targets
end

local function readCurrentOffset(): (Instance?, Vector3, Vector3)
	local targets = pivotTargets()
	local first = targets[1]
	if not first then
		return nil, Vector3.zero, Vector3.zero
	end
	local cf = CFrame.identity
	if first:IsA("BasePart") then
		cf = first.PivotOffset
	elseif first:IsA("Model") then
		if first.PrimaryPart then
			cf = first.PrimaryPart.PivotOffset
		else
			local ok, boxCf = pcall(function()
				return first:GetBoundingBox()
			end)
			if ok and boxCf then
				cf = boxCf:ToObjectSpace(first:GetPivot())
			end
		end
	end
	local rx, ry, rz = cf:ToEulerAnglesXYZ()
	return first,
		Vector3.new(round2(cf.Position.X), round2(cf.Position.Y), round2(cf.Position.Z)),
		Vector3.new(round2(math.deg(rx)), round2(math.deg(ry)), round2(math.deg(rz)))
end

local refreshHooks: { () -> () } = {}

resetButton.Click:Connect(function()
	resetSelectedPivots()
	for _, fn in ipairs(refreshHooks) do
		fn()
	end
end)

local function PivotEditorApp()
	local targetName = source("None")
	local targetCount = source(0)
	local pos = source(Vector3.zero)
	local rot = source(Vector3.zero)
	local status = source("")

	local function refresh()
		local targets = pivotTargets()
		local first, p, r = readCurrentOffset()
		Vide.batch(function()
			targetCount(#targets)
			targetName(if first then first.Name else "No BasePart or Model selected")
			pos(p)
			rot(r)
		end)
	end

	refresh()
	Vide.cleanup(Selection.SelectionChanged:Connect(refresh))
	pcall(function()
		Vide.cleanup(ChangeHistoryService.OnUndo:Connect(refresh))
		Vide.cleanup(ChangeHistoryService.OnRedo:Connect(refresh))
	end)
	table.insert(refreshHooks, refresh)
	Vide.cleanup(function()
		local idx = table.find(refreshHooks, refresh)
		if idx then
			table.remove(refreshHooks, idx)
		end
	end)

	local function commitPose(p: Vector3, r: Vector3)
		local cf = CFrame.new(p) * CFrame.Angles(math.rad(r.X), math.rad(r.Y), math.rad(r.Z))
		local n = applyOffsetToTargets(function()
			return cf
		end, "Set Pivot Offset")
		refresh()
		if n > 0 then
			status(`Updated pivot on {n} object(s)`)
		end
	end

	local function AxisRow(label: string, getV: () -> Vector3, setV: (Vector3) -> (), order: number)
		local function field(axis: "X" | "Y" | "Z", idx: number)
			local boxRef: TextBox? = nil
			local function readAxis(): number
				local v = getV()
				if axis == "X" then
					return v.X
				elseif axis == "Y" then
					return v.Y
				else
					return v.Z
				end
			end
			Vide.effect(function()
				local val = readAxis()
				if boxRef and not boxRef:IsFocused() then
					boxRef.Text = tostring(round2(val))
				end
			end)
			return create "Frame" {
				LayoutOrder = idx,
				Size = UDim2.new(1 / 3, -3, 1, 0),
				BackgroundColor3 = C_INPUT,
				BorderSizePixel = 0,
				{
					create "UICorner" { CornerRadius = UDim.new(0, 4) },
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = C_BORDER,
					},
					create "TextLabel" {
						Position = UDim2.fromOffset(5, 0),
						Size = UDim2.new(0, 12, 1, 0),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansBold,
						TextSize = 11,
						TextColor3 = C_DIM,
						Text = axis,
					},
					create "TextBox" {
						Name = label .. "_" .. axis,
						Position = UDim2.fromOffset(18, 0),
						Size = UDim2.new(1, -22, 1, 0),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						ClearTextOnFocus = false,
						Font = Enum.Font.SourceSans,
						TextSize = 12,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_TEXT,
						Text = tostring(round2(readAxis())),
						FocusLost = function()
							if not boxRef then
								return
							end
							local n = tonumber(boxRef.Text)
							if n then
								local cur = getV()
								if axis == "X" then
									setV(Vector3.new(n, cur.Y, cur.Z))
								elseif axis == "Y" then
									setV(Vector3.new(cur.X, n, cur.Z))
								else
									setV(Vector3.new(cur.X, cur.Y, n))
								end
							else
								boxRef.Text = tostring(round2(readAxis()))
							end
						end,
						action(function(inst) boxRef = inst :: TextBox end),
					},
				},
			}
		end

		return create "Frame" {
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 44),
			BackgroundTransparency = 1,
			{
				create "TextLabel" {
					Size = UDim2.new(1, 0, 0, 16),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSansSemibold,
					TextSize = 12,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_DIM,
					Text = label,
				},
				create "Frame" {
					Position = UDim2.fromOffset(0, 18),
					Size = UDim2.new(1, 0, 0, 24),
					BackgroundTransparency = 1,
					{
						create "UIListLayout" {
							FillDirection = Enum.FillDirection.Horizontal,
							SortOrder = Enum.SortOrder.LayoutOrder,
							Padding = UDim.new(0, 4),
						},
						field("X", 1),
						field("Y", 2),
						field("Z", 3),
					},
				},
			},
		}
	end

	return create "ScrollingFrame" {
		Name = "PivotEditorRoot",
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
				PaddingTop = UDim.new(0, 10),
				PaddingBottom = UDim.new(0, 10),
			},
			create "UIListLayout" {
				FillDirection = Enum.FillDirection.Vertical,
				SortOrder = Enum.SortOrder.LayoutOrder,
				Padding = UDim.new(0, 8),
			},

			create "Frame" {
				LayoutOrder = 1,
				Size = UDim2.new(1, 0, 0, 28),
				BackgroundColor3 = C_HEADER,
				BorderSizePixel = 0,
				{
					create "UICorner" { CornerRadius = UDim.new(0, 4) },
					create "TextLabel" {
						Name = "TargetLabel",
						Position = UDim2.fromOffset(8, 0),
						Size = UDim2.new(1, -16, 1, 0),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 13,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextTruncate = Enum.TextTruncate.AtEnd,
						TextColor3 = C_TEXT,
						Text = function()
							local n = targetCount()
							if n == 0 then
								return "Select a Part or Model"
							elseif n == 1 then
								return `Target: {targetName()}`
							else
								return `Targets: {targetName()} (+{n - 1})`
							end
						end,
					},
				},
			},

			AxisRow("Offset (studs)", pos, function(nextPos)
				commitPose(nextPos, rot())
			end, 2),
			AxisRow("Orientation (°)", rot, function(nextRot)
				commitPose(pos(), nextRot)
			end, 3),

			-- Quick Snap Presets
			create "TextLabel" {
				LayoutOrder = 4,
				Size = UDim2.new(1, 0, 0, 16),
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSansSemibold,
				TextSize = 12,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_DIM,
				Text = "Snap Pivot To",
			},
			create "Frame" {
				Name = "PresetGrid",
				LayoutOrder = 5,
				Size = UDim2.new(1, 0, 0, 56),
				BackgroundTransparency = 1,
				{
					create "UIGridLayout" {
						CellSize = UDim2.new(0.5, -3, 0, 26),
						CellPadding = UDim2.fromOffset(6, 4),
						SortOrder = Enum.SortOrder.LayoutOrder,
					},
					create "TextButton" {
						Name = "SnapBottom",
						LayoutOrder = 1,
						AutoButtonColor = false,
						BorderSizePixel = 0,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 12,
						BackgroundColor3 = C_BUTTON,
						TextColor3 = C_TEXT,
						Text = "Bottom Center",
						Activated = function()
							applyOffsetToTargets(function(inst)
								if inst:IsA("BasePart") then
									return CFrame.new(0, -inst.Size.Y * 0.5, 0)
								elseif inst:IsA("Model") then
									local ok, _, sz = pcall(function() return inst:GetBoundingBox() end)
									if ok and sz then
										return CFrame.new(0, -sz.Y * 0.5, 0)
									end
								end
								return CFrame.identity
							end, "Snap Pivot Bottom")
							refresh()
							status("Snapped pivot to bottom center")
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},
					create "TextButton" {
						Name = "SnapTop",
						LayoutOrder = 2,
						AutoButtonColor = false,
						BorderSizePixel = 0,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 12,
						BackgroundColor3 = C_BUTTON,
						TextColor3 = C_TEXT,
						Text = "Top Center",
						Activated = function()
							applyOffsetToTargets(function(inst)
								if inst:IsA("BasePart") then
									return CFrame.new(0, inst.Size.Y * 0.5, 0)
								elseif inst:IsA("Model") then
									local ok, _, sz = pcall(function() return inst:GetBoundingBox() end)
									if ok and sz then
										return CFrame.new(0, sz.Y * 0.5, 0)
									end
								end
								return CFrame.identity
							end, "Snap Pivot Top")
							refresh()
							status("Snapped pivot to top center")
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},
					create "TextButton" {
						Name = "SnapCenter",
						LayoutOrder = 3,
						AutoButtonColor = false,
						BorderSizePixel = 0,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 12,
						BackgroundColor3 = C_BUTTON,
						TextColor3 = C_TEXT,
						Text = "Center (0,0,0)",
						Activated = function()
							local n = resetSelectedPivots()
							refresh()
							if n > 0 then
								status(`Centered pivot on {n} object(s)`)
							end
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},
					create "TextButton" {
						Name = "ResetPivotBtn",
						LayoutOrder = 4,
						AutoButtonColor = false,
						BorderSizePixel = 0,
						Font = Enum.Font.SourceSansBold,
						TextSize = 12,
						BackgroundColor3 = C_ACCENT,
						TextColor3 = C_ACCENT_TEXT,
						Text = "Reset Pivot",
						Activated = function()
							local n = resetSelectedPivots()
							refresh()
							if n > 0 then
								status(`Reset pivot on {n} object(s)`)
							end
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},
				},
			},

			create "TextLabel" {
				Name = "StatusLabel",
				LayoutOrder = 6,
				Size = UDim2.new(1, 0, 0, 18),
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSans,
				TextSize = 12,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_DIM,
				Text = status,
			},
		},
	}
end

local unmount: (() -> ())? = nil

local function syncMount()
	local isOpen = widget.Enabled
	editButton:SetActive(isOpen)
	if isOpen and not unmount then
		unmount = Vide.mount(PivotEditorApp, widget)
	elseif not isOpen and unmount then
		local cleanup = unmount
		unmount = nil
		cleanup()
	end
end

editButton.Click:Connect(function()
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
