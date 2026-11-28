--!strict
--[[
    RusTudio Built-in Suite: Path2D Editor (Luau / Vide)

    1:1 recreation of Roblox Studio's PathEditor built-in plugin:
      - Registers Studio's 4 PluginActions:
          PathEditor_MoveTool, PathEditor_AddPointMode,
          PathEditor_AddTangentMode, PathEditor_DoneEditing
      - Top-center CoreGui PathToolbar HUD when actively editing a Path2D.
      - Interactive 2D Bézier canvas using Studio's real Control_Point &
        Tangent_Handle textures, with direct drag for points & tangents,
        Mirrored/Sharp modes, Closed toggle, presets, and live curve length.
]]

local Selection = game:GetService("Selection")
local CoreGui = game:GetService("CoreGui")
local StarterGui = game:GetService("StarterGui")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local effect = Vide.effect
local action = Vide.action

local TEX_BASE = "rbxasset://textures/PathEditor/"
local TEX_CP = TEX_BASE .. "Control_Point.png"
local TEX_CP_SEL = TEX_BASE .. "Control_Point_Selected.png"
local TEX_TAN = TEX_BASE .. "Tangent_Handle.png"
local TEX_TAN_SEL = TEX_BASE .. "Tangent_Handle_Selected.png"

local toolbar = plugin:CreateToolbar("path_editor")
local toggleButton = toolbar:CreateButton(
	"path_editor",
	"Visually edit Path2D Bézier control points and tangents",
	"",
	"Path2D"
)
toggleButton.ClickableWhenViewportHidden = true

local actMove = plugin:CreatePluginAction(
	"PathEditor_MoveTool",
	"Path2D Editor Select Mode",
	"Activate Select Mode in the Path2D Editor"
)
local actAddPoint = plugin:CreatePluginAction(
	"PathEditor_AddPointMode",
	"Path2D Editor Add Point Mode",
	"Activate Add Point Mode in the Path2D Editor"
)
local actAddTangent = plugin:CreatePluginAction(
	"PathEditor_AddTangentMode",
	"Path2D Editor Add Tangent Mode",
	"Activate Add Tangent Mode in the Path2D Editor"
)
local actDone = plugin:CreatePluginAction(
	"PathEditor_DoneEditing",
	"Path2D Editor Done Editing",
	"Exit the Path2D editing mode"
)

local widget = plugin:CreateDockWidgetPluginGui(
	"PathEditor",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 310, 480, 240, 320)
)
widget.Title = "Path2D Editor"
widget.Name = "RusTudioPathEditor"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

type ToolMode = "Select" | "AddPoint" | "AddTangent"

type PtData = {
	idx: number,
	x: number,
	y: number,
	lx: number,
	ly: number,
	rx: number,
	ry: number,
}

type SegLine = {
	cx: number,
	cy: number,
	len: number,
	rot: number,
}

local function round3(n: number): number
	return math.floor(n * 1000 + 0.5) / 1000
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
		warn("[PathEditor] " .. tostring(err))
	end
end

local function findSelectedPath(): Instance?
	for _, inst in ipairs(Selection:Get()) do
		if inst.ClassName == "Path2D" then
			return inst
		end
	end
	return nil
end

local function readPoints(path: Instance?): { PtData }
	if not path or not path.Parent then
		return {}
	end
	local ok, cps = pcall(function()
		return (path :: any):GetControlPoints()
	end)
	if not ok or type(cps) ~= "table" then
		return {}
	end
	local out: { PtData } = {}
	for i, cp in ipairs(cps) do
		local p: UDim2 = cp.Position
		local l: UDim2 = cp.LeftTangent
		local r: UDim2 = cp.RightTangent
		table.insert(out, {
			idx = i,
			x = p.X.Scale,
			y = p.Y.Scale,
			lx = l.X.Scale,
			ly = l.Y.Scale,
			rx = r.X.Scale,
			ry = r.Y.Scale,
		})
	end
	return out
end

local function writePoints(path: Instance, pts: { PtData }, record: boolean)
	local cps = {}
	for _, pt in ipairs(pts) do
		table.insert(
			cps,
			(Path2DControlPoint :: any).new(
				UDim2.fromScale(pt.x, pt.y),
				UDim2.fromScale(pt.lx, pt.ly),
				UDim2.fromScale(pt.rx, pt.ry)
			)
		)
	end
	local fn = function()
		(path :: any):SetControlPoints(cps)
	end
	if record then
		withRecording("PathEditorEdit", "Edit Path2D Control Points", fn)
	else
		pcall(fn)
	end
end

local function evalCubic(p0: Vector2, c1: Vector2, c2: Vector2, p1: Vector2, t: number): Vector2
	local u = 1 - t
	return p0 * (u * u * u) + c1 * (3 * u * u * t) + c2 * (3 * u * t * t) + p1 * (t * t * t)
end

