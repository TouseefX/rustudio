--!strict
--[[
    RusTudio Built-in Suite: 9-Slice Editor (Luau / Vide)

    1:1 recreation of Roblox Studio's 9SliceEditor built-in plugin:
      - Triggered via GuiService.Open9SliceEditor (Properties SliceCenter "...")
        or the UI ribbon "9-Slice" button.
      - Interactive checkerboard ImageEditor with 4 draggable Left/Right/Top/Bottom
        slice lines and margin shading.
      - Synchronized Left / Right / Top / Bottom pixel offset TextBoxes, quick
        presets, ScaleType = Slice helper, Revert (undoable), and Close.
]]

local Selection = game:GetService("Selection")
local GuiService = game:GetService("GuiService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local effect = Vide.effect
local action = Vide.action

local toolbar = plugin:CreateToolbar("nine_slice_editor")
local toggleButton = toolbar:CreateButton(
	"nine_slice_editor",
	"Visually edit SliceCenter bounds for ImageLabel and ImageButton UI objects",
	"",
	"9-Slice"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"9SliceEditor",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Float, false, false, 380, 460, 320, 390)
)
widget.Title = "9-Slice Editor"
widget.Name = "RusTudio9SliceEditor"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)

local GUIDE_COLOR = Color3.fromRGB(0, 162, 255)
local GUIDE_HOT = Color3.fromRGB(92, 206, 255)
local CHECK_A = Color3.fromRGB(40, 42, 48)
local CHECK_B = Color3.fromRGB(52, 55, 63)

type SliceTarget = ImageLabel | ImageButton

local function isSliceTarget(inst: Instance?): boolean
	return inst ~= nil and (inst:IsA("ImageLabel") or inst:IsA("ImageButton"))
end

local function selectedSliceTarget(): SliceTarget?
	for _, inst in ipairs(Selection:Get()) do
		if isSliceTarget(inst) then
			return inst :: SliceTarget
		end
	end
	return nil
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
		warn("[9SliceEditor] " .. tostring(err))
	end
end

local function inferImageSize(inst: SliceTarget): (number, number)
	local rs = inst.ImageRectSize
	if rs.X > 0 and rs.Y > 0 then
		return math.max(1, math.floor(rs.X + 0.5)), math.max(1, math.floor(rs.Y + 0.5))
	end
	local sc = inst.SliceCenter
	local minW = math.max(1, math.ceil(math.max(sc.Min.X, sc.Max.X)))
	local minH = math.max(1, math.ceil(math.max(sc.Min.Y, sc.Max.Y)))
	local abs = inst.AbsoluteSize
	local w = if abs.X >= 16 then math.floor(abs.X + 0.5) else 256
	local h = if abs.Y >= 16 then math.floor(abs.Y + 0.5) else 256
	return math.max(w, minW, 16), math.max(h, minH, 16)
end

-- Shared external open hook (fired by GuiService.Open9SliceEditor)
local requestedTarget = source(nil :: SliceTarget?)

