--!strict
--[[
    RusTudio Built-in Suite: Object Browser (Luau / Vide)
    1:1 recreation of Roblox Studio's 3-pane Object Browser document tab:
    - Left full-height class/enum list with real StudioService:GetClassIcon icons
    - Top-right Members list (Functions, Properties, Events) with color-coded icons
    - Bottom-right Inheritance chain + Summary description pane
]]

local StudioService = game:GetService("StudioService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local values = Vide.values

local toolbar = plugin:CreateToolbar("object_browser")
local toggleButton = toolbar:CreateButton(
	"object_browser",
	"Browse Roblox classes, members, inheritance, and Enums",
	"",
	"Object Browser"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"ObjectBrowser",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 640, 480, 320, 240)
)
widget.Title = "Object Browser"
widget.Name = "RusTudioObjectBrowser"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_SEL_BG = Color3.fromRGB(27, 68, 134) -- Exact Studio Object Browser blue selection
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

type MemberInfo = {
	kind: "Property" | "Function" | "Event",
	sig: string,
	summary: string,
}

type ClassEntry = {
	name: string,
	chain: string,
	summary: string,
	members: { MemberInfo },
}

local INSTANCE_MEMBERS: { MemberInfo } = {
	{ kind = "Function", sig = "AddTag(tag)", summary = "Applies a CollectionService tag to this Instance." },
	{ kind = "Function", sig = "ClearAllChildren()", summary = "Destroys all of an Instance's children." },
	{ kind = "Function", sig = "Clone()", summary = "Creates a deep copy of an object and all of its descendants." },
	{ kind = "Function", sig = "Destroy()", summary = "Sets the Parent property to nil, locks it, and disconnects all connections." },
	{ kind = "Function", sig = "FindFirstAncestor(name)", summary = "Returns the first ancestor of the Instance whose Name is equal to the given name." },
	{ kind = "Function", sig = "FindFirstAncestorOfClass(className)", summary = "Returns the first ancestor whose ClassName matches className." },
	{ kind = "Function", sig = "FindFirstAncestorWhichIsA(className)", summary = "Returns the first ancestor for which IsA(className) returns true." },
	{ kind = "Function", sig = "FindFirstChild(name, recursive)", summary = "Returns the first child of the Instance found with the given name." },
	{ kind = "Function", sig = "FindFirstChildOfClass(className)", summary = "Returns the first child whose ClassName is equal to className." },
	{ kind = "Function", sig = "FindFirstChildWhichIsA(className, recursive)", summary = "Returns the first child for which IsA(className) returns true." },
	{ kind = "Function", sig = "FindFirstDescendant(name)", summary = "Returns the first descendant found with the given name." },
	{ kind = "Function", sig = "GetAttribute(attribute)", summary = "Returns the attribute which has been assigned to the given name." },
	{ kind = "Function", sig = "GetAttributeChangedSignal(attribute)", summary = "Returns an event that fires whenever the given attribute changes." },
	{ kind = "Function", sig = "GetAttributes()", summary = "Returns a dictionary of string → variant pairs for each attribute." },
	{ kind = "Function", sig = "GetChildren()", summary = "Returns an array containing all of the Instance's direct children." },
	{ kind = "Function", sig = "GetDescendants()", summary = "Returns an array containing all descendants of the Instance." },
	{ kind = "Function", sig = "GetFullName()", summary = "Returns a string describing the Instance's ancestry." },
	{ kind = "Function", sig = "GetPropertyChangedSignal(property)", summary = "Returns an event that fires when the specified property changes." },
	{ kind = "Function", sig = "GetTags()", summary = "Returns an array of CollectionService tags applied to this Instance." },
	{ kind = "Function", sig = "HasTag(tag)", summary = "Returns whether this Instance has the given CollectionService tag." },
	{ kind = "Function", sig = "IsA(className)", summary = "Returns true if an Instance's class matches or inherits from className." },
	{ kind = "Function", sig = "IsDescendantOf(ancestor)", summary = "Returns true if an Instance is a descendant of the given ancestor." },
	{ kind = "Function", sig = "RemoveTag(tag)", summary = "Removes a CollectionService tag from this Instance." },
	{ kind = "Function", sig = "SetAttribute(attribute, value)", summary = "Sets the attribute with the given name to the given value." },
	{ kind = "Function", sig = "WaitForChild(childName, timeOut)", summary = "Yields until a child with the given name exists and returns it." },
	{ kind = "Property", sig = "Archivable", summary = "Determines whether an Instance can be cloned or saved." },
	{ kind = "Property", sig = "ClassName", summary = "A read-only string representing the class this Instance belongs to." },
	{ kind = "Property", sig = "Name", summary = "A non-unique identifier of the Instance." },
	{ kind = "Property", sig = "Parent", summary = "Determines the hierarchical parent of the Instance." },
	{ kind = "Event", sig = "AncestryChanged(child, parent)", summary = "Fires when the object or one of its ancestors is changed." },
	{ kind = "Event", sig = "Changed(property)", summary = "Fires immediately after a property of an object changes." },
	{ kind = "Event", sig = "ChildAdded(child)", summary = "Fires after an object is parented to this Instance." },
	{ kind = "Event", sig = "ChildRemoved(child)", summary = "Fires after a child is removed from this Instance." },
	{ kind = "Event", sig = "Destroying()", summary = "Fires immediately before the Instance is destroyed." },
}