local function evalQuad(p0: Vector2, c: Vector2, p1: Vector2, t: number): Vector2
	local u = 1 - t
	return p0 * (u * u) + c * (2 * u * t) + p1 * (t * t)
end

local function buildCurveSegments(pts: { PtData }, closed: boolean, canvasPx: number): { SegLine }
	local out: { SegLine } = {}
	if #pts < 2 or canvasPx <= 1 then
		return out
	end
	local segCount = if closed then #pts else #pts - 1
	local stepsPerSeg = 14
	for s = 1, segCount do
		local a = pts[s]
		local b = pts[(s % #pts) + 1]
		local p0 = Vector2.new(a.x * canvasPx, a.y * canvasPx)
		local p1 = Vector2.new(b.x * canvasPx, b.y * canvasPx)
		local r0 = Vector2.new(a.rx * canvasPx, a.ry * canvasPx)
		local l1 = Vector2.new(b.lx * canvasPx, b.ly * canvasPx)
		local hasR = r0.Magnitude > 0.25
		local hasL = l1.Magnitude > 0.25
		local prev = p0
		for k = 1, stepsPerSeg do
			local t = k / stepsPerSeg
			local cur: Vector2
			if hasR and hasL then
				cur = evalCubic(p0, p0 + r0, p1 + l1, p1, t)
			elseif hasR then
				cur = evalQuad(p0, p0 + r0, p1, t)
			elseif hasL then
				cur = evalQuad(p0, p1 + l1, p1, t)
			else
				cur = p0:Lerp(p1, t)
			end
			local delta = cur - prev
			local len = delta.Magnitude
			if len > 0.5 then
				local mid = (prev + cur) * 0.5
				table.insert(out, {
					cx = mid.X / canvasPx,
					cy = mid.Y / canvasPx,
					len = len + 1,
					rot = math.deg(math.atan2(delta.Y, delta.X)),
				})
			end
			prev = cur
		end
	end
	return out
end

-- Shared plugin-level state so PluginActions and CoreGui PathToolbar stay in sync
local toolMode = source("Select" :: ToolMode)
local mirrored = source(true)
local activePath = source(findSelectedPath())
local points = source(readPoints(activePath()))
local selectedIdx = source(1)
local closedPath = source(false)
local curveLength = source(0)
local statusMsg = source("")

local function refreshFromPath()
	local p = activePath()
	if not p or not p.Parent then
		Vide.batch(function()
			activePath(nil)
			points({})
			selectedIdx(0)
			closedPath(false)
			curveLength(0)
		end)
		return
	end
	local pts = readPoints(p)
	local cl = false
	pcall(function() cl = (p :: any).Closed == true end)
	local len = 0
	pcall(function() len = (p :: any):GetLength() end)
	Vide.batch(function()
		points(pts)
		if #pts == 0 then
			selectedIdx(0)
		elseif selectedIdx() < 1 or selectedIdx() > #pts then
			selectedIdx(1)
		end
		closedPath(cl)
		curveLength(len)
	end)
end

local function createStarterPath()
	local parent: Instance? = nil
	for _, inst in ipairs(Selection:Get()) do
		if inst:IsA("GuiBase2d") or inst:IsA("StarterGui") then
			parent = inst
			break
		end
	end
	if not parent then
		local sg = StarterGui:FindFirstChildOfClass("ScreenGui")
		if not sg then
			sg = Instance.new("ScreenGui")
			sg.Name = "Path2DScreen"
			sg.Parent = StarterGui
		end
		parent = sg
	end
	local created: Instance? = nil
	withRecording("PathEditorCreate", "Create Path2D", function()
		local p: any = Instance.new("Path2D")
		p.Name = "Path2D"
		p.Thickness = 3
		p.Color3 = Color3.fromRGB(0, 162, 255)
		p.Parent = parent
		p:SetControlPoints({
			(Path2DControlPoint :: any).new(UDim2.fromScale(0.15, 0.72), UDim2.new(), UDim2.fromScale(0.14, -0.25)),
			(Path2DControlPoint :: any).new(UDim2.fromScale(0.50, 0.30), UDim2.fromScale(-0.14, 0), UDim2.fromScale(0.14, 0)),
			(Path2DControlPoint :: any).new(UDim2.fromScale(0.85, 0.72), UDim2.fromScale(-0.14, -0.25), UDim2.new()),
		})
		created = p
		Selection:Set({ p })
	end)
	if created then
		activePath(created)
		selectedIdx(1)
		refreshFromPath()
		statusMsg("Created starter Path2D")
	end
end

-- CoreGui floating PathToolbar HUD (exact Studio top-center PathToolbar)
local hudGui: ScreenGui? = nil
local hudUnmount: (() -> ())? = nil

local function destroyHud()
	if hudUnmount then
		local fn = hudUnmount
		hudUnmount = nil
		fn()
	end
	if hudGui then
		hudGui:Destroy()
		hudGui = nil
	end
end

local function ensureHud()
	if hudGui then
		return
	end
	local existing = CoreGui:FindFirstChild("RusTudioPathToolbar")
	if existing then
		existing:Destroy()
	end
	local sg = Instance.new("ScreenGui")
	sg.Name = "RusTudioPathToolbar"
	sg.DisplayOrder = 12
	sg.ResetOnSpawn = false
	sg.Parent = CoreGui
	hudGui = sg

	hudUnmount = Vide.mount(function()
		local function ToolPill(label: string, active: () -> boolean, onClick: () -> (), order: number, width: number)
			return create "TextButton" {
				LayoutOrder = order,
				Size = UDim2.fromOffset(width, 26),
				AutoButtonColor = false,
				BorderSizePixel = 0,
				Font = Enum.Font.SourceSansSemibold,
				TextSize = 12,
				BackgroundColor3 = function()
					return if active() then C_ACCENT() else C_BUTTON()
				end,
				TextColor3 = function()
					return if active() then C_ACCENT_TEXT() else C_TEXT()
				end,
				Text = label,
				Activated = onClick,
				{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
			}
		end

		return create "Frame" {
			Name = "PathToolbar",
			AnchorPoint = Vector2.new(0.5, 0),
			Position = UDim2.new(0.5, 0, 0, 10),
			Size = UDim2.fromOffset(396, 38),
			BackgroundColor3 = C_HEADER,
			BackgroundTransparency = 0.06,
			BorderSizePixel = 0,
			Visible = function()
				return widget.Enabled and activePath() ~= nil
			end,
			{
				create "UICorner" { CornerRadius = UDim.new(0, 6) },
				create "UIStroke" {
					ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
					Thickness = 1,
					Color = C_BORDER,
				},
				create "UIPadding" {
					PaddingLeft = UDim.new(0, 6),
					PaddingRight = UDim.new(0, 6),
					PaddingTop = UDim.new(0, 6),
					PaddingBottom = UDim.new(0, 6),
				},
				create "UIListLayout" {
					FillDirection = Enum.FillDirection.Horizontal,
					SortOrder = Enum.SortOrder.LayoutOrder,
					Padding = UDim.new(0, 5),
				},
				ToolPill("Select", function() return toolMode() == "Select" end, function() toolMode("Select") end, 1, 62),
				ToolPill("Add Point", function() return toolMode() == "AddPoint" end, function() toolMode("AddPoint") end, 2, 74),
				ToolPill("Tangent", function() return toolMode() == "AddTangent" end, function() toolMode("AddTangent") end, 3, 66),
				ToolPill("Mirror", mirrored, function() mirrored(not mirrored()) end, 4, 56),
				ToolPill("Closed", closedPath, function()
					local p = activePath()
					if p then
						local nextVal = not closedPath()
						withRecording("PathEditorClosed", "Toggle Path2D Closed", function()
							(p :: any).Closed = nextVal
						end)
						refreshFromPath()
					end
				end, 5, 56),
				ToolPill("Done", function() return false end, function()
					widget.Enabled = false
				end, 6, 52),
			},
		}
	end, sg)
end

local function PathEditorApp()
	local dragKind = source(nil :: ("Point" | "Left" | "Right")?)
	local dragIdx = source(0)
	local canvasRef: Frame? = nil
	local pathConn: RBXScriptConnection? = nil

	local function bindPathConn()
		if pathConn then
			pathConn:Disconnect()
			pathConn = nil
		end
		local p = activePath()
		if p and p.Parent then
			pcall(function()
				pathConn = (p :: any).ControlPointChanged:Connect(function()
					if dragKind() == nil then
						refreshFromPath()
					end
				end)
			end)
		end
	end

	effect(function()
		local _ = activePath()
		bindPathConn()
	end)
	Vide.cleanup(function()
		if pathConn then
			pathConn:Disconnect()
			pathConn = nil
		end
	end)

	local function clonePts(): { PtData }
		local out = {}
		for _, pt in ipairs(points()) do
			table.insert(out, {
				idx = pt.idx,
				x = pt.x,
				y = pt.y,
				lx = pt.lx,
				ly = pt.ly,
				rx = pt.rx,
				ry = pt.ry,
			})
		end
		return out
	end

	local function commitPoints(nextPts: { PtData }, record: boolean)
		local p = activePath()
		if not p or not p.Parent then
			return
		end
		for i, pt in ipairs(nextPts) do
			pt.idx = i
		end
		points(nextPts)
		writePoints(p, nextPts, record)
		local len = 0
		pcall(function() len = (p :: any):GetLength() end)
		curveLength(len)
	end

	local function updateDrag(inputPos: Vector3)
		local kind = dragKind()
		local idx = dragIdx()
		if not kind or idx < 1 or not canvasRef then
			return
		end
		local absPos = canvasRef.AbsolutePosition
		local absSz = canvasRef.AbsoluteSize
		if absSz.X <= 1 or absSz.Y <= 1 then
			return
		end
		local u = math.clamp((inputPos.X - absPos.X) / absSz.X, 0, 1)
		local v = math.clamp((inputPos.Y - absPos.Y) / absSz.Y, 0, 1)
		local nextPts = clonePts()
		local pt = nextPts[idx]
		if not pt then
			return
		end
		if kind == "Point" then
			pt.x = round3(u)
			pt.y = round3(v)
		elseif kind == "Left" then
			pt.lx = round3(u - pt.x)
			pt.ly = round3(v - pt.y)
			if mirrored() then
				pt.rx = -pt.lx
				pt.ry = -pt.ly
			end
		elseif kind == "Right" then
			pt.rx = round3(u - pt.x)
			pt.ry = round3(v - pt.y)
			if mirrored() then
				pt.lx = -pt.rx
				pt.ly = -pt.ry
			end
		end
		commitPoints(nextPts, false)
	end

	local function finishDrag()
		if dragKind() ~= nil then
			dragKind(nil)
			dragIdx(0)
			commitPoints(clonePts(), true)
		end
	end

	local segments = Vide.derive(function(): { SegLine }
		return buildCurveSegments(points(), closedPath(), 220)
	end)

	local currentPt = Vide.derive(function(): PtData?
		local pts = points()
		local idx = selectedIdx()
		if idx >= 1 and idx <= #pts then
			return pts[idx]
		end
		return nil
	end)

	local function applyPreset(name: string, nextPts: { PtData }, closed: boolean)
		local p = activePath()
		if not p then
			return
		end
		withRecording("PathEditorPreset", `Apply Path2D Preset {name}`, function()
			(p :: any).Closed = closed
			writePoints(p, nextPts, false)
		end)
		selectedIdx(1)
		refreshFromPath()
		statusMsg(`Applied {name} preset`)
	end

	local function NumField(label: string, getV: () -> number, setV: (number) -> (), order: number)
		local boxRef: TextBox? = nil
		effect(function()
			local v = getV()
			if boxRef and not boxRef:IsFocused() then
				boxRef.Text = tostring(round3(v))
			end
		end)
		return create "Frame" {
			LayoutOrder = order,
			Size = UDim2.new(0.5, -3, 1, 0),
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
					Position = UDim2.fromOffset(6, 0),
					Size = UDim2.new(0, 14, 1, 0),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSansBold,
					TextSize = 11,
					TextColor3 = C_DIM,
					Text = label,
				},
				create "TextBox" {
					Position = UDim2.fromOffset(22, 0),
					Size = UDim2.new(1, -26, 1, 0),
					BackgroundTransparency = 1,
					BorderSizePixel = 0,
					ClearTextOnFocus = false,
					Font = Enum.Font.SourceSans,
					TextSize = 12,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_TEXT,
					Text = tostring(round3(getV())),
					FocusLost = function()
						if not boxRef then
							return
						end
						local n = tonumber(boxRef.Text)
						if n then
							setV(round3(n))
						else
							boxRef.Text = tostring(round3(getV()))
						end
					end,
					action(function(inst) boxRef = inst :: TextBox end),
				},
			},
		}
	end

	local function ModeBtn(label: string, m: ToolMode, order: number)
		return create "TextButton" {
			Name = "Mode_" .. m,
			LayoutOrder = order,
			Size = UDim2.new(1 / 3, -3, 0, 24),
			AutoButtonColor = false,
			BorderSizePixel = 0,
			Font = Enum.Font.SourceSansSemibold,
			TextSize = 12,
			BackgroundColor3 = function()
				return if toolMode() == m then C_ACCENT() else C_BUTTON()
			end,
			TextColor3 = function()
				return if toolMode() == m then C_ACCENT_TEXT() else C_TEXT()
			end,
			Text = label,
			Activated = function() toolMode(m) end,
			{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
		}
	end

	return create "ScrollingFrame" {
		Name = "PathEditorRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		CanvasSize = UDim2.fromOffset(0, 0),
		AutomaticCanvasSize = Enum.AutomaticSize.Y,
		ScrollBarThickness = 6,
		ScrollBarImageColor3 = C_SCROLL,
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

			-- Empty state when no Path2D is selected
			show(function() return activePath() == nil end, function()
				return create "Frame" {
					Name = "EmptyPathState",
					LayoutOrder = 1,
					Size = UDim2.new(1, 0, 0, 150),
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
							PaddingTop = UDim.new(0, 14),
							PaddingBottom = UDim.new(0, 14),
						},
						create "TextLabel" {
							Size = UDim2.new(1, 0, 0, 20),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSansBold,
							TextSize = 14,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextColor3 = C_TEXT,
							Text = "No Path2D Selected",
						},
						create "TextLabel" {
							Position = UDim2.fromOffset(0, 26),
							Size = UDim2.new(1, 0, 0, 48),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 12,
							TextWrapped = true,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextYAlignment = Enum.TextYAlignment.Top,
							TextColor3 = C_DIM,
							Text = "Select an existing Path2D in Explorer or insert a new starter Bézier curve into StarterGui.",
						},
						create "TextButton" {
							Name = "CreatePathBtn",
							Position = UDim2.new(0, 0, 1, -28),
							Size = UDim2.new(1, 0, 0, 28),
							AutoButtonColor = false,
							BorderSizePixel = 0,
							BackgroundColor3 = C_ACCENT,
							Font = Enum.Font.SourceSansSemibold,
							TextSize = 13,
							TextColor3 = C_ACCENT_TEXT,
							Text = "+ Create Path2D",
							Activated = createStarterPath,
							{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
						},
					},
				}
			end, function()
				return create "Frame" {
					Name = "EditorContent",
					LayoutOrder = 1,
					Size = UDim2.new(1, 0, 0, 440),
					BackgroundTransparency = 1,
					{
						create "UIListLayout" {
							FillDirection = Enum.FillDirection.Vertical,
							SortOrder = Enum.SortOrder.LayoutOrder,
							Padding = UDim.new(0, 8),
						},

						-- Mode strip
						create "Frame" {
							LayoutOrder = 1,
							Size = UDim2.new(1, 0, 0, 24),
							BackgroundTransparency = 1,
							{
								create "UIListLayout" {
									FillDirection = Enum.FillDirection.Horizontal,
									SortOrder = Enum.SortOrder.LayoutOrder,
									Padding = UDim.new(0, 4),
								},
								ModeBtn("Select", "Select", 1),
								ModeBtn("Add Point", "AddPoint", 2),
								ModeBtn("Tangent", "AddTangent", 3),
							},
						},

						-- Interactive 2D Bézier Canvas (220px tall)
						create "TextButton" {
							Name = "BezierCanvas",
							LayoutOrder = 2,
							Size = UDim2.new(1, 0, 0, 196),
							AutoButtonColor = false,
							BackgroundColor3 = Color3.fromRGB(24, 26, 30),
							BorderSizePixel = 0,
							Text = "",
							ClipsDescendants = true,
							InputBegan = function(input: InputObject)
								if input.UserInputType == Enum.UserInputType.MouseButton1 and toolMode() == "AddPoint" and canvasRef then
									local absPos = canvasRef.AbsolutePosition
									local absSz = canvasRef.AbsoluteSize
									if absSz.X > 1 and absSz.Y > 1 then
										local u = round3(math.clamp((input.Position.X - absPos.X) / absSz.X, 0.02, 0.98))
										local v = round3(math.clamp((input.Position.Y - absPos.Y) / absSz.Y, 0.02, 0.98))
										local nextPts = clonePts()
										if #nextPts < 100 then
											table.insert(nextPts, {
												idx = #nextPts + 1,
												x = u,
												y = v,
												lx = 0,
												ly = 0,
												rx = 0,
												ry = 0,
											})
											selectedIdx(#nextPts)
											commitPoints(nextPts, true)
											statusMsg(`Added point #{#nextPts}`)
										end
									end
								end
							end,
							InputChanged = function(input: InputObject)
								if dragKind() ~= nil and input.UserInputType == Enum.UserInputType.MouseMovement then
									updateDrag(input.Position)
								end
							end,
							InputEnded = function(input: InputObject)
								if input.UserInputType == Enum.UserInputType.MouseButton1 then
									finishDrag()
								end
							end,
							action(function(inst) canvasRef = inst :: any end),
							{
								create "UICorner" { CornerRadius = UDim.new(0, 4) },
								create "UIStroke" {
									ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
									Thickness = 1,
									Color = C_BORDER,
								},
								-- Subtle center crosshair grid
								create "Frame" {
									Position = UDim2.fromScale(0.5, 0),
									Size = UDim2.new(0, 1, 1, 0),
									BackgroundColor3 = Color3.fromRGB(45, 48, 56),
									BorderSizePixel = 0,
									ZIndex = 0,
								},
								create "Frame" {
									Position = UDim2.fromScale(0, 0.5),
									Size = UDim2.new(1, 0, 0, 1),
									BackgroundColor3 = Color3.fromRGB(45, 48, 56),
									BorderSizePixel = 0,
									ZIndex = 0,
								},
								-- Sampled Bézier curve polyline
								values(segments, function(seg: SegLine)
									return create "Frame" {
										AnchorPoint = Vector2.new(0.5, 0.5),
										Position = UDim2.fromScale(seg.cx, seg.cy),
										Size = UDim2.fromOffset(math.max(2, math.ceil(seg.len)), 2),
										Rotation = seg.rot,
										BackgroundColor3 = Color3.fromRGB(0, 162, 255),
										BorderSizePixel = 0,
										ZIndex = 2,
									}
								end),
								-- Control points + tangent handles (using Studio's real PathEditor sprites)
								values(points, function(pt: PtData)
									local isSel = function()
										return selectedIdx() == pt.idx
									end
									local hasLeft = math.abs(pt.lx) > 1e-4 or math.abs(pt.ly) > 1e-4
									local hasRight = math.abs(pt.rx) > 1e-4 or math.abs(pt.ry) > 1e-4
									return create "Frame" {
										Size = UDim2.fromScale(1, 1),
										BackgroundTransparency = 1,
										ZIndex = 4,
										{
											-- Left tangent handle
											show(function() return isSel() and hasLeft end, function()
												return create "ImageButton" {
													Name = `LeftTan_{pt.idx}`,
													AnchorPoint = Vector2.new(0.5, 0.5),
													Position = UDim2.fromScale(
														math.clamp(pt.x + pt.lx, 0, 1),
														math.clamp(pt.y + pt.ly, 0, 1)
													),
													Size = UDim2.fromOffset(12, 12),
													BackgroundTransparency = 1,
													AutoButtonColor = false,
													Image = TEX_TAN_SEL,
													ZIndex = 5,
													InputBegan = function(input: InputObject)
														if input.UserInputType == Enum.UserInputType.MouseButton1 then
															selectedIdx(pt.idx)
															dragKind("Left")
															dragIdx(pt.idx)
														end
													end,
													InputEnded = function(input: InputObject)
														if input.UserInputType == Enum.UserInputType.MouseButton1 then
															finishDrag()
														end
													end,
												}
											end),
											-- Right tangent handle
											show(function() return isSel() and hasRight end, function()
												return create "ImageButton" {
													Name = `RightTan_{pt.idx}`,
													AnchorPoint = Vector2.new(0.5, 0.5),
													Position = UDim2.fromScale(
														math.clamp(pt.x + pt.rx, 0, 1),
														math.clamp(pt.y + pt.ry, 0, 1)
													),
													Size = UDim2.fromOffset(12, 12),
													BackgroundTransparency = 1,
													AutoButtonColor = false,
													Image = TEX_TAN,
													ZIndex = 5,
													InputBegan = function(input: InputObject)
														if input.UserInputType == Enum.UserInputType.MouseButton1 then
															selectedIdx(pt.idx)
															dragKind("Right")
															dragIdx(pt.idx)
														end
													end,
													InputEnded = function(input: InputObject)
														if input.UserInputType == Enum.UserInputType.MouseButton1 then
															finishDrag()
														end
													end,
												}
											end),
											-- Control point sprite
											create "ImageButton" {
												Name = `CP_{pt.idx}`,
												AnchorPoint = Vector2.new(0.5, 0.5),
												Position = UDim2.fromScale(pt.x, pt.y),
												Size = UDim2.fromOffset(14, 14),
												BackgroundTransparency = 1,
												AutoButtonColor = false,
												Image = function()
													return if isSel() then TEX_CP_SEL else TEX_CP
												end,
												ZIndex = 6,
												InputBegan = function(input: InputObject)
													if input.UserInputType == Enum.UserInputType.MouseButton1 then
														selectedIdx(pt.idx)
														if toolMode() == "AddTangent" then
															local nextPts = clonePts()
															local cur = nextPts[pt.idx]
															if cur then
																if math.abs(cur.lx) > 1e-4 or math.abs(cur.rx) > 1e-4 then
																	cur.lx, cur.ly, cur.rx, cur.ry = 0, 0, 0, 0
																	statusMsg(`Point #{pt.idx} set to Sharp`)
																else
																	cur.lx, cur.ly, cur.rx, cur.ry = -0.1, 0, 0.1, 0
																	statusMsg(`Point #{pt.idx} set to Smooth`)
																end
																commitPoints(nextPts, true)
															end
														else
															dragKind("Point")
															dragIdx(pt.idx)
														end
													end
												end,
												InputEnded = function(input: InputObject)
													if input.UserInputType == Enum.UserInputType.MouseButton1 then
														finishDrag()
													end
												end,
											},
										},
									}
								end),
							},
						},

						-- Curve metrics readout
						create "TextLabel" {
							LayoutOrder = 3,
							Size = UDim2.new(1, 0, 0, 16),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 12,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextColor3 = C_DIM,
							Text = function()
								local p = activePath()
								local nm = if p then p.Name else "Path2D"
								return `{nm} · {#points()}/100 pts · {string.format("%.1f", curveLength())} px`
							end,
						},

						-- Selected Control Point precision controls
						show(function() return currentPt() ~= nil end, function()
							local function editCur(mutator: (PtData) -> ())
								local nextPts = clonePts()
								local cur = nextPts[selectedIdx()]
								if cur then
									mutator(cur)
									commitPoints(nextPts, true)
								end
							end
							return create "Frame" {
								Name = "PointInspector",
								LayoutOrder = 4,
								Size = UDim2.new(1, 0, 0, 128),
								BackgroundColor3 = C_HEADER,
								BorderSizePixel = 0,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = C_BORDER,
									},
									create "UIPadding" {
										PaddingLeft = UDim.new(0, 8),
										PaddingRight = UDim.new(0, 8),
										PaddingTop = UDim.new(0, 6),
										PaddingBottom = UDim.new(0, 6),
									},
									create "UIListLayout" {
										FillDirection = Enum.FillDirection.Vertical,
										SortOrder = Enum.SortOrder.LayoutOrder,
										Padding = UDim.new(0, 5),
									},
									create "TextLabel" {
										LayoutOrder = 1,
										Size = UDim2.new(1, 0, 0, 16),
										BackgroundTransparency = 1,
										Font = Enum.Font.SourceSansSemibold,
										TextSize = 12,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = C_TEXT,
										Text = function() return `Control Point #{selectedIdx()} (Scale)` end,
									},
									create "Frame" {
										LayoutOrder = 2,
										Size = UDim2.new(1, 0, 0, 22),
										BackgroundTransparency = 1,
										{
											create "UIListLayout" {
												FillDirection = Enum.FillDirection.Horizontal,
												SortOrder = Enum.SortOrder.LayoutOrder,
												Padding = UDim.new(0, 6),
											},
											NumField("X", function()
												local c = currentPt()
												return if c then c.x else 0
											end, function(v) editCur(function(c) c.x = v end) end, 1),
											NumField("Y", function()
												local c = currentPt()
												return if c then c.y else 0
											end, function(v) editCur(function(c) c.y = v end) end, 2),
										},
									},
									create "Frame" {
										LayoutOrder = 3,
										Size = UDim2.new(1, 0, 0, 22),
										BackgroundTransparency = 1,
										{
											create "UIListLayout" {
												FillDirection = Enum.FillDirection.Horizontal,
												SortOrder = Enum.SortOrder.LayoutOrder,
												Padding = UDim.new(0, 6),
											},
											NumField("L", function()
												local c = currentPt()
												return if c then c.lx else 0
											end, function(v)
												editCur(function(c)
													c.lx = v
													if mirrored() then c.rx = -v end
												end)
											end, 1),
											NumField("R", function()
												local c = currentPt()
												return if c then c.rx else 0
											end, function(v)
												editCur(function(c)
													c.rx = v
													if mirrored() then c.lx = -v end
												end)
											end, 2),
										},
									},
									create "Frame" {
										LayoutOrder = 4,
										Size = UDim2.new(1, 0, 0, 24),
										BackgroundTransparency = 1,
										{
											create "UIListLayout" {
												FillDirection = Enum.FillDirection.Horizontal,
												SortOrder = Enum.SortOrder.LayoutOrder,
												Padding = UDim.new(0, 4),
											},
											create "TextButton" {
												Name = "SmoothPtBtn",
												LayoutOrder = 1,
												Size = UDim2.new(1 / 3, -3, 1, 0),
												AutoButtonColor = false,
												BorderSizePixel = 0,
												BackgroundColor3 = C_BUTTON,
												Font = Enum.Font.SourceSans,
												TextSize = 11,
												TextColor3 = C_TEXT,
												Text = "Smooth",
												Activated = function()
													editCur(function(c)
														c.lx, c.ly, c.rx, c.ry = -0.12, 0, 0.12, 0
													end)
												end,
												{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
											},
											create "TextButton" {
												Name = "SharpPtBtn",
												LayoutOrder = 2,
												Size = UDim2.new(1 / 3, -3, 1, 0),
												AutoButtonColor = false,
												BorderSizePixel = 0,
												BackgroundColor3 = C_BUTTON,
												Font = Enum.Font.SourceSans,
												TextSize = 11,
												TextColor3 = C_TEXT,
												Text = "Sharp",
												Activated = function()
													editCur(function(c)
														c.lx, c.ly, c.rx, c.ry = 0, 0, 0, 0
													end)
												end,
												{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
											},
											create "TextButton" {
												Name = "DeletePtBtn",
												LayoutOrder = 3,
												Size = UDim2.new(1 / 3, -3, 1, 0),
												AutoButtonColor = false,
												BorderSizePixel = 0,
												BackgroundColor3 = Color3.fromRGB(150, 52, 52),
												Font = Enum.Font.SourceSansSemibold,
												TextSize = 11,
												TextColor3 = Color3.fromRGB(255, 255, 255),
												Text = "Delete",
												Activated = function()
													local nextPts = clonePts()
													if #nextPts > 0 then
														local i = selectedIdx()
														if i >= 1 and i <= #nextPts then
															table.remove(nextPts, i)
															selectedIdx(math.min(i, #nextPts))
															commitPoints(nextPts, true)
															statusMsg("Deleted control point")
														end
													end
												end,
												{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
											},
										},
									},
								},
							}
						end),

						-- Curve Presets row
						create "Frame" {
							LayoutOrder = 5,
							Size = UDim2.new(1, 0, 0, 24),
							BackgroundTransparency = 1,
							{
								create "UIListLayout" {
									FillDirection = Enum.FillDirection.Horizontal,
									SortOrder = Enum.SortOrder.LayoutOrder,
									Padding = UDim.new(0, 4),
								},
								create "TextButton" {
									Name = "PresetLine",
									LayoutOrder = 1,
									Size = UDim2.new(0.25, -3, 1, 0),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									BackgroundColor3 = C_BUTTON,
									Font = Enum.Font.SourceSans,
									TextSize = 11,
									TextColor3 = C_TEXT,
									Text = "Line",
									Activated = function()
										applyPreset("Line", {
											{ idx = 1, x = 0.15, y = 0.5, lx = 0, ly = 0, rx = 0, ry = 0 },
											{ idx = 2, x = 0.85, y = 0.5, lx = 0, ly = 0, rx = 0, ry = 0 },
										}, false)
									end,
									{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
								},
								create "TextButton" {
									Name = "PresetSCurve",
									LayoutOrder = 2,
									Size = UDim2.new(0.25, -3, 1, 0),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									BackgroundColor3 = C_BUTTON,
									Font = Enum.Font.SourceSans,
									TextSize = 11,
									TextColor3 = C_TEXT,
									Text = "S-Curve",
									Activated = function()
										applyPreset("S-Curve", {
											{ idx = 1, x = 0.15, y = 0.78, lx = 0, ly = 0, rx = 0.25, ry = 0 },
											{ idx = 2, x = 0.85, y = 0.22, lx = -0.25, ly = 0, rx = 0, ry = 0 },
										}, false)
									end,
									{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
								},
								create "TextButton" {
									Name = "PresetArch",
									LayoutOrder = 3,
									Size = UDim2.new(0.25, -3, 1, 0),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									BackgroundColor3 = C_BUTTON,
									Font = Enum.Font.SourceSans,
									TextSize = 11,
									TextColor3 = C_TEXT,
									Text = "Arch",
									Activated = function()
										applyPreset("Arch", {
											{ idx = 1, x = 0.14, y = 0.78, lx = 0, ly = 0, rx = 0.08, ry = -0.35 },
											{ idx = 2, x = 0.50, y = 0.22, lx = -0.18, ly = 0, rx = 0.18, ry = 0 },
											{ idx = 3, x = 0.86, y = 0.78, lx = -0.08, ly = -0.35, rx = 0, ry = 0 },
										}, false)
									end,
									{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
								},
								create "TextButton" {
									Name = "PresetLoop",
									LayoutOrder = 4,
									Size = UDim2.new(0.25, -3, 1, 0),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									BackgroundColor3 = C_BUTTON,
									Font = Enum.Font.SourceSans,
									TextSize = 11,
									TextColor3 = C_TEXT,
									Text = "Loop",
									Activated = function()
										applyPreset("Loop", {
											{ idx = 1, x = 0.50, y = 0.18, lx = -0.18, ly = 0, rx = 0.18, ry = 0 },
											{ idx = 2, x = 0.82, y = 0.50, lx = 0, ly = -0.18, rx = 0, ry = 0.18 },
											{ idx = 3, x = 0.50, y = 0.82, lx = 0.18, ly = 0, rx = -0.18, ry = 0 },
											{ idx = 4, x = 0.18, y = 0.50, lx = 0, ly = 0.18, rx = 0, ry = -0.18 },
										}, true)
									end,
									{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
								},
							},
						},

						create "TextLabel" {
							LayoutOrder = 6,
							Size = UDim2.new(1, 0, 0, 16),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 11,
							TextXAlignment = Enum.TextXAlignment.Left,
							TextColor3 = C_DIM,
							Text = statusMsg,
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
	if isOpen then
		ensureHud()
		if not unmount then
			unmount = Vide.mount(PathEditorApp, widget)
		end
	else
		destroyHud()
		if unmount then
			local cleanup = unmount
			unmount = nil
			cleanup()
		end
	end
end

Selection.SelectionChanged:Connect(function()
	local found = findSelectedPath()
	if found then
		activePath(found)
		refreshFromPath()
	elseif activePath() and not (activePath() :: Instance).Parent then
		activePath(nil)
		refreshFromPath()
	end
end)

actMove.Triggered:Connect(function()
	toolMode("Select")
	widget.Enabled = true
	syncMount()
end)
actAddPoint.Triggered:Connect(function()
	toolMode("AddPoint")
	widget.Enabled = true
	syncMount()
end)
actAddTangent.Triggered:Connect(function()
	toolMode("AddTangent")
	widget.Enabled = true
	syncMount()
end)
actDone.Triggered:Connect(function()
	widget.Enabled = false
	syncMount()
end)

toggleButton.Click:Connect(function()
	widget.Enabled = not widget.Enabled
	syncMount()
end)
widget:GetPropertyChangedSignal("Enabled"):Connect(syncMount)
syncMount()

plugin.Unloading:Connect(function()
	destroyHud()
	if unmount then
		local cleanup = unmount
		unmount = nil
		cleanup()
	end
	widget.Enabled = false
end)
