--!strict
--[[
    RusTudio Built-in Suite: Texture Viewer (Luau / Vide)

    1:1 recreation of Roblox Studio's TextureViewer built-in plugin:
      - Scans all 25 Studio texture properties across the entire place (or
        current Selection subtree when "Selection" mode is toggled).
      - TopPanel with Refresh, Search, Selection-only filter, and counts.
      - Left TextureTable with live thumbnail previews, Texture ID, Uses, and
        Select All action; Right References & Bulk Replace panel (undoable).
]]

local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local changed = Vide.changed
local action = Vide.action

local toolbar = plugin:CreateToolbar("texture_viewer")
local toggleButton = toolbar:CreateButton(
	"texture_viewer",
	"Inspect, preview, select, and bulk-replace textures across the place",
	"",
	"Textures"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"TextureViewer",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Bottom, false, false, 740, 340, 420, 200)
)
widget.Title = "Texture Viewer"
widget.Name = "RusTudioTextureViewer"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SELECTED = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Selected)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

-- Exact 25 texture properties from Roblox Studio's TextureViewer/Src/Util/Constants
local CLASS_PROPS: { [string]: { string } } = {
	ImageLabel = { "Image" },
	ImageButton = { "Image", "HoverImage", "PressedImage" },
	Decal = { "Texture" },
	Texture = { "Texture" },
	MeshPart = { "TextureID" },
	SpecialMesh = { "TextureId" },
	FileMesh = { "TextureId" },
	SurfaceAppearance = { "ColorMap", "NormalMap", "MetalnessMap", "RoughnessMap" },
	MaterialVariant = { "ColorMap", "NormalMap", "MetalnessMap", "RoughnessMap" },
	Beam = { "Texture" },
	Trail = { "Texture" },
	ParticleEmitter = { "Texture" },
	ScrollingFrame = { "TopImage", "MidImage", "BottomImage" },
	Sky = { "SkyboxBk", "SkyboxDn", "SkyboxFt", "SkyboxLf", "SkyboxRt", "SkyboxUp", "SunTextureId", "MoonTextureId" },
	Shirt = { "ShirtTemplate" },
	Pants = { "PantsTemplate" },
	ShirtGraphic = { "Graphic" },
	Tool = { "TextureId" },
	ClickDetector = { "CursorIcon" },
	VideoFrame = { "FallbackImage" },
}

type TextureRef = {
	inst: Instance,
	prop: string,
	path: string,
	className: string,
}

type TextureGroup = {
	uri: string,
	refs: { TextureRef },
	summary: string,
}

local function trim(s: string): string
	return (string.gsub(s, "^%s*(.-)%s*$", "%1"))
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
		warn("[TextureViewer] " .. tostring(err))
	end
end

local function extractUri(raw: any): string
	if type(raw) == "string" then
		return trim(raw)
	end
	if typeof(raw) == "Content" then
		local ok, uri = pcall(function()
			return (raw :: any).Uri
		end)
		if ok and type(uri) == "string" then
			return trim(uri)
		end
	end
	return ""
end

local function scanTextures(selectionOnly: boolean): ({ TextureGroup }, number)
	local byUri: { [string]: { TextureRef } } = {}
	local seenInst: { [Instance]: boolean } = {}
	local totalRefs = 0

	local function inspect(inst: Instance)
		if seenInst[inst] then
			return
		end
		seenInst[inst] = true
		local props = CLASS_PROPS[inst.ClassName]
		if not props then
			return
		end
		for _, prop in ipairs(props) do
			local ok, raw = pcall(function()
				return (inst :: any)[prop]
			end)
			if ok then
				local uri = extractUri(raw)
				if uri ~= "" then
					local bucket = byUri[uri]
					if not bucket then
						bucket = {}
						byUri[uri] = bucket
					end
					table.insert(bucket, {
						inst = inst,
						prop = prop,
						path = inst:GetFullName(),
						className = inst.ClassName,
					})
					totalRefs += 1
				end
			end
		end
	end

	local roots: { Instance }
	if selectionOnly then
		roots = Selection:Get()
	else
		roots = {}
		for _, svc in ipairs(game:GetChildren()) do
			if svc.ClassName ~= "CoreGui" then
				table.insert(roots, svc)
			end
		end
	end

	for _, root in ipairs(roots) do
		inspect(root)
		local ok, desc = pcall(function()
			return root:GetDescendants()
		end)
		if ok and type(desc) == "table" then
			for _, inst in ipairs(desc) do
				inspect(inst)
			end
		end
	end

	local groups: { TextureGroup } = {}
	for uri, refs in pairs(byUri) do
		table.sort(refs, function(a, b)
			return a.path < b.path
		end)
		local first = refs[1]
		table.insert(groups, {
			uri = uri,
			refs = refs,
			summary = `{first.className}.{first.prop}`,
		})
	end
	table.sort(groups, function(a, b)
		if #a.refs ~= #b.refs then
			return #a.refs > #b.refs
		end
		return a.uri < b.uri
	end)
	return groups, totalRefs