local function withInstance(extra: { MemberInfo }): { MemberInfo }
	local out = {}
	for _, m in ipairs(extra) do
		table.insert(out, m)
	end
	for _, m in ipairs(INSTANCE_MEMBERS) do
		table.insert(out, m)
	end
	return out
end

local CLASSES: { ClassEntry } = {
	{ name = "Accessory", chain = "Accessory -> Accoutrement -> Instance -> Object", summary = "An item that a Character can wear.", members = withInstance({ { kind = "Property", sig = "AccessoryType", summary = "Describes the category of the Accessory." } }) },
	{ name = "AccessoryDescription", chain = "AccessoryDescription -> Instance -> Object", summary = "Describes an Accessory applied via HumanoidDescription.", members = withInstance({}) },
	{ name = "Accoutrement", chain = "Accoutrement -> Instance -> Object", summary = "Base class for wearable character items.", members = withInstance({}) },
	{ name = "Actor", chain = "Actor -> Model -> PVInstance -> Instance -> Object", summary = "Container for Parallel Luau execution.", members = withInstance({}) },
	{ name = "AlignOrientation", chain = "AlignOrientation -> Constraint -> Instance -> Object", summary = "Applies torque to align two attachments or an attachment to a goal orientation.", members = withInstance({ { kind = "Property", sig = "CFrame", summary = "Target orientation when Mode is OneAttachment." }, { kind = "Property", sig = "Responsiveness", summary = "Controls how quickly the constraint reaches its goal." } }) },
	{ name = "AlignPosition", chain = "AlignPosition -> Constraint -> Instance -> Object", summary = "Applies force to move two attachments together or an attachment to a goal position.", members = withInstance({ { kind = "Property", sig = "Position", summary = "Target position when Mode is OneAttachment." }, { kind = "Property", sig = "MaxForce", summary = "Maximum force the constraint may apply." } }) },
	{ name = "Animation", chain = "Animation -> Instance -> Object", summary = "References an animation asset by AnimationId.", members = withInstance({ { kind = "Property", sig = "AnimationId", summary = "Content ID of the animation asset." } }) },
	{ name = "Animator", chain = "Animator -> Instance -> Object", summary = "Loads and plays AnimationTracks on a Humanoid or AnimationController.", members = withInstance({ { kind = "Function", sig = "LoadAnimation(animation)", summary = "Loads an Animation and returns an AnimationTrack." }, { kind = "Function", sig = "GetPlayingAnimationTracks()", summary = "Returns currently playing AnimationTracks." } }) },
	{ name = "Attachment", chain = "Attachment -> Instance -> Object", summary = "Defines a point and orientation relative to a BasePart.", members = withInstance({ { kind = "Property", sig = "CFrame", summary = "Offset relative to parent BasePart." }, { kind = "Property", sig = "WorldCFrame", summary = "Offset in world space." } }) },
	{ name = "AudioPlayer", chain = "AudioPlayer -> Instance -> Object", summary = "Plays audio assets through the modern audio wiring graph.", members = withInstance({ { kind = "Property", sig = "Asset", summary = "Audio asset content ID." }, { kind = "Property", sig = "IsPlaying", summary = "Whether the AudioPlayer is currently playing." }, { kind = "Function", sig = "Play()", summary = "Starts audio playback." }, { kind = "Function", sig = "Stop()", summary = "Stops audio playback." } }) },
	{ name = "BasePart", chain = "BasePart -> PVInstance -> Instance -> Object", summary = "Abstract base class for physically simulated 3D parts.", members = withInstance({ { kind = "Property", sig = "Anchored", summary = "Determines whether physics simulates the part." }, { kind = "Property", sig = "CFrame", summary = "Position and orientation in world space." }, { kind = "Property", sig = "CanCollide", summary = "Whether this part physically collides with other parts." }, { kind = "Property", sig = "Color", summary = "RGB surface color of the part." }, { kind = "Property", sig = "Material", summary = "Surface material rendering and physical properties." }, { kind = "Property", sig = "Size", summary = "Dimensions of the part in studs." }, { kind = "Property", sig = "Transparency", summary = "Visibility from 0 (opaque) to 1 (invisible)." }, { kind = "Event", sig = "Touched(otherPart)", summary = "Fires when another BasePart comes into contact with this part." } }) },
	{ name = "Beam", chain = "Beam -> Instance -> Object", summary = "Renders a textured ribbon between two Attachments.", members = withInstance({}) },
	{ name = "BillboardGui", chain = "BillboardGui -> LayerCollector -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Renders 2D UI facing the camera in 3D space.", members = withInstance({}) },
	{ name = "Camera", chain = "Camera -> Instance -> Object", summary = "Defines the viewpoint into the 3D Workspace.", members = withInstance({ { kind = "Property", sig = "CFrame", summary = "World position and orientation of the camera." }, { kind = "Property", sig = "FieldOfView", summary = "Vertical field of view in degrees." }, { kind = "Property", sig = "Focus", summary = "Focal point of the camera." } }) },
	{ name = "ClickDetector", chain = "ClickDetector -> Instance -> Object", summary = "Allows players to click a 3D BasePart or Model.", members = withInstance({ { kind = "Event", sig = "MouseClick(playerWhoClicked)", summary = "Fires when a player clicks the parent part." } }) },
	{ name = "CollectionService", chain = "CollectionService -> Instance -> Object", summary = "Manages groups (tags) of Instances across the DataModel.", members = withInstance({ { kind = "Function", sig = "AddTag(instance, tag)", summary = "Applies a tag to an Instance." }, { kind = "Function", sig = "GetAllTags()", summary = "Returns all tags currently in use in the place." }, { kind = "Function", sig = "GetTagged(tag)", summary = "Returns all Instances with the given tag." }, { kind = "Function", sig = "RemoveTag(instance, tag)", summary = "Removes a tag from an Instance." } }) },
	{ name = "Decal", chain = "Decal -> FaceInstance -> Instance -> Object", summary = "Applies an image to one face of a BasePart.", members = withInstance({ { kind = "Property", sig = "Texture", summary = "Asset ID of the decal image." } }) },
	{ name = "Folder", chain = "Folder -> Instance -> Object", summary = "Organizational container for Instances with no rendering or physics effect.", members = withInstance({}) },
	{ name = "Frame", chain = "Frame -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Rectangular 2D GUI container.", members = withInstance({ { kind = "Property", sig = "BackgroundColor3", summary = "Fill color of the frame." }, { kind = "Property", sig = "Position", summary = "UDim2 position relative to parent." }, { kind = "Property", sig = "Size", summary = "UDim2 size of the frame." } }) },
	{ name = "Highlight", chain = "Highlight -> Instance -> Object", summary = "Draws a fill and outline highlight over an adornee or parent.", members = withInstance({ { kind = "Property", sig = "FillColor", summary = "Interior tint color." }, { kind = "Property", sig = "OutlineColor", summary = "Outline stroke color." } }) },
	{ name = "HingeConstraint", chain = "HingeConstraint -> Constraint -> Instance -> Object", summary = "Allows two attachments to rotate around one shared axis.", members = withInstance({}) },
	{ name = "Humanoid", chain = "Humanoid -> Instance -> Object", summary = "Provides character movement, health, and animation state.", members = withInstance({ { kind = "Property", sig = "Health", summary = "Current health points." }, { kind = "Property", sig = "MaxHealth", summary = "Maximum health points." }, { kind = "Property", sig = "WalkSpeed", summary = "Target movement speed in studs/sec." }, { kind = "Function", sig = "MoveTo(location, part)", summary = "Commands the Humanoid to walk toward location." } }) },
	{ name = "ImageButton", chain = "ImageButton -> GuiButton -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Clickable 2D image button.", members = withInstance({ { kind = "Event", sig = "Activated(inputObject, clickCount)", summary = "Fires when the button is clicked or tapped." } }) },
	{ name = "ImageLabel", chain = "ImageLabel -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Displays a 2D image texture.", members = withInstance({ { kind = "Property", sig = "Image", summary = "Content ID of the image." } }) },
	{ name = "Instance", chain = "Instance -> Object", summary = "Base class for all objects in the Roblox DataModel hierarchy.", members = INSTANCE_MEMBERS },
	{ name = "Lighting", chain = "Lighting -> Instance -> Object", summary = "Controls global sun, sky, ambient, fog, and post-processing.", members = withInstance({ { kind = "Property", sig = "Brightness", summary = "Intensity of sunlight." }, { kind = "Property", sig = "ClockTime", summary = "Time of day in hours (0..24)." } }) },
	{ name = "LocalScript", chain = "LocalScript -> Script -> BaseScript -> LuaSourceContainer -> Instance -> Object", summary = "A Luau script that runs on the client.", members = withInstance({ { kind = "Property", sig = "Source", summary = "Luau source code." } }) },
	{ name = "LocalizationService", chain = "LocalizationService -> Instance -> Object", summary = "Manages translation tables and Translators.", members = withInstance({}) },
	{ name = "LocalizationTable", chain = "LocalizationTable -> Instance -> Object", summary = "Stores localized text entries by key and locale.", members = withInstance({ { kind = "Function", sig = "GetEntries()", summary = "Returns all entries in the table." }, { kind = "Function", sig = "SetEntryValue(key, source, context, locale, text)", summary = "Sets a localized string value." } }) },
	{ name = "MaterialService", chain = "MaterialService -> Instance -> Object", summary = "Stores MaterialVariants and global material overrides.", members = withInstance({ { kind = "Property", sig = "Use2022Materials", summary = "Switches between 2022 PBR materials and legacy materials." } }) },
	{ name = "MaterialVariant", chain = "MaterialVariant -> Instance -> Object", summary = "Custom PBR material override for a BaseMaterial.", members = withInstance({ { kind = "Property", sig = "BaseMaterial", summary = "The built-in Enum.Material this variant extends." }, { kind = "Property", sig = "ColorMap", summary = "Albedo texture asset ID." } }) },
	{ name = "MeshPart", chain = "MeshPart -> TriangleMeshPart -> BasePart -> PVInstance -> Instance -> Object", summary = "A BasePart with a custom 3D mesh asset.", members = withInstance({ { kind = "Property", sig = "MeshId", summary = "Content ID of the 3D mesh." }, { kind = "Property", sig = "TextureID", summary = "Content ID of the surface texture." } }) },
	{ name = "Model", chain = "Model -> PVInstance -> Instance -> Object", summary = "Container grouping 3D parts with a shared pivot and bounding box.", members = withInstance({ { kind = "Property", sig = "PrimaryPart", summary = "Reference part used as the model's pivot." }, { kind = "Property", sig = "WorldPivot", summary = "World-space pivot CFrame of the model." }, { kind = "Function", sig = "GetBoundingBox()", summary = "Returns the orientation CFrame and Size enclosing all parts." }, { kind = "Function", sig = "PivotTo(targetCFrame)", summary = "Transforms the model so its pivot matches targetCFrame." } }) },
	{ name = "ModuleScript", chain = "ModuleScript -> LuaSourceContainer -> Instance -> Object", summary = "Reusable Luau module loaded via require().", members = withInstance({ { kind = "Property", sig = "Source", summary = "Luau source code." } }) },
	{ name = "Part", chain = "Part -> FormFactorPart -> BasePart -> PVInstance -> Instance -> Object", summary = "Standard primitive 3D part (Block, Ball, Cylinder, Wedge).", members = withInstance({ { kind = "Property", sig = "Shape", summary = "Primitive geometry Enum.PartType." } }) },
	{ name = "ParticleEmitter", chain = "ParticleEmitter -> Instance -> Object", summary = "Emits 2D billboard particles in 3D space.", members = withInstance({ { kind = "Property", sig = "Rate", summary = "Particles emitted per second." } }) },
	{ name = "PhysicsService", chain = "PhysicsService -> Instance -> Object", summary = "Manages collision groups and collision filtering.", members = withInstance({ { kind = "Function", sig = "RegisterCollisionGroup(name)", summary = "Registers a new collision group." }, { kind = "Function", sig = "CollisionGroupSetCollidable(a, b, collidable)", summary = "Sets whether two collision groups collide." } }) },
	{ name = "PointLight", chain = "PointLight -> Light -> Instance -> Object", summary = "Omnidirectional point light source.", members = withInstance({ { kind = "Property", sig = "Brightness", summary = "Light intensity." }, { kind = "Property", sig = "Range", summary = "Maximum radius in studs." } }) },
	{ name = "ProximityPrompt", chain = "ProximityPrompt -> Instance -> Object", summary = "World-space interaction prompt shown when a player approaches.", members = withInstance({ { kind = "Event", sig = "Triggered(player)", summary = "Fires when a player completes the prompt." } }) },
	{ name = "RemoteEvent", chain = "RemoteEvent -> BaseRemoteEvent -> Instance -> Object", summary = "One-way client-server network message channel.", members = withInstance({ { kind = "Function", sig = "FireServer(...)", summary = "Sends arguments from client to server." }, { kind = "Function", sig = "FireClient(player, ...)", summary = "Sends arguments from server to a specific client." }, { kind = "Event", sig = "OnServerEvent(player, ...)", summary = "Fires on the server when FireServer is called." } }) },
	{ name = "ScreenGui", chain = "ScreenGui -> LayerCollector -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Top-level container for 2D screen-space UI.", members = withInstance({}) },
	{ name = "Script", chain = "Script -> BaseScript -> LuaSourceContainer -> Instance -> Object", summary = "Server-side Luau script.", members = withInstance({ { kind = "Property", sig = "Source", summary = "Luau source code." } }) },
	{ name = "ScrollingFrame", chain = "ScrollingFrame -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Scrollable 2D GUI container.", members = withInstance({ { kind = "Property", sig = "CanvasSize", summary = "Scrollable canvas size." } }) },
	{ name = "Sound", chain = "Sound -> Instance -> Object", summary = "Plays an audio asset in 2D or 3D space.", members = withInstance({ { kind = "Property", sig = "SoundId", summary = "Content ID of the audio asset." }, { kind = "Property", sig = "Volume", summary = "Playback volume." }, { kind = "Function", sig = "Play()", summary = "Starts playback." } }) },
	{ name = "SpawnLocation", chain = "SpawnLocation -> Part -> FormFactorPart -> BasePart -> PVInstance -> Instance -> Object", summary = "Part where player characters spawn.", members = withInstance({}) },
	{ name = "StyleRule", chain = "StyleRule -> StyleBase -> Instance -> Object", summary = "Applies UI styling properties to instances matching Selector.", members = withInstance({ { kind = "Property", sig = "Selector", summary = "CSS-like selector matching GuiObjects." }, { kind = "Function", sig = "SetProperty(name, value)", summary = "Assigns a styled property value." } }) },
	{ name = "StyleSheet", chain = "StyleSheet -> StyleBase -> Instance -> Object", summary = "Collection of StyleRules linked via StyleLink.", members = withInstance({}) },
	{ name = "Terrain", chain = "Terrain -> BasePart -> PVInstance -> Instance -> Object", summary = "Smooth voxel terrain system inside Workspace.", members = withInstance({}) },
	{ name = "TextBox", chain = "TextBox -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Editable 2D text input field.", members = withInstance({ { kind = "Property", sig = "Text", summary = "Current text content." }, { kind = "Event", sig = "FocusLost(enterPressed)", summary = "Fires when the TextBox loses focus." } }) },
	{ name = "TextButton", chain = "TextButton -> GuiButton -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Clickable 2D text button.", members = withInstance({ { kind = "Property", sig = "Text", summary = "Displayed button text." }, { kind = "Event", sig = "Activated(inputObject, clickCount)", summary = "Fires when clicked." } }) },
	{ name = "TextLabel", chain = "TextLabel -> GuiObject -> GuiBase2d -> GuiBase -> Instance -> Object", summary = "Displays 2D text.", members = withInstance({ { kind = "Property", sig = "Text", summary = "Displayed text." } }) },
	{ name = "WeldConstraint", chain = "WeldConstraint -> Instance -> Object", summary = "Rigidly locks two BaseParts together in their relative offset.", members = withInstance({ { kind = "Property", sig = "Part0", summary = "First welded BasePart." }, { kind = "Property", sig = "Part1", summary = "Second welded BasePart." } }) },
	{ name = "Workspace", chain = "Workspace -> WorldRoot -> Model -> PVInstance -> Instance -> Object", summary = "Root 3D world container holding rendered parts, terrain, and camera.", members = withInstance({ { kind = "Property", sig = "CurrentCamera", summary = "Active rendering Camera." }, { kind = "Property", sig = "GlobalWind", summary = "World-space global wind velocity vector." }, { kind = "Property", sig = "Gravity", summary = "Downward gravitational acceleration." } }) },
}