local function NineSliceApp()
	local target = source(requestedTarget() or selectedSliceTarget())
	local revertRect = source(Rect.new(0, 0, 0, 0))
	local imgW = source(256)
	local imgH = source(256)
	local leftPx = source(0)
	local rightPx = source(0)
	local topPx = source(0)
	local bottomPx = source(0)
	local isSliceMode = source(true)
	local imageUri = source("")
	local imageColor = source(Color3.fromRGB(255, 255, 255))
	local rectOffset = source(Vector2.zero)
	local rectSize = source(Vector2.zero)
	local activeDrag = source(nil :: ("Left" | "Right" | "Top" | "Bottom")?)
	local statusMsg = source("")

	local propConns: { RBXScriptConnection } = {}
	local canvasRef: Frame? = nil

	local function clearPropConns()
		for _, c in ipairs(propConns) do
			c:Disconnect()
		end
		table.clear(propConns)
	end

	local function syncFromTarget(inst: SliceTarget?, resetRevert: boolean)
		clearPropConns()
		if not inst or not inst.Parent then
			Vide.batch(function()
				target(nil)
				imageUri("")
				isSliceMode(true)
				leftPx(0)
				rightPx(0)
				topPx(0)
				bottomPx(0)
			end)
			return
		end

		local w, h = inferImageSize(inst)
		local sc = inst.SliceCenter
		local x0 = math.clamp(math.floor(sc.Min.X + 0.5), 0, w)
		local y0 = math.clamp(math.floor(sc.Min.Y + 0.5), 0, h)
		local x1 = if sc.Max.X > 0 then math.clamp(math.floor(sc.Max.X + 0.5), x0, w) else w
		local y1 = if sc.Max.Y > 0 then math.clamp(math.floor(sc.Max.Y + 0.5), y0, h) else h

		Vide.batch(function()
			target(inst)
			if resetRevert then
				revertRect(sc)
			end
			imgW(w)
			imgH(h)
			leftPx(x0)
			rightPx(math.max(0, w - x1))
			topPx(y0)
			bottomPx(math.max(0, h - y1))
			isSliceMode(inst.ScaleType == Enum.ScaleType.Slice)
			imageUri(tostring(inst.Image or ""))
			imageColor(inst.ImageColor3)
			rectOffset(inst.ImageRectOffset)
			rectSize(inst.ImageRectSize)
		end)

		local function watch(prop: string)
			local ok, sig = pcall(function()
				return inst:GetPropertyChangedSignal(prop)
			end)
			if ok and sig then
				table.insert(propConns, sig:Connect(function()
					if activeDrag() == nil then
						syncFromTarget(inst, false)
					end
				end))
			end
		end
		watch("SliceCenter")
		watch("ScaleType")
		watch("Image")
		watch("ImageColor3")
		watch("ImageRectOffset")
		watch("ImageRectSize")
		table.insert(propConns, inst.AncestryChanged:Connect(function()
			if not inst.Parent then
				syncFromTarget(nil, false)
			end
		end))
	end

	local function writeSlice(l: number, r: number, t: number, b: number, record: boolean)
		local inst = target()
		if not inst or not inst.Parent then
			return
		end
		local w = math.max(1, imgW())
		local h = math.max(1, imgH())
		local cl = math.clamp(math.floor(l + 0.5), 0, w)
		local cr = math.clamp(math.floor(r + 0.5), 0, w - cl)
		local ct = math.clamp(math.floor(t + 0.5), 0, h)
		local cb = math.clamp(math.floor(b + 0.5), 0, h - ct)
		local newRect = Rect.new(cl, ct, w - cr, h - cb)
		Vide.batch(function()
			leftPx(cl)
			rightPx(cr)
			topPx(ct)
			bottomPx(cb)
		end)
		if record then
			withRecording("9SliceEditorEdit", "9SliceEditor changed SliceCenter", function()
				inst.SliceCenter = newRect
			end)
		else
			inst.SliceCenter = newRect
		end
	end

	syncFromTarget(target(), true)

	Vide.cleanup(Selection.SelectionChanged:Connect(function()
		local nextT = selectedSliceTarget()
		if nextT and nextT ~= target() then
			syncFromTarget(nextT, true)
			statusMsg("")
		end
	end))
	effect(function()
		local req = requestedTarget()
		if req and req.Parent and req ~= target() then
			syncFromTarget(req, true)
			statusMsg("")
		end
	end)
	Vide.cleanup(clearPropConns)

	local function updateDragFromPointer(inputPos: Vector3)
		local side = activeDrag()
		if not side or not canvasRef then
			return
		end
		local absPos = canvasRef.AbsolutePosition
		local absSz = canvasRef.AbsoluteSize
		if absSz.X <= 1 or absSz.Y <= 1 then
			return
		end
		local u = math.clamp((inputPos.X - absPos.X) / absSz.X, 0, 1)
		local v = math.clamp((inputPos.Y - absPos.Y) / absSz.Y, 0, 1)
		local w = math.max(1, imgW())
		local h = math.max(1, imgH())
		if side == "Left" then
			local nl = math.clamp(math.floor(u * w + 0.5), 0, w - rightPx())
			writeSlice(nl, rightPx(), topPx(), bottomPx(), false)
		elseif side == "Right" then
			local nr = math.clamp(math.floor((1 - u) * w + 0.5), 0, w - leftPx())
			writeSlice(leftPx(), nr, topPx(), bottomPx(), false)
		elseif side == "Top" then
			local nt = math.clamp(math.floor(v * h + 0.5), 0, h - bottomPx())
			writeSlice(leftPx(), rightPx(), nt, bottomPx(), false)
		elseif side == "Bottom" then
			local nb = math.clamp(math.floor((1 - v) * h + 0.5), 0, h - topPx())
			writeSlice(leftPx(), rightPx(), topPx(), nb, false)
		end
	end

	local function finishDrag()
		if activeDrag() ~= nil then
			activeDrag(nil)
			writeSlice(leftPx(), rightPx(), topPx(), bottomPx(), true)
			statusMsg("Updated SliceCenter")
		end
	end

	-- Checkerboard cells (8x8 dark grid inside preview canvas)
	local checkerCells = {}
	for row = 0, 7 do
		for col = 0, 7 do
			if (row + col) % 2 == 1 then
				table.insert(
					checkerCells,
					create "Frame" {
						Position = UDim2.fromScale(col / 8, row / 8),
						Size = UDim2.fromScale(1 / 8, 1 / 8),
						BackgroundColor3 = CHECK_B,
						BorderSizePixel = 0,
						ZIndex = 0,
					}
				)
			end
		end
	end

	local function OffsetInput(label: string, getVal: () -> number, onCommit: (number) -> (), order: number)
		local boxRef: TextBox? = nil
		effect(function()
			local v = getVal()
			if boxRef and not boxRef:IsFocused() then
				boxRef.Text = tostring(v)
			end
		end)
		return create "Frame" {
			LayoutOrder = order,
			Size = UDim2.new(0.5, -4, 0, 24),
			BackgroundTransparency = 1,
			{
				create "TextLabel" {
					Size = UDim2.new(0, 50, 1, 0),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSans,
					TextSize = 13,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_TEXT,
					Text = label,
				},
				create "Frame" {
					Position = UDim2.fromOffset(52, 0),
					Size = UDim2.new(1, -52, 1, 0),
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
							Name = "Offset_" .. label,
							Position = UDim2.fromOffset(6, 0),
							Size = UDim2.new(1, -10, 1, 0),
							BackgroundTransparency = 1,
							BorderSizePixel = 0,
							ClearTextOnFocus = false,
							Font = Enum.Font.SourceSans,
							TextSize = 13,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextColor3 = C_TEXT,
							Text = tostring(getVal()),
							FocusLost = function()
								if not boxRef then
									return
								end
								local n = tonumber(boxRef.Text)
								if n then
									onCommit(math.max(0, math.floor(n + 0.5)))
								else
									boxRef.Text = tostring(getVal())
								end
							end,
							action(function(inst) boxRef = inst :: TextBox end),
						},
					},
				},
			},
		}
	end

	local function PresetBtn(label: string, onClick: () -> (), order: number)
		return create "TextButton" {
			Name = "Preset_" .. label,
			LayoutOrder = order,
			Size = UDim2.new(0.25, -4, 0, 22),
			AutoButtonColor = false,
			BorderSizePixel = 0,
			BackgroundColor3 = C_BUTTON,
			Font = Enum.Font.SourceSans,
			TextSize = 11,
			TextColor3 = C_TEXT,
			Text = label,
			Activated = onClick,
			{
				create "UICorner" { CornerRadius = UDim.new(0, 4) },
				create "UIStroke" {
					ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
					Thickness = 1,
					Color = C_BORDER,
				},
			},
		}
	end

	return create "Frame" {
		Name = "NineSliceRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- Empty state when no ImageLabel / ImageButton is selected
			show(function() return target() == nil end, function()
				return create "Frame" {
					Name = "NoSelectionState",
					Size = UDim2.fromScale(1, 1),
					BackgroundTransparency = 1,
					{
						create "TextLabel" {
							AnchorPoint = Vector2.new(0.5, 0.5),
							Position = UDim2.fromScale(0.5, 0.46),
							Size = UDim2.new(1, -36, 0, 64),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 14,
							TextWrapped = true,
							TextColor3 = C_DIM,
							Text = "Select an ImageLabel or ImageButton to edit its 9-Slice SliceCenter.",
						},
					},
				}
			end, function()
				return create "Frame" {
					Name = "SliceEditorMain",
					Size = UDim2.fromScale(1, 1),
					BackgroundTransparency = 1,
					{
						create "UIPadding" {
							PaddingLeft = UDim.new(0, 12),
							PaddingRight = UDim.new(0, 12),
							PaddingTop = UDim.new(0, 10),
							PaddingBottom = UDim.new(0, 10),
						},

						-- Target header + optional "Set ScaleType = Slice" quick fix
						create "Frame" {
							Name = "HeaderRow",
							Size = UDim2.new(1, 0, 0, 26),
							BackgroundTransparency = 1,
							{
								create "TextLabel" {
									Size = UDim2.new(1, -124, 1, 0),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSansSemibold,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = C_TEXT,
									Text = function()
										local t = target()
										return if t then `{t.Name} ({imgW()} × {imgH()} px)` else ""
									end,
								},
								show(function() return not isSliceMode() end, function()
									return create "TextButton" {
										Name = "EnableSliceBtn",
										AnchorPoint = Vector2.new(1, 0.5),
										Position = UDim2.new(1, 0, 0.5, 0),
										Size = UDim2.fromOffset(118, 22),
										AutoButtonColor = false,
										BorderSizePixel = 0,
										BackgroundColor3 = C_ACCENT,
										Font = Enum.Font.SourceSansSemibold,
										TextSize = 11,
										TextColor3 = C_ACCENT_TEXT,
										Text = "Set ScaleType = Slice",
										Activated = function()
											local t = target()
											if t then
												withRecording("9SliceScaleType", "Set ScaleType to Slice", function()
													t.ScaleType = Enum.ScaleType.Slice
												end)
												isSliceMode(true)
												statusMsg("ScaleType set to Slice")
											end
										end,
										{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
									}
								end),
							},
						},

						-- ImageEditor checkerboard preview + 4 draggable slice guides
						create "Frame" {
							Name = "ImageEditorHost",
							Position = UDim2.fromOffset(0, 32),
							Size = UDim2.new(1, 0, 1, -206),
							BackgroundColor3 = Color3.fromRGB(24, 26, 30),
							BorderSizePixel = 0,
							{
								create "UICorner" { CornerRadius = UDim.new(0, 4) },
								create "UIStroke" {
									ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
									Thickness = 1,
									Color = C_BORDER,
								},
								create "Frame" {
									Name = "ImageEditorCanvas",
									AnchorPoint = Vector2.new(0.5, 0.5),
									Position = UDim2.fromScale(0.5, 0.5),
									Size = UDim2.new(1, -28, 1, -28),
									BackgroundColor3 = CHECK_A,
									BorderSizePixel = 0,
									ClipsDescendants = false,
									InputChanged = function(input: InputObject)
										if activeDrag() ~= nil and input.UserInputType == Enum.UserInputType.MouseMovement then
											updateDragFromPointer(input.Position)
										end
									end,
									InputEnded = function(input: InputObject)
										if input.UserInputType == Enum.UserInputType.MouseButton1 then
											finishDrag()
										end
									end,
									action(function(inst) canvasRef = inst :: Frame end),
									{
										create "UIAspectRatioConstraint" {
											AspectRatio = function()
												return math.clamp(imgW() / math.max(1, imgH()), 0.25, 4.0)
											end,
										},
										create "Frame" {
											Name = "Checker",
											Size = UDim2.fromScale(1, 1),
											BackgroundTransparency = 1,
											ClipsDescendants = true,
											checkerCells,
										},
										create "ImageLabel" {
											Name = "PreviewImage",
											Size = UDim2.fromScale(1, 1),
											BackgroundTransparency = 1,
											ZIndex = 1,
											ScaleType = Enum.ScaleType.Stretch,
											Image = imageUri,
											ImageColor3 = imageColor,
											ImageRectOffset = rectOffset,
											ImageRectSize = rectSize,
										},

										-- Left margin shade + draggable line + handle
										create "Frame" {
											Size = function()
												return UDim2.fromScale(leftPx() / math.max(1, imgW()), 1)
											end,
											BackgroundColor3 = Color3.fromRGB(0, 110, 210),
											BackgroundTransparency = 0.84,
											BorderSizePixel = 0,
											ZIndex = 2,
										},
										create "TextButton" {
											Name = "LeftDragger",
											Position = function()
												return UDim2.new(leftPx() / math.max(1, imgW()), -6, 0, -10)
											end,
											Size = UDim2.new(0, 12, 1, 20),
											BackgroundTransparency = 1,
											BorderSizePixel = 0,
											AutoButtonColor = false,
											Text = "",
											ZIndex = 5,
											InputBegan = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													activeDrag("Left")
													updateDragFromPointer(input.Position)
												end
											end,
											InputEnded = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													finishDrag()
												end
											end,
											{
												create "Frame" {
													Position = UDim2.new(0.5, -1, 0, 10),
													Size = UDim2.new(0, 2, 1, -20),
													BackgroundColor3 = function()
														return if activeDrag() == "Left" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
												},
												create "Frame" {
													Position = UDim2.new(0.5, -4, 0, 1),
													Size = UDim2.fromOffset(8, 8),
													BackgroundColor3 = function()
														return if activeDrag() == "Left" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
													{ create "UICorner" { CornerRadius = UDim.new(0, 2) } },
												},
											},
										},

										-- Right margin shade + draggable line + handle
										create "Frame" {
											AnchorPoint = Vector2.new(1, 0),
											Position = UDim2.fromScale(1, 0),
											Size = function()
												return UDim2.fromScale(rightPx() / math.max(1, imgW()), 1)
											end,
											BackgroundColor3 = Color3.fromRGB(0, 110, 210),
											BackgroundTransparency = 0.84,
											BorderSizePixel = 0,
											ZIndex = 2,
										},
										create "TextButton" {
											Name = "RightDragger",
											Position = function()
												return UDim2.new(1 - rightPx() / math.max(1, imgW()), -6, 0, -10)
											end,
											Size = UDim2.new(0, 12, 1, 20),
											BackgroundTransparency = 1,
											BorderSizePixel = 0,
											AutoButtonColor = false,
											Text = "",
											ZIndex = 5,
											InputBegan = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													activeDrag("Right")
													updateDragFromPointer(input.Position)
												end
											end,
											InputEnded = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													finishDrag()
												end
											end,
											{
												create "Frame" {
													Position = UDim2.new(0.5, -1, 0, 10),
													Size = UDim2.new(0, 2, 1, -20),
													BackgroundColor3 = function()
														return if activeDrag() == "Right" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
												},
												create "Frame" {
													Position = UDim2.new(0.5, -4, 1, -9),
													Size = UDim2.fromOffset(8, 8),
													BackgroundColor3 = function()
														return if activeDrag() == "Right" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
													{ create "UICorner" { CornerRadius = UDim.new(0, 2) } },
												},
											},
										},

										-- Top margin shade + draggable line + handle
										create "Frame" {
											Size = function()
												return UDim2.fromScale(1, topPx() / math.max(1, imgH()))
											end,
											BackgroundColor3 = Color3.fromRGB(0, 110, 210),
											BackgroundTransparency = 0.84,
											BorderSizePixel = 0,
											ZIndex = 2,
										},
										create "TextButton" {
											Name = "TopDragger",
											Position = function()
												return UDim2.new(0, -10, topPx() / math.max(1, imgH()), -6)
											end,
											Size = UDim2.new(1, 20, 0, 12),
											BackgroundTransparency = 1,
											BorderSizePixel = 0,
											AutoButtonColor = false,
											Text = "",
											ZIndex = 5,
											InputBegan = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													activeDrag("Top")
													updateDragFromPointer(input.Position)
												end
											end,
											InputEnded = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													finishDrag()
												end
											end,
											{
												create "Frame" {
													Position = UDim2.new(0, 10, 0.5, -1),
													Size = UDim2.new(1, -20, 0, 2),
													BackgroundColor3 = function()
														return if activeDrag() == "Top" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
												},
												create "Frame" {
													Position = UDim2.new(0, 1, 0.5, -4),
													Size = UDim2.fromOffset(8, 8),
													BackgroundColor3 = function()
														return if activeDrag() == "Top" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
													{ create "UICorner" { CornerRadius = UDim.new(0, 2) } },
												},
											},
										},

										-- Bottom margin shade + draggable line + handle
										create "Frame" {
											AnchorPoint = Vector2.new(0, 1),
											Position = UDim2.fromScale(0, 1),
											Size = function()
												return UDim2.fromScale(1, bottomPx() / math.max(1, imgH()))
											end,
											BackgroundColor3 = Color3.fromRGB(0, 110, 210),
											BackgroundTransparency = 0.84,
											BorderSizePixel = 0,
											ZIndex = 2,
										},
										create "TextButton" {
											Name = "BottomDragger",
											Position = function()
												return UDim2.new(0, -10, 1 - bottomPx() / math.max(1, imgH()), -6)
											end,
											Size = UDim2.new(1, 20, 0, 12),
											BackgroundTransparency = 1,
											BorderSizePixel = 0,
											AutoButtonColor = false,
											Text = "",
											ZIndex = 5,
											InputBegan = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													activeDrag("Bottom")
													updateDragFromPointer(input.Position)
												end
											end,
											InputEnded = function(input: InputObject)
												if input.UserInputType == Enum.UserInputType.MouseButton1 then
													finishDrag()
												end
											end,
											{
												create "Frame" {
													Position = UDim2.new(0, 10, 0.5, -1),
													Size = UDim2.new(1, -20, 0, 2),
													BackgroundColor3 = function()
														return if activeDrag() == "Bottom" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
												},
												create "Frame" {
													Position = UDim2.new(1, -9, 0.5, -4),
													Size = UDim2.fromOffset(8, 8),
													BackgroundColor3 = function()
														return if activeDrag() == "Bottom" then GUIDE_HOT else GUIDE_COLOR
													end,
													BorderSizePixel = 0,
													{ create "UICorner" { CornerRadius = UDim.new(0, 2) } },
												},
											},
										},
									},
								},
							},
						},

						-- TextEditor section: Offsets (Left/Right/Top/Bottom) + SliceCenter readout + Presets + Revert/Close
						create "Frame" {
							Name = "TextEditorSection",
							Position = UDim2.new(0, 0, 1, -166),
							Size = UDim2.new(1, 0, 0, 166),
							BackgroundTransparency = 1,
							{
								create "TextLabel" {
									Size = UDim2.new(0.45, 0, 0, 18),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSansSemibold,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_TEXT,
									Text = "Offsets (px)",
								},
								create "TextLabel" {
									Position = UDim2.fromScale(0.45, 0),
									Size = UDim2.new(0.55, 0, 0, 18),
									BackgroundTransparency = 1,
									Font = Enum.Font.Code,
									TextSize = 11,
									TextXAlignment = Enum.TextXAlignment.Right,
									TextColor3 = C_DIM,
									Text = function()
										local w, h = imgW(), imgH()
										return `Rect({leftPx()}, {topPx()}, {w - rightPx()}, {h - bottomPx()})`
									end,
								},

								-- Row 1: Left / Right
								create "Frame" {
									Position = UDim2.fromOffset(0, 24),
									Size = UDim2.new(1, 0, 0, 24),
									BackgroundTransparency = 1,
									{
										create "UIListLayout" {
											FillDirection = Enum.FillDirection.Horizontal,
											SortOrder = Enum.SortOrder.LayoutOrder,
											Padding = UDim.new(0, 8),
										},
										OffsetInput("Left", leftPx, function(v)
											writeSlice(v, rightPx(), topPx(), bottomPx(), true)
										end, 1),
										OffsetInput("Right", rightPx, function(v)
											writeSlice(leftPx(), v, topPx(), bottomPx(), true)
										end, 2),
									},
								},

								-- Row 2: Top / Bottom
								create "Frame" {
									Position = UDim2.fromOffset(0, 54),
									Size = UDim2.new(1, 0, 0, 24),
									BackgroundTransparency = 1,
									{
										create "UIListLayout" {
											FillDirection = Enum.FillDirection.Horizontal,
											SortOrder = Enum.SortOrder.LayoutOrder,
											Padding = UDim.new(0, 8),
										},
										OffsetInput("Top", topPx, function(v)
											writeSlice(leftPx(), rightPx(), v, bottomPx(), true)
										end, 1),
										OffsetInput("Bottom", bottomPx, function(v)
											writeSlice(leftPx(), rightPx(), topPx(), v, true)
										end, 2),
									},
								},

								-- Quick presets row
								create "Frame" {
									Position = UDim2.fromOffset(0, 86),
									Size = UDim2.new(1, 0, 0, 24),
									BackgroundTransparency = 1,
									{
										create "UIListLayout" {
											FillDirection = Enum.FillDirection.Horizontal,
											SortOrder = Enum.SortOrder.LayoutOrder,
											Padding = UDim.new(0, 5),
										},
										PresetBtn("Thirds", function()
											local mx = math.floor(imgW() / 3)
											local my = math.floor(imgH() / 3)
											writeSlice(mx, mx, my, my, true)
											statusMsg("Applied 1/3 margins")
										end, 1),
										PresetBtn("8 px", function()
											writeSlice(8, 8, 8, 8, true)
											statusMsg("Applied 8 px margins")
										end, 2),
										PresetBtn("16 px", function()
											writeSlice(16, 16, 16, 16, true)
											statusMsg("Applied 16 px margins")
										end, 3),
										PresetBtn("Clear", function()
											writeSlice(0, 0, 0, 0, true)
											statusMsg("Cleared margins")
										end, 4),
									},
								},

								-- Status line
								create "TextLabel" {
									Position = UDim2.fromOffset(0, 114),
									Size = UDim2.new(1, 0, 0, 16),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 11,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_DIM,
									Text = statusMsg,
								},

								-- Footer buttons: Revert & Close (matches Studio SliceEditorMain)
								create "Frame" {
									Position = UDim2.new(0, 0, 1, -30),
									Size = UDim2.new(1, 0, 0, 30),
									BackgroundTransparency = 1,
									{
										create "TextButton" {
											Name = "RevertButton",
											Position = UDim2.new(1, -168, 0, 0),
											Size = UDim2.fromOffset(80, 28),
											AutoButtonColor = false,
											BorderSizePixel = 0,
											BackgroundColor3 = C_BUTTON,
											Font = Enum.Font.SourceSans,
											TextSize = 13,
											TextColor3 = C_TEXT,
											Text = "Revert",
											Activated = function()
												local t = target()
												if t then
													local orig = revertRect()
													withRecording("9SliceRevert", "Revert SliceCenter", function()
														t.SliceCenter = orig
													end)
													syncFromTarget(t, false)
													statusMsg("Reverted SliceCenter")
												end
											end,
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
											Name = "CloseButton",
											Position = UDim2.new(1, -80, 0, 0),
											Size = UDim2.fromOffset(80, 28),
											AutoButtonColor = false,
											BorderSizePixel = 0,
											BackgroundColor3 = C_ACCENT,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 13,
											TextColor3 = C_ACCENT_TEXT,
											Text = "Close",
											Activated = function()
												widget.Enabled = false
											end,
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

local function syncMount()
	local isOpen = widget.Enabled
	toggleButton:SetActive(isOpen)
	if isOpen and not unmount then
		unmount = Vide.mount(NineSliceApp, widget)
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

-- Studio's GuiService.Open9SliceEditor trigger (e.g. clicking "..." on SliceCenter)
pcall(function()
	(GuiService :: any).Open9SliceEditor:Connect(function(inst: Instance?)
		if isSliceTarget(inst) then
			requestedTarget(inst :: SliceTarget)
		else
			requestedTarget(selectedSliceTarget())
		end
		widget.Enabled = true
		syncMount()
	end)
end)

syncMount()

plugin.Unloading:Connect(function()
	if unmount then
		local cleanup = unmount
		unmount = nil
		cleanup()
	end
	widget.Enabled = false
end)
