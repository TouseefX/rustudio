--!strict
--[[
    RusTudio Built-in Suite: Material Manager (Luau / Vide)

    1:1 recreation of Roblox Studio's MaterialManager built-in plugin
    (MaterialPreview, TopBar, SideBar, MaterialGrid):
      - Real 3D PBR lit sphere previews rendered sequentially through a SINGLE
        shared offscreen ViewportFrame via CaptureSnapshotAsync() and cached as
        lightweight 96x96 ImageLabels (zero multi-ViewportFrame VRAM spike).
      - TopBar with Studio Create Variant (+), Apply to Selection, centered
        "Search Materials" bar, and Grid/List toggle.
      - Left Category tree (Materials -> Ceramic, Concrete, Fabric, Ground,
        Metal, Organic, Plastic, Rock, Synthetic, Wood, Variants).
      - Undoable ChangeHistoryService apply & MaterialVariant creation.
]]

local Selection = game:GetService("Selection")
local MaterialService = game:GetService("MaterialService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local values = Vide.values
local changed = Vide.changed
local action = Vide.action

local toolbar = plugin:CreateToolbar("material_manager")
local toggleButton = toolbar:CreateButton(
	"material_manager",
	"Browse and apply Roblox materials and MaterialVariants",
	"",
	"Material Manager"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"MaterialManager",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Bottom, false, false, 900, 260, 420, 180)
)
widget.Title = "Material Manager"
widget.Name = "RusTudioMaterialManager"

local ICON_CREATE = "rbxasset://textures/MaterialManager/Create_New_Variant.png"
local ICON_APPLY = "rbxasset://textures/MaterialManager/Apply_to_Selection.png"
local ICON_GRID = "rbxasset://textures/MaterialManager/Grid_DT.png"
local ICON_LIST = "rbxasset://textures/MaterialManager/List_DT.png"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

-- Session-wide thumbnail cache (key -> { uri, refined })
local snapshotCache: { [string]: string } = {}
local refinedCache: { [string]: boolean } = {}

type MaterialEntry = {
	key: string,
	name: string,
	displayName: string,
	category: string,
	material: Enum.Material,
	variantName: string,
	swatch: Color3,
}

local CATEGORIES = {
	"All",
	"Ceramic",
	"Concrete",
	"Fabric",
	"Ground",
	"Metal",
	"Organic",
	"Plastic",
	"Rock",
	"Synthetic",
	"Wood",
	"Variants",
}