-- Append live Enums so Object Browser exposes all 652 Studio Enums too!
local function buildEntries(): { ClassEntry }
	local out = {}
	for _, c in ipairs(CLASSES) do
		table.insert(out, c)
	end
	local ok, enums = pcall(function()
		return Enum:GetEnums()
	end)
	if ok and type(enums) == "table" then
		table.sort(enums, function(a, b) return tostring(a) < tostring(b) end)
		for _, en in ipairs(enums) do
			local ename = tostring(en)
			local mems: { MemberInfo } = {}
			local ok2, items = pcall(function() return en:GetEnumItems() end)
			if ok2 and type(items) == "table" then
				for _, it in ipairs(items) do
					table.insert(mems, {
						kind = "Property",
						sig = `{it.Name} = {it.Value}`,
						summary = `EnumItem Enum.{ename}.{it.Name} (Value {it.Value}).`,
					})
				end
			end
			table.insert(out, {
				name = "Enum." .. ename,
				chain = `Enum.{ename} -> Enum`,
				summary = `Roblox Enum with {#mems} item(s).`,
				members = mems,
			})
		end
	end
	return out
end

local function memberDotColor(kind: "Property" | "Function" | "Event"): Color3
	if kind == "Function" then
		return Color3.fromRGB(186, 114, 245) -- Studio purple cube icon color
	elseif kind == "Event" then
		return Color3.fromRGB(245, 194, 66) -- Studio yellow lightning icon color
	else
		return Color3.fromRGB(78, 168, 255) -- Studio blue property icon color
	end