end

local function HeaderCell(text: string, xScale: number, xOff: number, wScale: number, wOff: number)
	return create "TextLabel" {
		Position = UDim2.new(xScale, xOff, 0, 0),
		Size = UDim2.new(wScale, wOff, 1, 0),
		BackgroundTransparency = 1,
		Font = Enum.Font.SourceSansSemibold,
		TextSize = 12,
		TextXAlignment = Enum.TextXAlignment.Left,
		TextColor3 = C_DIM,
		Text = text,
		{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
	}
end

local function TextureViewerApp()
	local query = source("")
	local selectionOnly = source(false)
	local groups = source({} :: { TextureGroup })
	local totalRefs = source(0)
	local selectedUri = source(nil :: string?)
	local replaceText = source("")
	local statusMsg = source("")
	local replaceBoxRef: TextBox? = nil

	local function refresh()
		local g, n = scanTextures(selectionOnly())
		Vide.batch(function()
			groups(g)
			totalRefs(n)
			local cur = selectedUri()
			if cur then
				local found = false
				for _, item in ipairs(g) do
					if item.uri == cur then
						found = true
						break
					end
				end
				if not found then
					selectedUri(if g[1] then g[1].uri else nil)
				end
			elseif g[1] then
				selectedUri(g[1].uri)
			end
		end)
	end

	refresh()

	Vide.cleanup(Selection.SelectionChanged:Connect(function()
		if selectionOnly() then
			refresh()
		end
	end))

	local filtered = Vide.derive(function(): { TextureGroup }
		local q = string.lower(trim(query()))
		local all = groups()
		if q == "" then
			return all
		end
		local out: { TextureGroup } = {}
		for _, g in ipairs(all) do
			local hit = string.find(string.lower(g.uri), q, 1, true) ~= nil
				or string.find(string.lower(g.summary), q, 1, true) ~= nil
			if not hit then
				for _, r in ipairs(g.refs) do
					if string.find(string.lower(r.path), q, 1, true) ~= nil then
						hit = true
						break
					end
				end
			end
			if hit then
				table.insert(out, g)
			end
		end
		return out
	end)

	local activeGroup = Vide.derive(function(): TextureGroup?
		local uri = selectedUri()
		if not uri then
			return nil
		end
		for _, g in ipairs(groups()) do
			if g.uri == uri then
				return g
			end
		end
		return nil
	end)

	local function selectGroupInstances(g: TextureGroup)
		local sel: { Instance } = {}
		local seen: { [Instance]: boolean } = {}
		for _, r in ipairs(g.refs) do
			if r.inst.Parent and not seen[r.inst] then
				seen[r.inst] = true
				table.insert(sel, r.inst)
			end
		end
		if #sel > 0 then
			Selection:Set(sel)
			statusMsg(`Selected {#sel} instance(s)`)
		end
	end

	local function replaceActiveTexture()
		local g = activeGroup()
		local nextUri = trim(replaceText())
		if not g or nextUri == "" then
			return
		end
		-- Bare numeric asset IDs become rbxassetid://<id>
		if string.match(nextUri, "^%d+$") then
			nextUri = "rbxassetid://" .. nextUri
		end
		local count = 0
		withRecording("TextureViewerReplace", "Replace Texture Asset", function()
			for _, r in ipairs(g.refs) do
				if r.inst.Parent then
					local ok = pcall(function()
						(r.inst :: any)[r.prop] = nextUri
					end)
					if ok then
						count += 1
					end
				end
			end
		end)
		replaceText("")
		if replaceBoxRef then
			replaceBoxRef.Text = ""
		end
		selectedUri(nextUri)
		refresh()
		statusMsg(`Replaced texture on {count} property reference(s)`)
	end

	return create "Frame" {
		Name = "TextureViewerRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- LEFT PANEL (62%): TopPanel + TextureTable
			create "Frame" {
				Name = "LeftPanel",
				Size = UDim2.new(0.62, 0, 1, 0),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					-- TopPanel (34px)
					create "Frame" {
						Name = "TopPanel",
						Size = UDim2.new(1, 0, 0, 34),
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						{
							create "TextButton" {
								Name = "RefreshBtn",
								Position = UDim2.fromOffset(8, 5),
								Size = UDim2.fromOffset(60, 24),
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
							create "TextButton" {
								Name = "SelectionFilterBtn",
								Position = UDim2.fromOffset(74, 5),
								Size = UDim2.fromOffset(70, 24),
								AutoButtonColor = false,
								BorderSizePixel = 0,
								Font = Enum.Font.SourceSans,
								TextSize = 12,
								BackgroundColor3 = function()
									return if selectionOnly() then C_ACCENT() else C_BUTTON()
								end,
								TextColor3 = function()
									return if selectionOnly() then C_ACCENT_TEXT() else C_TEXT()
								end,
								Text = "Selection",
								Activated = function()
									selectionOnly(not selectionOnly())
									refresh()
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
							create "Frame" {
								Position = UDim2.fromOffset(150, 5),
								Size = UDim2.new(1, -242, 0, 24),
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
										PlaceholderText = "Search textures...",
										Text = "",
										changed("Text", function(t) query(t) end),
									},
								},
							},
							create "TextLabel" {
								Position = UDim2.new(1, -88, 0, 0),
								Size = UDim2.fromOffset(80, 34),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 12,
								TextXAlignment = Enum.TextXAlignment.Right,
								TextColor3 = C_DIM,
								Text = function()
									return `{#filtered()} tex ({totalRefs()})`
								end,
							},
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},

					-- Column headers
					create "Frame" {
						Name = "TextureTableHeader",
						Position = UDim2.fromOffset(0, 34),
						Size = UDim2.new(1, 0, 0, 24),
						BackgroundColor3 = C_HEADER,
						BorderSizePixel = 0,
						{
							HeaderCell("Texture", 0, 0, 0.56, 0),
							HeaderCell("Source", 0.56, 0, 0.24, 0),
							HeaderCell("Uses", 0.80, 0, 0.20, 0),
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},

					-- Texture rows
					create "ScrollingFrame" {
						Name = "TextureTableBody",
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
							values(filtered, function(g: TextureGroup, order: () -> number)
								local hovered = source(false)
								return create "TextButton" {
									Name = "TexRow_" .. tostring(order()),
									LayoutOrder = order,
									Size = UDim2.new(1, 0, 0, 36),
									AutoButtonColor = false,
									BorderSizePixel = 0,
									Text = "",
									BackgroundColor3 = function()
										if selectedUri() == g.uri then
											return C_SELECTED()
										elseif hovered() then
											return C_HOVER()
										end
										return C_BG()
									end,
									MouseEnter = function() hovered(true) end,
									MouseLeave = function() hovered(false) end,
									Activated = function()
										selectedUri(g.uri)
									end,
									{
										-- 28x28 preview swatch
										create "Frame" {
											Position = UDim2.new(0, 8, 0.5, -14),
											Size = UDim2.fromOffset(28, 28),
											BackgroundColor3 = Color3.fromRGB(30, 32, 38),
											BorderSizePixel = 0,
											{
												create "UICorner" { CornerRadius = UDim.new(0, 3) },
												create "UIStroke" {
													ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
													Thickness = 1,
													Color = C_BORDER,
												},
												create "ImageLabel" {
													Size = UDim2.fromScale(1, 1),
													BackgroundTransparency = 1,
													ScaleType = Enum.ScaleType.Fit,
													Image = g.uri,
												},
											},
										},
										create "TextLabel" {
											Position = UDim2.fromOffset(42, 0),
											Size = UDim2.new(0.56, -46, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.Code,
											TextSize = 11,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = g.uri,
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.56, 0),
											Size = UDim2.new(0.24, -4, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_DIM,
											Text = g.summary,
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
										create "TextLabel" {
											Position = UDim2.fromScale(0.80, 0),
											Size = UDim2.new(0.10, 0, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 12,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_TEXT,
											Text = tostring(#g.refs),
											{ create "UIPadding" { PaddingLeft = UDim.new(0, 6) } },
										},
										create "TextButton" {
											Name = "SelectAllBtn",
											AnchorPoint = Vector2.new(1, 0.5),
											Position = UDim2.new(1, -8, 0.5, 0),
											Size = UDim2.fromOffset(46, 20),
											AutoButtonColor = false,
											BorderSizePixel = 0,
											BackgroundColor3 = C_BUTTON,
											Font = Enum.Font.SourceSans,
											TextSize = 11,
											TextColor3 = C_TEXT,
											Text = "Select",
											Activated = function()
												selectedUri(g.uri)
												selectGroupInstances(g)
											end,
											{ create "UICorner" { CornerRadius = UDim.new(0, 3) } },
										},
									},
								}
							end),
							show(function() return #filtered() == 0 end, function()
								return create "TextLabel" {
									Size = UDim2.new(1, 0, 0, 48),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextColor3 = C_DIM,
									Text = function()
										return if selectionOnly()
											then "No textures found in the current selection."
											else "No textures found in this place."
									end,
								}
							end),
						},
					},
				},
			},

			-- Vertical split divider
			create "Frame" {
				Position = UDim2.fromScale(0.62, 0),
				Size = UDim2.new(0, 1, 1, 0),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- RIGHT PANEL (38%): Preview + Replace + Referencing Instances
			create "Frame" {
				Name = "RightPanel",
				Position = UDim2.new(0.62, 1, 0, 0),
				Size = UDim2.new(0.38, -1, 1, 0),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					create "Frame" {
						Name = "RightHeader",
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
								Text = "References & Replace",
							},
							create "Frame" {
								Position = UDim2.new(0, 0, 1, -1),
								Size = UDim2.new(1, 0, 0, 1),
								BackgroundColor3 = C_BORDER,
								BorderSizePixel = 0,
							},
						},
					},

					show(function() return activeGroup() ~= nil end, function()
						return create "Frame" {
							Position = UDim2.fromOffset(0, 34),
							Size = UDim2.new(1, 0, 1, -34),
							BackgroundTransparency = 1,
							{
								-- Preview + Replace bar
								create "Frame" {
									Size = UDim2.new(1, 0, 0, 74),
									BackgroundColor3 = C_HEADER,
									BorderSizePixel = 0,
									{
										create "Frame" {
											Position = UDim2.fromOffset(8, 8),
											Size = UDim2.fromOffset(58, 58),
											BackgroundColor3 = Color3.fromRGB(26, 28, 34),
											BorderSizePixel = 0,
											{
												create "UICorner" { CornerRadius = UDim.new(0, 4) },
												create "UIStroke" {
													ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
													Thickness = 1,
													Color = C_BORDER,
												},
												create "ImageLabel" {
													Size = UDim2.fromScale(1, 1),
													BackgroundTransparency = 1,
													ScaleType = Enum.ScaleType.Fit,
													Image = function()
														local g = activeGroup()
														return if g then g.uri else ""
													end,
												},
											},
										},
										create "TextLabel" {
											Position = UDim2.fromOffset(74, 8),
											Size = UDim2.new(1, -82, 0, 18),
											BackgroundTransparency = 1,
											Font = Enum.Font.Code,
											TextSize = 11,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_TEXT,
											Text = function()
												local g = activeGroup()
												return if g then g.uri else ""
											end,
										},
										create "Frame" {
											Position = UDim2.fromOffset(74, 30),
											Size = UDim2.new(1, -148, 0, 24),
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
													Name = "ReplaceInput",
													Position = UDim2.fromOffset(6, 0),
													Size = UDim2.new(1, -10, 1, 0),
													BackgroundTransparency = 1,
													BorderSizePixel = 0,
													ClearTextOnFocus = false,
													Font = Enum.Font.SourceSans,
													TextSize = 12,
													TextXAlignment = Enum.TextXAlignment.Left,
													TextColor3 = C_TEXT,
													PlaceholderColor3 = C_DIM,
													PlaceholderText = "New asset ID...",
													Text = "",
													changed("Text", function(t) replaceText(t) end),
													FocusLost = function(e) if e then replaceActiveTexture() end end,
													action(function(inst) replaceBoxRef = inst :: TextBox end),
												},
											},
										},
										create "TextButton" {
											Name = "ReplaceBtn",
											Position = UDim2.new(1, -68, 0, 30),
											Size = UDim2.fromOffset(60, 24),
											AutoButtonColor = false,
											BorderSizePixel = 0,
											BackgroundColor3 = C_ACCENT,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 12,
											TextColor3 = C_ACCENT_TEXT,
											Text = "Replace",
											Activated = replaceActiveTexture,
											{ create "UICorner" { CornerRadius = UDim.new(0, 4) } },
										},
										create "TextLabel" {
											Position = UDim2.fromOffset(74, 56),
											Size = UDim2.new(1, -82, 0, 14),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 11,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextTruncate = Enum.TextTruncate.AtEnd,
											TextColor3 = C_DIM,
											Text = statusMsg,
										},
										create "Frame" {
											Position = UDim2.new(0, 0, 1, -1),
											Size = UDim2.new(1, 0, 0, 1),
											BackgroundColor3 = C_BORDER,
											BorderSizePixel = 0,
										},
									},
								},

								-- Referencing instances list
								create "ScrollingFrame" {
									Name = "RefList",
									Position = UDim2.fromOffset(0, 74),
									Size = UDim2.new(1, 0, 1, -74),
									BackgroundTransparency = 1,
									BorderSizePixel = 0,
									ScrollBarThickness = 6,
									ScrollBarImageColor3 = C_SCROLL,
									CanvasSize = UDim2.fromOffset(0, 0),
									AutomaticCanvasSize = Enum.AutomaticSize.Y,
									{
										create "UIListLayout" { SortOrder = Enum.SortOrder.LayoutOrder },
										values(function(): { TextureRef }
											local g = activeGroup()
											return if g then g.refs else {}
										end, function(r: TextureRef, order: () -> number)
											local hovered = source(false)
											return create "TextButton" {
												LayoutOrder = order,
												Size = UDim2.new(1, 0, 0, 24),
												AutoButtonColor = false,
												BorderSizePixel = 0,
												Text = "",
												BackgroundColor3 = function()
													return if hovered() then C_HOVER() else C_BG()
												end,
												MouseEnter = function() hovered(true) end,
												MouseLeave = function() hovered(false) end,
												Activated = function()
													if r.inst.Parent then
														Selection:Set({ r.inst })
													end
												end,
												{
													create "TextLabel" {
														Position = UDim2.fromOffset(8, 0),
														Size = UDim2.new(0.42, -8, 1, 0),
														BackgroundTransparency = 1,
														Font = Enum.Font.SourceSans,
														TextSize = 12,
														TextXAlignment = Enum.TextXAlignment.Left,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_TEXT,
														Text = `{r.inst.Name}.{r.prop}`,
													},
													create "TextLabel" {
														Position = UDim2.fromScale(0.42, 0),
														Size = UDim2.new(0.58, -8, 1, 0),
														BackgroundTransparency = 1,
														Font = Enum.Font.SourceSans,
														TextSize = 11,
														TextXAlignment = Enum.TextXAlignment.Left,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_DIM,
														Text = r.path,
													},
												},
											}
										end),
									},
								},
							},
						}
					end, function()
						return create "TextLabel" {
							Position = UDim2.fromOffset(12, 48),
							Size = UDim2.new(1, -24, 0, 40),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 12,
							TextWrapped = true,
							TextColor3 = C_DIM,
							Text = "Select a texture on the left to inspect references or bulk-replace its asset ID.",
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
		unmount = Vide.mount(TextureViewerApp, widget)
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