local BUILTIN_MATERIALS: { { name: string, display: string, category: string, swatch: Color3 } } = {
	{ name = "Asphalt", display = "Asphalt", category = "Concrete", swatch = Color3.fromRGB(68, 70, 74) },
	{ name = "Basalt", display = "Basalt", category = "Rock", swatch = Color3.fromRGB(80, 82, 86) },
	{ name = "Brick", display = "Brick", category = "Rock", swatch = Color3.fromRGB(130, 126, 124) },
	{ name = "Cardboard", display = "Cardboard", category = "Synthetic", swatch = Color3.fromRGB(142, 140, 142) },
	{ name = "Carpet", display = "Carpet", category = "Fabric", swatch = Color3.fromRGB(152, 152, 155) },
	{ name = "CeramicTiles", display = "Ceramic Tiles", category = "Ceramic", swatch = Color3.fromRGB(172, 174, 178) },
	{ name = "ClayRoofTiles", display = "Clay Roof Tiles", category = "Ceramic", swatch = Color3.fromRGB(128, 120, 118) },
	{ name = "Cobblestone", display = "Cobblestone", category = "Rock", swatch = Color3.fromRGB(135, 130, 126) },
	{ name = "Concrete", display = "Concrete", category = "Concrete", swatch = Color3.fromRGB(148, 150, 154) },
	{ name = "CorrodedMetal", display = "Corroded Metal", category = "Metal", swatch = Color3.fromRGB(138, 96, 68) },
	{ name = "CrackedLava", display = "Cracked Lava", category = "Rock", swatch = Color3.fromRGB(120, 62, 46) },
	{ name = "DiamondPlate", display = "Diamond Plate", category = "Metal", swatch = Color3.fromRGB(144, 148, 156) },
	{ name = "Fabric", display = "Fabric", category = "Fabric", swatch = Color3.fromRGB(146, 146, 150) },
	{ name = "Foil", display = "Foil", category = "Metal", swatch = Color3.fromRGB(184, 188, 196) },
	{ name = "ForceField", display = "Force Field", category = "Synthetic", swatch = Color3.fromRGB(92, 148, 224) },
	{ name = "Glacier", display = "Glacier", category = "Ground", swatch = Color3.fromRGB(156, 182, 198) },
	{ name = "Glass", display = "Glass", category = "Synthetic", swatch = Color3.fromRGB(168, 194, 214) },
	{ name = "Granite", display = "Granite", category = "Rock", swatch = Color3.fromRGB(116, 118, 122) },
	{ name = "Grass", display = "Grass", category = "Organic", swatch = Color3.fromRGB(118, 138, 108) },
	{ name = "Ground", display = "Ground", category = "Ground", swatch = Color3.fromRGB(126, 116, 104) },
	{ name = "Ice", display = "Ice", category = "Ground", swatch = Color3.fromRGB(162, 188, 206) },
	{ name = "LeafyGrass", display = "Leafy Grass", category = "Organic", swatch = Color3.fromRGB(110, 132, 98) },
	{ name = "Leather", display = "Leather", category = "Fabric", swatch = Color3.fromRGB(116, 106, 98) },
	{ name = "Limestone", display = "Limestone", category = "Rock", swatch = Color3.fromRGB(168, 164, 154) },
	{ name = "Marble", display = "Marble", category = "Rock", swatch = Color3.fromRGB(194, 194, 196) },
	{ name = "Metal", display = "Metal", category = "Metal", swatch = Color3.fromRGB(162, 166, 174) },
	{ name = "Mud", display = "Mud", category = "Ground", swatch = Color3.fromRGB(98, 90, 82) },
	{ name = "Neon", display = "Neon", category = "Synthetic", swatch = Color3.fromRGB(224, 226, 232) },
	{ name = "Pavement", display = "Pavement", category = "Concrete", swatch = Color3.fromRGB(142, 144, 148) },
	{ name = "Pebble", display = "Pebble", category = "Rock", swatch = Color3.fromRGB(134, 134, 136) },
	{ name = "Plaster", display = "Plaster", category = "Concrete", swatch = Color3.fromRGB(178, 178, 180) },
	{ name = "Plastic", display = "Plastic", category = "Plastic", swatch = Color3.fromRGB(163, 162, 165) },
	{ name = "Rock", display = "Rock", category = "Rock", swatch = Color3.fromRGB(120, 120, 122) },
	{ name = "RoofShingles", display = "Roof Shingles", category = "Ceramic", swatch = Color3.fromRGB(108, 104, 102) },
	{ name = "Rubber", display = "Rubber", category = "Synthetic", swatch = Color3.fromRGB(90, 92, 96) },
	{ name = "Salt", display = "Salt", category = "Ground", swatch = Color3.fromRGB(206, 206, 208) },
	{ name = "Sand", display = "Sand", category = "Ground", swatch = Color3.fromRGB(178, 170, 150) },
	{ name = "Sandstone", display = "Sandstone", category = "Rock", swatch = Color3.fromRGB(166, 152, 134) },
	{ name = "Slate", display = "Slate", category = "Rock", swatch = Color3.fromRGB(106, 110, 116) },
	{ name = "SmoothPlastic", display = "Smooth Plastic", category = "Plastic", swatch = Color3.fromRGB(168, 168, 172) },
	{ name = "Snow", display = "Snow", category = "Ground", swatch = Color3.fromRGB(218, 222, 226) },
	{ name = "Wood", display = "Wood", category = "Wood", swatch = Color3.fromRGB(146, 130, 112) },
	{ name = "WoodPlanks", display = "Wood Planks", category = "Wood", swatch = Color3.fromRGB(140, 124, 106) },
}

local function resolveEnumMaterial(name: string): Enum.Material?
	local ok, mat = pcall(function()
		return (Enum.Material :: any)[name]
	end)
	if ok and mat ~= nil then
		return mat :: Enum.Material
	end
	return nil
end