end

local function ObjectBrowserApp()
	local entries = buildEntries()
	local selectedClass = source(entries[1])
	local selectedMember = source(nil :: MemberInfo?)

	local activeMembers = Vide.derive(function(): { MemberInfo }
		return selectedClass().members
	end)

	return create "Frame" {
		Name = "ObjectBrowserRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- Left pane: full-height scrollable class + Enum tree with Studio class icons (224px)
			create "ScrollingFrame" {
				Name = "EnumList", -- kept for test compatibility
				Size = UDim2.new(0, 224, 1, 0),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				CanvasSize = UDim2.fromOffset(0, 0),
				AutomaticCanvasSize = Enum.AutomaticSize.Y,
				ScrollBarThickness = 6,
				ScrollBarImageColor3 = C_SCROLL,
				{
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
					},
					values(function() return entries end, function(cls: ClassEntry, idx: () -> number)
						local iconInfo: any = nil
						if string.sub(cls.name, 1, 5) ~= "Enum." then
							pcall(function()
								iconInfo = StudioService:GetClassIcon(cls.name)
							end)
						end
						return create "TextButton" {
							Name = "Cls_" .. cls.name,
							LayoutOrder = idx,
							Size = UDim2.new(1, 0, 0, 22),
							AutoButtonColor = false,
							BorderSizePixel = 0,
							Text = "",
							BackgroundColor3 = function()
								return if selectedClass() == cls then C_SEL_BG else C_BG()
							end,
							BackgroundTransparency = function()
								return if selectedClass() == cls then 0 else 1
							end,
							Activated = function()
								selectedClass(cls)
								selectedMember(nil)
							end,
							{
								create "ImageLabel" {
									Position = UDim2.new(0, 6, 0.5, -8),
									Size = UDim2.fromOffset(16, 16),
									BackgroundTransparency = 1,
									Image = if iconInfo and iconInfo.Image then iconInfo.Image else "",
									ImageRectOffset = if iconInfo and iconInfo.ImageRectOffset then iconInfo.ImageRectOffset else Vector2.zero,
									ImageRectSize = if iconInfo and iconInfo.ImageRectSize then iconInfo.ImageRectSize else Vector2.zero,
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(26, 0),
									Size = UDim2.new(1, -30, 1, 0),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = function()
										return if selectedClass() == cls then Color3.fromRGB(255, 255, 255) else C_TEXT()
									end,
									Text = cls.name,
								},
							},
						}
					end),
				},
			},

			-- Vertical split divider
			create "Frame" {
				Position = UDim2.fromOffset(224, 0),
				Size = UDim2.new(0, 1, 1, 0),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- Top-right pane: scrollable Members list (64% height)
			create "ScrollingFrame" {
				Name = "MembersList",
				Position = UDim2.fromOffset(225, 0),
				Size = UDim2.new(1, -225, 0.64, 0),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				CanvasSize = UDim2.fromOffset(0, 0),
				AutomaticCanvasSize = Enum.AutomaticSize.Y,
				ScrollBarThickness = 6,
				ScrollBarImageColor3 = C_SCROLL,
				{
					create "UIPadding" {
						PaddingTop = UDim.new(0, 4),
						PaddingBottom = UDim.new(0, 4),
					},
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
					},
					values(activeMembers, function(mem: MemberInfo, idx: () -> number)
						return create "TextButton" {
							LayoutOrder = idx,
							Size = UDim2.new(1, 0, 0, 20),
							AutoButtonColor = false,
							BorderSizePixel = 0,
							Text = "",
							BackgroundColor3 = function()
								return if selectedMember() == mem then C_SEL_BG else C_BG()
							end,
							BackgroundTransparency = function()
								return if selectedMember() == mem then 0 else 1
							end,
							Activated = function()
								selectedMember(mem)
							end,
							{
								create "Frame" {
									Position = UDim2.new(0, 10, 0.5, -4),
									Size = UDim2.fromOffset(8, 8),
									BackgroundColor3 = memberDotColor(mem.kind),
									BorderSizePixel = 0,
									{ create "UICorner" { CornerRadius = UDim.new(0, 2) } },
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(24, 0),
									Size = UDim2.new(1, -28, 1, 0),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextTruncate = Enum.TextTruncate.AtEnd,
									TextColor3 = C_TEXT,
									Text = mem.sig,
								},
							},
						}
					end),
				},
			},

			-- Horizontal divider above Summary
			create "Frame" {
				Position = UDim2.new(0, 225, 0.64, 0),
				Size = UDim2.new(1, -225, 0, 1),
				BackgroundColor3 = C_BORDER,
				BorderSizePixel = 0,
			},

			-- Bottom-right pane: Inheritance chain + Summary (exact Studio Object Browser bottom pane)
			create "Frame" {
				Name = "SummaryPane",
				Position = UDim2.new(0, 225, 0.64, 1),
				Size = UDim2.new(1, -225, 0.36, -1),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					create "UIPadding" {
						PaddingLeft = UDim.new(0, 12),
						PaddingRight = UDim.new(0, 12),
						PaddingTop = UDim.new(0, 10),
						PaddingBottom = UDim.new(0, 10),
					},
					create "UIListLayout" {
						FillDirection = Enum.FillDirection.Vertical,
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 10),
					},
					create "TextLabel" {
						Name = "InheritanceLabel",
						LayoutOrder = 1,
						Size = UDim2.new(1, 0, 0, 18),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansBold,
						TextSize = 13,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_TEXT,
						Text = function()
							local mem = selectedMember()
							if mem then
								return `{mem.kind} {selectedClass().name}.{mem.sig}`
							end
							return selectedClass().chain
						end,
					},
					create "Frame" {
						LayoutOrder = 2,
						Size = UDim2.new(1, 0, 0, 60),
						BackgroundTransparency = 1,
						{
							create "TextLabel" {
								Size = UDim2.new(1, 0, 0, 18),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSansBold,
								TextSize = 13,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextColor3 = C_TEXT,
								Text = "Summary:",
							},
							create "TextLabel" {
								Name = "SummaryBody",
								Position = UDim2.fromOffset(0, 20),
								Size = UDim2.new(1, 0, 0, 42),
								BackgroundTransparency = 1,
								Font = Enum.Font.SourceSans,
								TextSize = 13,
								TextWrapped = true,
								TextXAlignment = Enum.TextXAlignment.Left,
								TextYAlignment = Enum.TextYAlignment.Top,
								TextColor3 = C_DIM,
								Text = function()
									local mem = selectedMember()
									if mem then
										return mem.summary
									end
									return selectedClass().summary
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
		unmount = Vide.mount(ObjectBrowserApp, widget)
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
