--!strict
--[[
    RusTudio Built-in Suite: Wind Direction / WindControl (Luau / Vide)

    1:1 recreation of Roblox Studio's WindControl built-in plugin
    (MainGui, Windhose, WindspeedDisplay, MakeTheme):
      - CoreGui ScreenGui HUD pinned top-right of the 3D edit viewport
        (210x120 translucent card).
      - Real 3D animated Windsock ViewportFrame (loads Studio's
        `rbxasset://models/WindControl/windhose.rbxm` via InsertService with a
        procedural 3D windsock + compass ring + arrow fallback), synced every
        frame to Workspace.CurrentCamera.CFrame and Workspace.GlobalWind, with
        click-and-drag Yaw/Pitch aiming.
      - Stacked clickable Speed / Yaw / Pitch tabs, inline editable value
        display ("0.0 sps", "90°", "0°"), and interactive slider using Studio's
        exact log10 0..1000 sps curve.
]]

local CoreGui = game:GetService("CoreGui")
local InsertService = game:GetService("InsertService")
local RunService = game:GetService("RunService")
local UserInputService = game:GetService("UserInputService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)

local create = Vide.create
local source = Vide.source
local action = Vide.action

local toolbar = plugin:CreateToolbar("wind_direction")
local toggleButton = toolbar:CreateButton(
	"wind_direction",
	"Show Wind Direction HUD and edit Workspace.GlobalWind",
	"",
	"Show Wind Direction"
)
toggleButton.ClickableWhenViewportHidden = true

type FieldMode = "Speed" | "Yaw" | "Pitch"

local function windToSpherical(v: Vector3): (number, number, number)
	local speed = v.Magnitude
	if speed < 1e-4 then
		return 0, 90, 0
	end
	local dir = v / speed
	local yaw = math.deg(math.atan2(dir.X, dir.Z))
	if yaw < 0 then
		yaw += 360
	end
	local pitch = math.deg(math.asin(math.clamp(dir.Y, -1, 1)))
	return speed, math.round(yaw) % 360, math.round(pitch)
end

local function sphericalToWind(speed: number, yawDeg: number, pitchDeg: number): Vector3
	if speed <= 0.001 then
		return Vector3.zero
	end
	local y = math.rad(yawDeg)
	local p = math.rad(pitchDeg)
	local cp = math.cos(p)
	return Vector3.new(math.sin(y) * cp, math.sin(p), math.cos(y) * cp) * speed
end

-- Exact log10 slider mapping from Studio's WindspeedDisplay.bin (-0.1..3.0 -> 0..1000 sps)
local function speedToSlider(s: number): number
	if s <= 0 then
		return 0
	end
	local lg = math.log10(math.clamp(s, 0.8, 1000))
	return math.clamp((lg + 0.1) / 3.1, 0, 1)
end

local function sliderToSpeed(t: number): number
	if t <= 0.01 then
		return 0
	end
	local lg = -0.1 + math.clamp(t, 0, 1) * 3.1
	local s = 10 ^ lg
	if s < 0.85 then
		return 0
	end
	return math.floor(s * 10 + 0.5) / 10
end

local function setGlobalWind(vec: Vector3, record: boolean)
	local rec = nil
	if record then
		pcall(function()
			rec = ChangeHistoryService:TryBeginRecording("WindControl", "Set GlobalWind")
		end)
	end
	local ok = pcall(function()
		workspace.GlobalWind = vec
	end)
	if rec then
		pcall(function()
			ChangeHistoryService:FinishRecording(
				rec,
				if ok then Enum.FinishRecordingOperation.Commit else Enum.FinishRecordingOperation.Cancel
			)
		end)
	end
end

local function buildFallbackWindRig(parent: Instance): {
	root: Model,
	arrow: Part,
	sockSegs: { Part },
}
	local model = Instance.new("Model")
	model.Name = "ProceduralWindhose"

	-- Compass ring (8 subtle grey segments on XZ plane)
	local ringRadius = 1.65
	for i = 0, 15 do
		local a = (i / 16) * math.pi * 2
		local seg = Instance.new("Part")
		seg.Name = "RingSeg" .. tostring(i)
		seg.Anchored = true
		seg.Size = Vector3.new(0.12, 0.08, 0.7)
		seg.Color = Color3.fromRGB(220, 224, 232)
		seg.Material = Enum.Material.SmoothPlastic
		seg.Transparency = 0.25
		seg.CFrame = CFrame.new(math.sin(a) * ringRadius, 0, math.cos(a) * ringRadius) * CFrame.Angles(0, a + math.pi * 0.5, 0)
		seg.Parent = model
	end

	-- Center hub
	local hub = Instance.new("Part")
	hub.Name = "Hub"
	hub.Shape = Enum.PartType.Ball
	hub.Anchored = true
	hub.Size = Vector3.new(0.42, 0.42, 0.42)
	hub.Color = Color3.fromRGB(240, 242, 246)
	hub.Material = Enum.Material.SmoothPlastic
	hub.CFrame = CFrame.identity
	hub.Parent = model

	-- Direction shaft/arrow
	local arrow = Instance.new("Part")
	arrow.Name = "WindArrow"
	arrow.Anchored = true
	arrow.Size = Vector3.new(0.14, 0.14, 1.55)
	arrow.Color = Color3.fromRGB(0, 162, 255)
	arrow.Material = Enum.Material.SmoothPlastic
	arrow.Parent = model

	-- Tapered orange-and-white windsock segments
	local sockSegs: { Part } = {}
	local segColors = {
		Color3.fromRGB(244, 112, 42),
		Color3.fromRGB(245, 245, 248),
		Color3.fromRGB(244, 112, 42),
		Color3.fromRGB(245, 245, 248),
	}
	for i = 1, 4 do
		local p = Instance.new("Part")
		p.Name = "Sock_" .. tostring(i)
		p.Shape = Enum.PartType.Cylinder
		p.Anchored = true
		local dia = 0.62 - (i - 1) * 0.11
		p.Size = Vector3.new(0.46, dia, dia)
		p.Color = segColors[i]
		p.Material = Enum.Material.SmoothPlastic
		p.Parent = model
		table.insert(sockSegs, p)
	end

	model.Parent = parent
	return { root = model, arrow = arrow, sockSegs = sockSegs }
end

local function WindControlHud()
	local s0, y0, p0 = windToSpherical(workspace.GlobalWind)
	local speed = source(s0)
	local yaw = source(y0)
	local pitch = source(p0)
	local mode = source("Speed" :: FieldMode)
	local editingText = source(false)

	local function pullFromWorkspace()
		local s, y, p = windToSpherical(workspace.GlobalWind)
		Vide.batch(function()
			speed(s)
			if s > 1e-4 then
				yaw(y)
				pitch(p)
			end
		end)
	end

	Vide.cleanup(workspace:GetPropertyChangedSignal("GlobalWind"):Connect(pullFromWorkspace))

	local function applySpherical(s: number, y: number, p: number, record: boolean?)
		local cs = math.clamp(s, 0, 1000)
		local cy = ((math.round(y) % 360) + 360) % 360
		local cp = math.clamp(math.round(p), -90, 90)
		Vide.batch(function()
			speed(cs)
			yaw(cy)
			pitch(cp)
		end)
		setGlobalWind(sphericalToWind(cs, cy, cp), record == true)
	end

	local displayText = Vide.derive(function()
		local m = mode()
		if m == "Speed" then
			return string.format("%.1f sps", speed())
		elseif m == "Yaw" then
			return string.format("%d°", yaw())
		else
			return string.format("%d°", pitch())
		end
	end)

	local sliderT = Vide.derive(function()
		local m = mode()
		if m == "Speed" then
			return speedToSlider(speed())
		elseif m == "Yaw" then
			return math.clamp(yaw() / 360, 0, 1)
		else
			return math.clamp((pitch() + 90) / 180, 0, 1)
		end
	end)

	local function setFromSliderT(t: number, record: boolean?)
		local m = mode()
		if m == "Speed" then
			applySpherical(sliderToSpeed(t), yaw(), pitch(), record)
		elseif m == "Yaw" then
			applySpherical(if speed() <= 0 then 12 else speed(), math.round(t * 360), pitch(), record)
		else
			applySpherical(if speed() <= 0 then 12 else speed(), yaw(), math.round(t * 180 - 90), record)
		end
	end

	local ACTIVE_COLOR = Color3.fromRGB(255, 255, 255)
	local INACTIVE_COLOR = Color3.fromRGB(214, 218, 224)

	local sliderTrackRef: Frame? = nil
	local draggingSlider = false
	local dragging3D = false
	local lastDragPos = Vector2.zero

	local function updateSliderFromX(mouseX: number)
		local track = sliderTrackRef
		if not track then return end
		local absPos = track.AbsolutePosition.X
		local absW = math.max(1, track.AbsoluteSize.X)
		local t = math.clamp((mouseX - absPos) / absW, 0, 1)
		setFromSliderT(t, false)
	end

	Vide.cleanup(UserInputService.InputChanged:Connect(function(input: InputObject)
		if input.UserInputType ~= Enum.UserInputType.MouseMovement then
			return
		end
		if draggingSlider then
			updateSliderFromX(input.Position.X)
		elseif dragging3D then
			local cur = Vector2.new(input.Position.X, input.Position.Y)
			local d = cur - lastDragPos
			lastDragPos = cur
			if d.Magnitude > 0.2 then
				applySpherical(
					if speed() <= 0 then 12 else speed(),
					yaw() + d.X * 1.4,
					pitch() - d.Y * 0.9,
					false
				)
			end
		end
	end))

	Vide.cleanup(UserInputService.InputEnded:Connect(function(input: InputObject)
		if input.UserInputType == Enum.UserInputType.MouseButton1 then
			if draggingSlider or dragging3D then
				draggingSlider = false
				dragging3D = false
				setGlobalWind(sphericalToWind(speed(), yaw(), pitch()), true)
			end
		end
	end))

	local function ModeTab(m: FieldMode, order: number)
		return create "TextButton" {
			Name = "Mode_" .. m,
			LayoutOrder = order,
			Size = UDim2.new(0, 64, 0, 16),
			BackgroundTransparency = 1,
			BorderSizePixel = 0,
			AutoButtonColor = false,
			Font = function()
				return if mode() == m then Enum.Font.SourceSansSemibold else Enum.Font.SourceSans
			end,
			TextSize = 13,
			TextXAlignment = Enum.TextXAlignment.Left,
			TextColor3 = function()
				return if mode() == m then ACTIVE_COLOR else INACTIVE_COLOR
			end,
			TextTransparency = function()
				return if mode() == m then 0 else 0.35
			end,
			Text = m,
			Activated = function()
				mode(m)
			end,
		}
	end

	return create "ScreenGui" {
		Name = "RusTudioWindControlGui",
		DisplayOrder = 10,
		ResetOnSpawn = false,
		{
			-- Studio WindControl translucent card (210x120 from MakeTheme.bin, clear of ViewCube)
			create "Frame" {
				Name = "BackgroundPane",
				AnchorPoint = Vector2.new(1, 0),
				Position = UDim2.new(1, -110, 0, 18),
				Size = UDim2.fromOffset(210, 120),
				BackgroundColor3 = Color3.fromRGB(102, 102, 102),
				BackgroundTransparency = 0.36,
				BorderSizePixel = 0,
				{
					create "UICorner" { CornerRadius = UDim.new(0, 6) },
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = Color3.fromRGB(150, 155, 165),
						Transparency = 0.65,
					},

					-- Real 3D Windsock ViewportFrame on the right side of the HUD card!
					create "ViewportFrame" {
						Name = "WindhoseViewport",
						Position = UDim2.fromOffset(72, -10),
						Size = UDim2.fromOffset(136, 114),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						Ambient = Color3.fromRGB(170, 175, 185),
						LightColor = Color3.fromRGB(255, 255, 255),
						LightDirection = Vector3.new(-1, -1, -1),
						action(function(vfInst)
							local vf = vfInst :: ViewportFrame
							local cam = Instance.new("Camera")
							cam.CameraType = Enum.CameraType.Scriptable
							cam.FieldOfView = 35
							cam.Parent = vf
							vf.CurrentCamera = cam

							-- Try loading Studio's real windhose.rbxm; fall back to procedural 3D windsock
							local loadedModel: Instance? = nil
							local okLoad, res = pcall(function()
								return InsertService:LoadLocalAsset("rbxasset://models/WindControl/windhose.rbxm")
							end)
							if okLoad and typeof(res) == "Instance" then
								res.Parent = vf
								loadedModel = res
							end

							local conePart: BasePart? = nil
							local selectStick: BasePart? = nil
							local selectArrow: BasePart? = nil
							local selectRing: BasePart? = nil
							local bones: { Bone } = {}
							local fallbackRig: { root: Model, arrow: Part, sockSegs: { Part } }? = nil

							if loadedModel then
								local c = loadedModel:FindFirstChild("Cone", true)
								if c and c:IsA("BasePart") then
									conePart = c
									for _, d in ipairs(c:GetDescendants()) do
										if d:IsA("Bone") then
											table.insert(bones, d)
										end
									end
									table.sort(bones, function(a, b) return a.Name < b.Name end)
								end
								local st = loadedModel:FindFirstChild("UnitSelectStickDownPositiveZ", true)
								if st and st:IsA("BasePart") then
									selectStick = st
									st.Color = Color3.fromRGB(48, 148, 255)
								end
								local ar = loadedModel:FindFirstChild("ArrowTipDownPositiveZ", true)
								if ar and ar:IsA("BasePart") then
									selectArrow = ar
								end
								local rg = loadedModel:FindFirstChild("UnitSelectRingXZ", true)
								if rg and rg:IsA("BasePart") then
									selectRing = rg
								end
							else
								fallbackRig = buildFallbackWindRig(vf)
							end

							local t0 = os.clock()
							local function updateVisual()
								local wcam = workspace.CurrentCamera
								local rot = if wcam then wcam.CFrame.Rotation else CFrame.Angles(-0.35, 0.55, 0)
								local look = rot.LookVector
								local dist = if loadedModel then 5.6 else 5.8
								local camPos = -look * dist + Vector3.new(0, 0.15, 0)
								cam.CFrame = CFrame.lookAt(camPos, Vector3.zero, rot.UpVector)
								vf.LightDirection = (rot:VectorToWorldSpace(Vector3.new(-1, -1, -1))).Unit

								local sp = speed()
								local yRad = math.rad(yaw())
								local pRad = math.rad(pitch())
								local windDir = Vector3.new(
									math.sin(yRad) * math.cos(pRad),
									math.sin(pRad),
									math.cos(yRad) * math.cos(pRad)
								)
								local dirCF = CFrame.lookAt(Vector3.zero, windDir)
								local elapsed = os.clock() - t0
								local strength = math.clamp(sp / 25, 0, 1)

								if loadedModel then
									-- Orient arrow/stick & windsock along current Yaw/Pitch
									if selectStick then
										selectStick.CFrame = dirCF * CFrame.new(0, 0, -0.75)
									end
									if selectArrow then
										selectArrow.CFrame = dirCF * CFrame.new(0, 0, -1.55)
									end
									if selectRing then
										selectRing.CFrame = CFrame.Angles(0, yRad, 0)
									end
									if conePart then
										-- Droop down when wind speed is near 0; point along windDir when blowing
										local droop = (1 - strength) * math.rad(-55)
										local flutter = math.sin(elapsed * (3 + strength * 9)) * math.rad(6 * strength)
										conePart.CFrame = CFrame.Angles(0, yRad, 0) * CFrame.Angles(pRad + droop, flutter, 0)
										for idx, bone in ipairs(bones) do
											local phase = elapsed * (4 + strength * 12) - idx * 0.7
											local amp = math.rad(4 + 10 * strength) * (idx / math.max(1, #bones))
											pcall(function()
												bone.Transform = CFrame.Angles(
													math.sin(phase) * amp * 0.6,
													math.cos(phase * 1.15) * amp,
													0
												)
											end)
										end
									end
								elseif fallbackRig then
									local rig = fallbackRig
									rig.arrow.CFrame = dirCF * CFrame.new(0, 0, -0.82)
									local droopDir = (windDir * (0.25 + 0.75 * strength) + Vector3.new(0, -(1 - strength) * 0.9, 0)).Unit
									local baseCF = CFrame.lookAt(Vector3.zero, droopDir)
									for i, seg in ipairs(rig.sockSegs) do
										local phase = elapsed * (4 + strength * 10) - i * 0.75
										local swayX = math.sin(phase) * 0.08 * strength * i
										local swayY = math.cos(phase * 1.2) * 0.05 * strength * i
										local segPos = baseCF:PointToWorldSpace(Vector3.new(swayX, swayY, -(0.45 * i)))
										seg.CFrame = CFrame.new(segPos) * baseCF.Rotation * CFrame.Angles(0, math.pi * 0.5, 0)
									end
								end
							end

							updateVisual()
							Vide.cleanup(RunService.Heartbeat:Connect(updateVisual))
						end),
						{
							-- Transparent drag overlay over the 3D windsock to aim Yaw/Pitch directly
							create "TextButton" {
								Name = "DragOverlay",
								Size = UDim2.fromScale(1, 1),
								BackgroundTransparency = 1,
								Text = "",
								AutoButtonColor = false,
								InputBegan = function(input: InputObject)
									if input.UserInputType == Enum.UserInputType.MouseButton1 then
										dragging3D = true
										lastDragPos = Vector2.new(input.Position.X, input.Position.Y)
									end
								end,
							},
						},
					},

					-- Top-left stacked mode switcher: Speed / Yaw / Pitch
					create "Frame" {
						Name = "ModeStack",
						Position = UDim2.fromOffset(12, 10),
						Size = UDim2.fromOffset(64, 50),
						BackgroundTransparency = 1,
						{
							create "UIListLayout" {
								FillDirection = Enum.FillDirection.Vertical,
								SortOrder = Enum.SortOrder.LayoutOrder,
								Padding = UDim.new(0, 1),
							},
							ModeTab("Speed", 1),
							ModeTab("Yaw", 2),
							ModeTab("Pitch", 3),
						},
					},

					-- Main bold value readout (editable on click, exact "0.0 sps" format)
					create "TextBox" {
						Name = "ValueDisplay",
						Position = UDim2.fromOffset(12, 62),
						Size = UDim2.fromOffset(96, 26),
						BackgroundTransparency = function()
							return if editingText() then 0.65 else 1
						end,
						BackgroundColor3 = Color3.fromRGB(30, 32, 38),
						BorderSizePixel = 0,
						ClearTextOnFocus = false,
						Font = Enum.Font.SourceSansBold,
						TextSize = 20,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = ACTIVE_COLOR,
						Text = displayText,
						Focused = function()
							editingText(true)
						end,
						action(function(tbInst)
							local tb = tbInst :: TextBox
							Vide.cleanup(tb.FocusLost:Connect(function(enterPressed)
								editingText(false)
								if not enterPressed then
									tb.Text = displayText()
									return
								end
								local n = tonumber((string.gsub(tb.Text, "[^%d%.%-]", "")))
								if n then
									local m = mode()
									if m == "Speed" then
										applySpherical(n, yaw(), pitch(), true)
									elseif m == "Yaw" then
										applySpherical(if speed() <= 0 then 12 else speed(), n, pitch(), true)
									else
										applySpherical(if speed() <= 0 then 12 else speed(), yaw(), n, true)
									end
								else
									tb.Text = displayText()
								end
							end))
						end),
						{ create "UICorner" { CornerRadius = UDim.new(0, 3) } },
					},

					-- Bottom interactive Studio slider bar with white circular knob
					create "TextButton" {
						Name = "SliderHitArea",
						Position = UDim2.fromOffset(12, 92),
						Size = UDim2.new(1, -24, 0, 20),
						BackgroundTransparency = 1,
						Text = "",
						AutoButtonColor = false,
						InputBegan = function(input: InputObject)
							if input.UserInputType == Enum.UserInputType.MouseButton1 then
								draggingSlider = true
								updateSliderFromX(input.Position.X)
							end
						end,
						{
							create "Frame" {
								Name = "SliderTrack",
								AnchorPoint = Vector2.new(0, 0.5),
								Position = UDim2.new(0, 0, 0.5, 0),
								Size = UDim2.new(1, 0, 0, 4),
								BackgroundColor3 = Color3.fromRGB(40, 43, 48),
								BorderSizePixel = 0,
								action(function(inst)
									sliderTrackRef = inst :: Frame
								end),
								{
									create "UICorner" { CornerRadius = UDim.new(1, 0) },
									create "Frame" {
										Name = "SliderFill",
										Size = function()
											return UDim2.fromScale(sliderT(), 1)
										end,
										BackgroundColor3 = Color3.fromRGB(255, 255, 255),
										BorderSizePixel = 0,
										{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
									},
									create "Frame" {
										Name = "SliderKnob",
										AnchorPoint = Vector2.new(0.5, 0.5),
										Position = function()
											return UDim2.new(sliderT(), 0, 0.5, 0)
										end,
										Size = UDim2.fromOffset(14, 14),
										BackgroundColor3 = Color3.fromRGB(255, 255, 255),
										BorderSizePixel = 0,
										{
											create "UICorner" { CornerRadius = UDim.new(1, 0) },
											create "UIStroke" {
												ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
												Thickness = 1,
												Color = Color3.fromRGB(90, 94, 102),
											},
										},
									},
								},
							},
						},
					},
				},
			},
		},
	}
end

local enabled = false
local unmount: (() -> ())? = nil

local function mount()
	if unmount then return end
	unmount = Vide.mount(WindControlHud, CoreGui)
end

local function teardown()
	if unmount then
		local fn = unmount
		unmount = nil
		fn()
	end
end

local function sync()
	toggleButton:SetActive(enabled)
	if enabled then mount() else teardown() end
end

toggleButton.Click:Connect(function()
	enabled = not enabled
	sync()
end)

plugin.Unloading:Connect(teardown)
sync()