local function buildCatalog(): { MaterialEntry }
	local out: { MaterialEntry } = {}
	for _, info in ipairs(BUILTIN_MATERIALS) do
		local m = resolveEnumMaterial(info.name)
		if m then
			table.insert(out, {
				key = "mat:" .. info.name,
				name = info.name,
				displayName = info.display,
				category = info.category,
				material = m,
				variantName = "",
				swatch = info.swatch,
			})
		end
	end
	for _, desc in ipairs(MaterialService:GetDescendants()) do
		if desc:IsA("MaterialVariant") then
			table.insert(out, {
				key = "var:" .. desc.Name,
				name = desc.Name,
				displayName = desc.Name,
				category = "Variants",
				material = desc.BaseMaterial,
				variantName = desc.Name,
				swatch = Color3.fromRGB(148, 162, 186),
			})
		end
	end
	return out
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
		warn("[MaterialManager] " .. tostring(err))
	end
end

local function selectedParts(): { BasePart }
	local out = {}
	for _, inst in ipairs(Selection:Get()) do
		if inst:IsA("BasePart") then
			table.insert(out, inst)
		elseif inst:IsA("Model") then
			for _, d in ipairs(inst:GetDescendants()) do
				if d:IsA("BasePart") then
					table.insert(out, d)
				end
			end
		end
	end
	return out
end

local function MaterialSphereTile(
	entry: MaterialEntry,
	selected: () -> boolean,
	gridMode: () -> boolean,
	thumbRev: () -> number,
	onSelect: () -> (),
	onApply: () -> (),
	order: () -> number
)
	local hovered = source(false)
	local lastClick = 0

	local thumbUri = Vide.derive(function()
		local _ = thumbRev()
		return snapshotCache[entry.key] or ""
	end)

	return create "TextButton" {
		Name = "Mat_" .. entry.name,
		LayoutOrder = order,
		AutoButtonColor = false,
		Text = "",
		BorderSizePixel = 0,
		BackgroundColor3 = function()
			if selected() then
				return Color3.fromRGB(36, 42, 54)
			elseif hovered() then
				return C_HOVER()
			end
			return Color3.fromRGB(31, 33, 39)
		end,
		MouseEnter = function() hovered(true) end,
		MouseLeave = function() hovered(false) end,
		Activated = function()
			onSelect()
			local now = os.clock()
			if now - lastClick < 0.35 then
				onApply()
			end
			lastClick = now
		end,
		{
			create "UICorner" { CornerRadius = UDim.new(0, 4) },
			create "UIStroke" {
				ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
				Thickness = function()
					return if selected() then 2 else 1
				end,
				Color = function()
					return if selected() then Color3.fromRGB(0, 162, 255) else Color3.fromRGB(52, 55, 64)
				end,
			},

			-- Lightweight placeholder sphere disc while snapshot renders
			create "Frame" {
				AnchorPoint = function()
					return if gridMode() then Vector2.new(0.5, 0) else Vector2.new(0, 0.5)
				end,
				Position = function()
					return if gridMode() then UDim2.new(0.5, 0, 0, 8) else UDim2.new(0, 8, 0.5, 0)
				end,
				Size = function()
					return if gridMode() then UDim2.fromOffset(66, 66) else UDim2.fromOffset(18, 18)
				end,
				BackgroundColor3 = entry.swatch,
				BackgroundTransparency = function()
					return if thumbUri() ~= "" then 1 else 0
				end,
				BorderSizePixel = 0,
				{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
			},

			-- Cached 3D PBR lit sphere PNG captured from the single shared ViewportFrame
			create "ImageLabel" {
				Name = "SphereThumb",
				AnchorPoint = function()
					return if gridMode() then Vector2.new(0, 0) else Vector2.new(0, 0.5)
				end,
				Position = function()
					return if gridMode() then UDim2.fromOffset(6, 4) else UDim2.new(0, 6, 0.5, 0)
				end,
				Size = function()
					return if gridMode() then UDim2.new(1, -12, 1, -24) else UDim2.fromOffset(22, 22)
				end,
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				ScaleType = Enum.ScaleType.Fit,
				Image = thumbUri,
			},

			-- Material name label (underneath in Grid View, beside icon in List View)
			create "TextLabel" {
				Name = "TileLabel",
				Position = function()
					return if gridMode() then UDim2.new(0, 5, 1, -20) else UDim2.fromOffset(34, 0)
				end,
				Size = function()
					return if gridMode() then UDim2.new(1, -10, 0, 17) else UDim2.new(1, -120, 1, 0)
				end,
				BackgroundTransparency = 1,
				Font = Enum.Font.SourceSans,
				TextSize = function()
					return if gridMode() then 12 else 13
				end,
				TextTruncate = Enum.TextTruncate.AtEnd,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextColor3 = C_TEXT,
				Text = entry.displayName,
			},

			-- Right-aligned category column in List View
			create "TextLabel" {
				Name = "CategoryLabel",
				Position = UDim2.new(1, -86, 0, 0),
				Size = UDim2.new(0, 78, 1, 0),
				BackgroundTransparency = 1,
				Visible = function()
					return not gridMode()
				end,
				Font = Enum.Font.SourceSans,
				TextSize = 12,
				TextXAlignment = Enum.TextXAlignment.Right,
				TextColor3 = C_DIM,
				Text = entry.category,
			},
		},
	}
end

local function MaterialManagerApp()
	local catalog = source(buildCatalog())
	local activeCategory = source("All")
	local query = source("")
	local selectedKey = source("mat:Asphalt")
	local gridMode = source(true)
	local thumbRev = source(0)

	local alive = true
	Vide.cleanup(function()
		alive = false
	end)

	local function refreshCatalog()
		catalog(buildCatalog())
	end

	Vide.cleanup(MaterialService.DescendantAdded:Connect(function(inst)
		if inst:IsA("MaterialVariant") then
			refreshCatalog()
		end
	end))
	Vide.cleanup(MaterialService.DescendantRemoving:Connect(function(inst)
		if inst:IsA("MaterialVariant") then
			task.defer(refreshCatalog)
		end
	end))

	local filtered = Vide.derive(function(): { MaterialEntry }
		local cat = activeCategory()
		local q = string.lower((string.gsub(query(), "^%s*(.-)%s*$", "%1")))
		local out = {}
		for _, e in ipairs(catalog()) do
			local catOk = (cat == "All") or (e.category == cat)
			local qOk = (q == "") or (string.find(string.lower(e.displayName .. " " .. e.name), q, 1, true) ~= nil)
			if catOk and qOk then
				table.insert(out, e)
			end
		end
		return out
	end)

	local function findSelectedEntry(): MaterialEntry?
		local k = selectedKey()
		for _, e in ipairs(catalog()) do
			if e.key == k then
				return e
			end
		end
		return nil
	end

	local function applyEntry(entry: MaterialEntry?)
		if not entry then return end
		local parts = selectedParts()
		if #parts == 0 then return end
		withRecording("MaterialManagerApply", `Apply {entry.displayName}`, function()
			for _, p in ipairs(parts) do
				p.Material = entry.material :: any
				pcall(function()
					p.MaterialVariant = entry.variantName
				end)
			end
		end)
	end

	local function createNewVariant()
		local sel = findSelectedEntry()
		local baseMat = if sel then sel.material else Enum.Material.Plastic
		local idx = 1
		for _, d in ipairs(MaterialService:GetDescendants()) do
			if d:IsA("MaterialVariant") then
				idx += 1
			end
		end
		local name = `MaterialVariant{idx}`
		withRecording("MaterialManagerCreate", `Create {name}`, function()
			local mv = Instance.new("MaterialVariant")
			mv.Name = name
			mv.BaseMaterial = baseMat :: any
			mv.Parent = MaterialService
			Selection:Set({ mv })
		end)
		refreshCatalog()
		selectedKey("var:" .. name)
		activeCategory("Variants")
	end

	-- Sequential thumbnail baker using ONE single offscreen 96x96 ViewportFrame
	local bakerRunning = false
	local sharedVfRef: ViewportFrame? = nil
	local sharedSphereRef: Part? = nil

	local function ensureBake()
		if bakerRunning then return end
		local vf = sharedVfRef
		local sphere = sharedSphereRef
		if not vf or not sphere then return end
		bakerRunning = true
		task.spawn(function()
			local function bakeOne(entry: MaterialEntry, refine: boolean): boolean
				if not alive or not vf.Parent or not sphere.Parent then
					return false
				end
				sphere.Material = entry.material :: any
				pcall(function()
					sphere.MaterialVariant = entry.variantName
				end)
				task.wait()
				if refine then
					task.wait(0.06)
				end
				if not alive then return false end
				local ok, uri = pcall(function()
					return (vf :: any):CaptureSnapshotAsync()
				end)
				if ok and type(uri) == "string" and #uri > 0 then
					snapshotCache[entry.key] = uri
					if refine then
						refinedCache[entry.key] = true
					end
					thumbRev(thumbRev() + 1)
					return true
				end
				return false
			end

			-- Pass 1: fast initial capture (triggers async PBR pack loads)
			local didFirst = false
			while alive do
				local nextEntry: MaterialEntry? = nil
				for _, e in ipairs(filtered()) do
					if not snapshotCache[e.key] then
						nextEntry = e
						break
					end
				end
				if not nextEntry then break end
				if not bakeOne(nextEntry, false) then
					bakerRunning = false
					return
				end
				didFirst = true
			end

			-- Pass 2: once async PBR packs have decoded, re-snapshot crisp PBR spheres
			if didFirst and alive then
				task.wait(0.45)
			end
			while alive do
				local nextEntry: MaterialEntry? = nil
				for _, e in ipairs(filtered()) do
					if not refinedCache[e.key] then
						nextEntry = e
						break
					end
				end
				if not nextEntry then break end
				if not bakeOne(nextEntry, true) then
					break
				end
			end
			bakerRunning = false
		end)
	end

	Vide.effect(function()
		local _ = filtered()
		task.defer(ensureBake)
	end)

	return create "Frame" {
		Name = "MaterialManagerRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- Single shared offscreen 96x96 ViewportFrame (only 1 GPU target total!)
			create "ViewportFrame" {
				Name = "SharedSphereBaker",
				Position = UDim2.fromOffset(-2000, -2000),
				Size = UDim2.fromOffset(96, 96),
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				Ambient = Color3.fromRGB(145, 148, 156),
				LightColor = Color3.fromRGB(255, 255, 255),
				LightDirection = Vector3.new(-0.7, -0.8, -0.6),
				action(function(vfInst)
					local vf = vfInst :: ViewportFrame
					local cam = Instance.new("Camera")
					cam.FieldOfView = 40
					cam.CFrame = CFrame.lookAt(Vector3.new(0, 0, 3.1), Vector3.zero)
					cam.Parent = vf
					vf.CurrentCamera = cam

					local sphere = Instance.new("Part")
					sphere.Shape = Enum.PartType.Ball
					sphere.Size = Vector3.new(2, 2, 2)
					sphere.Color = Color3.fromRGB(163, 162, 165)
					sphere.Material = Enum.Material.Asphalt
					sphere.CFrame = CFrame.Angles(0, math.pi * 0.25, 0) * CFrame.Angles(math.pi * 0.25, 0, 0)
					sphere.Anchored = true
					sphere.Parent = vf

					sharedVfRef = vf
					sharedSphereRef = sphere
					task.defer(ensureBake)
				end),
			},

			-- TopBar matching Studio's Material Manager
			create "Frame" {
				Name = "TopBar",
				Size = UDim2.new(1, 0, 0, 36),
				BackgroundColor3 = C_HEADER,
				BorderSizePixel = 0,
				{
					create "Frame" {
						Position = UDim2.new(0, 0, 1, -1),
						Size = UDim2.new(1, 0, 0, 1),
						BackgroundColor3 = C_BORDER,
						BorderSizePixel = 0,
					},

					create "ImageButton" {
						Name = "CreateVariantBtn",
						Position = UDim2.fromOffset(8, 5),
						Size = UDim2.fromOffset(26, 26),
						AutoButtonColor = false,
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						Image = ICON_CREATE,
						ScaleType = Enum.ScaleType.Fit,
						ImageColor3 = C_TEXT,
						Activated = createNewVariant,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},

					create "ImageButton" {
						Name = "ApplySelectedBtn",
						Position = UDim2.fromOffset(38, 5),
						Size = UDim2.fromOffset(26, 26),
						AutoButtonColor = false,
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						Image = ICON_APPLY,
						ScaleType = Enum.ScaleType.Fit,
						ImageColor3 = C_TEXT,
						Activated = function()
							applyEntry(findSelectedEntry())
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},

					create "Frame" {
						Name = "SearchWrap",
						AnchorPoint = Vector2.new(0.5, 0.5),
						Position = UDim2.new(0.5, 0, 0.5, 0),
						Size = UDim2.new(0.52, 0, 0, 24),
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
								Position = UDim2.fromOffset(10, 0),
								Size = UDim2.new(1, -16, 1, 0),
								BackgroundTransparency = 1,
								BorderSizePixel = 0,
								ClearTextOnFocus = false,
								Font = Enum.Font.SourceSans,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = C_TEXT,
								PlaceholderColor3 = C_DIM,
								PlaceholderText = "Search Materials",
								Text = "",
								changed("Text", function(t) query(t) end),
							},
						},
					},

					create "ImageButton" {
						Name = "GridToggleBtn",
						AnchorPoint = Vector2.new(1, 0.5),
						Position = UDim2.new(1, -10, 0.5, 0),
						Size = UDim2.fromOffset(26, 24),
						AutoButtonColor = false,
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						Image = function()
							return if gridMode() then ICON_GRID else ICON_LIST
						end,
						ScaleType = Enum.ScaleType.Fit,
						ImageColor3 = C_TEXT,
						Activated = function()
							gridMode(not gridMode())
						end,
						{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
					},
				},
			},

			-- Left Category Tree (SideBar)
			create "ScrollingFrame" {
				Name = "CategorySideBar",
				Position = UDim2.fromOffset(0, 36),
				Size = UDim2.new(0, 168, 1, -36),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				ScrollBarThickness = 5,
				ScrollBarImageColor3 = C_SCROLL,
				CanvasSize = UDim2.fromOffset(0, 0),
				AutomaticCanvasSize = Enum.AutomaticSize.Y,
				{
					create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
					create "UIPadding" {
						PaddingTop = UDim.new(0, 4),
						PaddingBottom = UDim.new(0, 6),
					},
					(function()
						local rows = {}
						for i, cat in ipairs(CATEGORIES) do
							local isRoot = (cat == "All")
							table.insert(
								rows,
								create "TextButton" {
									Name = "Cat_" .. cat,
									LayoutOrder = i,
									Size = UDim2.new(1, 0, 0, 24),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									BackgroundColor3 = function()
										return if activeCategory() == cat then Color3.fromRGB(0, 102, 255) else C_BG()
									end,
									Font = if isRoot then Enum.Font.SourceSansSemibold else Enum.Font.SourceSans,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = function()
										return if activeCategory() == cat then Color3.fromRGB(255, 255, 255) else C_TEXT()
									end,
									Text = if isRoot then "  ▾ Materials" else "      " .. cat,
									Activated = function()
										activeCategory(cat)
									end,
								}
							)
						end
						return rows
					end)(),
				},
			},

			-- Vertical divider
			create "Frame" {
				Position = UDim2.fromOffset(168, 36),
				Size = UDim2.new(0, 1, 1, -36),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- Right 3D Sphere Material Grid
			create "ScrollingFrame" {
				Name = "MaterialGrid",
				Position = UDim2.fromOffset(169, 36),
				Size = UDim2.new(1, -169, 1, -36),
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				CanvasSize = UDim2.fromOffset(0, 0),
				AutomaticCanvasSize = Enum.AutomaticSize.Y,
				ScrollBarThickness = 6,
				ScrollBarImageColor3 = C_SCROLL,
				{
					create "UIPadding" {
						PaddingLeft = UDim.new(0, 8),
						PaddingRight = UDim.new(0, 8),
						PaddingTop = UDim.new(0, 6),
						PaddingBottom = UDim.new(0, 8),
					},
					create "UIGridLayout" {
						CellSize = function()
							return if gridMode() then UDim2.fromOffset(102, 102) else UDim2.new(1, -4, 0, 30)
						end,
						CellPadding = function()
							return if gridMode() then UDim2.fromOffset(6, 6) else UDim2.fromOffset(0, 3)
						end,
						SortOrder = Enum.SortOrder.LayoutOrder,
					},
					values(filtered, function(entry: MaterialEntry, idx: () -> number)
						return MaterialSphereTile(
							entry,
							function() return selectedKey() == entry.key end,
							gridMode,
							thumbRev,
							function() selectedKey(entry.key) end,
							function() applyEntry(entry) end,
							idx
						)
					end),
				},
			},
		},
	}
end

local unmount: (() -> ())? = nil
local function mount()
	if unmount then return end
	unmount = Vide.mount(MaterialManagerApp, widget)
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
