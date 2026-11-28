--!strict
--[[
    RusTudio Built-in Suite: Tag Editor (Luau / Vide)

    1:1 recreation of Roblox Studio's TagEditor built-in plugin
    (TagTopPane, NewTagTextInput, TagListView, TagListRow, TagSettingsToggleButton)
    using Studio's real TagEditor textures.
]]

local CollectionService = game:GetService("CollectionService")
local CoreGui = game:GetService("CoreGui")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)
local TagStore = require(script.Parent.TagStore)

local create = Vide.create
local source = Vide.source
local show = Vide.show
local values = Vide.values
local changed = Vide.changed
local action = Vide.action

local toolbar = plugin:CreateToolbar("tag_editor")
local toggleButton = toolbar:CreateButton(
	"tag_editor",
	"Inspect, create, and assign CollectionService tags",
	"rbxasset://studio_svg_textures/Shared/WidgetIcons/Dark/Large/TagEditor.png",
	"Tag Editor"
)
toggleButton.ClickableWhenViewportHidden = true

local widgetInfo = DockWidgetPluginGuiInfo.new(
	Enum.InitialDockState.Right,
	false,
	false,
	300,
	200,
	200,
	250
)

local widget = plugin:CreateDockWidgetPluginGui("TagEditor", widgetInfo)
widget.Title = "Tag Editor"
widget.Name = "RusTudioTagEditor"

-- Studio TagEditor textures (verified in StudioContent/textures/TagEditor/ & StudioSharedUI)
local ICON_SEARCH = "rbxasset://textures/StudioSharedUI/search.png"
local ICON_CLEAR = "rbxasset://textures/StudioSharedUI/clear.png"
local ICON_FOLDER = "rbxasset://textures/TagEditor/Folder.png"
local ICON_TRASH = "rbxasset://textures/TagEditor/Trash.png"
local ICON_ADD = "rbxasset://textures/TagEditor/Add.png"
local ICON_TAG = "rbxasset://textures/TagEditor/Tag.png"
local ICON_VIS_ON = "rbxasset://textures/TagEditor/VisibilityOnDarkTheme.png"
local ICON_VIS_OFF = "rbxasset://textures/TagEditor/VisibilityOffDarkTheme.png"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_SELECTED = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Selected)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_ACCENT_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButtonText)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

local function TagRow(
	store: any,
	tagName: string,
	layoutOrder: () -> number,
	selectedTag: any
)
	local hovered = source(false)
	local deleteHovered = source(false)

	local info = Vide.derive(function()
		return store.getTagInfo(tagName)
	end)

	return create "Frame" {
		Name = "Row_" .. tagName,
		LayoutOrder = layoutOrder,
		Size = UDim2.new(1, 0, 0, 26),
		BackgroundColor3 = function()
			if selectedTag() == tagName then
				return C_SELECTED()
			elseif hovered() then
				return C_HOVER()
			end
			return C_BG()
		end,
		BackgroundTransparency = function()
			return if selectedTag() == tagName or hovered() then 0 else 1
		end,
		BorderSizePixel = 0,
		MouseEnter = function() hovered(true) end,
		MouseLeave = function() hovered(false) end,
		{
			create "UICorner" { CornerRadius = UDim.new(0, 3) },

			-- Tri-state checkbox (adds/removes tag on current Selection)
			create "TextButton" {
				Name = "Checkbox",
				Position = UDim2.new(0, 6, 0.5, -8),
				Size = UDim2.fromOffset(16, 16),
				AutoButtonColor = false,
				BackgroundColor3 = function()
					local st = info().check
					if st == "all" or st == "some" then
						return C_ACCENT()
					end
					return C_INPUT()
				end,
				BackgroundTransparency = function()
					return if store.selectionCount() == 0 then 0.55 else 0
				end,
				BorderSizePixel = 0,
				Font = Enum.Font.SourceSansBold,
				TextSize = 12,
				TextColor3 = C_ACCENT_TEXT,
				Text = function()
					local st = info().check
					if st == "all" then
						return "✔"
					elseif st == "some" then
						return "–"
					end
					return ""
				end,
				Activated = function()
					selectedTag(tagName)
					store.toggleTagOnSelection(tagName)
				end,
				{
					create "UICorner" { CornerRadius = UDim.new(0, 3) },
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = function()
							return if info().check ~= "none" then C_ACCENT() else C_BORDER()
						end,
					},
				},
			},

			-- Real Studio Tag icon tinted with tag color (clicking cycles swatch color)
			create "ImageButton" {
				Name = "ColorDot",
				Position = UDim2.new(0, 28, 0.5, -8),
				Size = UDim2.fromOffset(16, 16),
				AutoButtonColor = false,
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				Image = ICON_TAG,
				ImageColor3 = function()
					return info().color
				end,
				Activated = function()
					selectedTag(tagName)
					store.cycleColor(tagName)
				end,
			},

			-- Tag name button (clicking selects all instances with this tag)
			create "TextButton" {
				Name = "TagLabel",
				Position = UDim2.new(0, 50, 0, 0),
				Size = UDim2.new(1, -108, 1, 0),
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				AutoButtonColor = false,
				Font = Enum.Font.SourceSans,
				TextSize = 13,
				TextXAlignment = Enum.TextXAlignment.Left,
				TextTruncate = Enum.TextTruncate.AtEnd,
				TextColor3 = C_TEXT,
				Text = tagName,
				Activated = function()
					selectedTag(tagName)
					store.selectTagged(tagName)
				end,
			},

			-- Tagged instance count badge
			create "TextButton" {
				Name = "CountBadge",
				Position = UDim2.new(1, -54, 0.5, -8),
				Size = UDim2.fromOffset(26, 16),
				AutoButtonColor = false,
				BackgroundColor3 = C_BUTTON,
				BorderSizePixel = 0,
				Font = Enum.Font.SourceSans,
				TextSize = 11,
				TextColor3 = function()
					return if info().count > 0 then C_TEXT() else C_DIM()
				end,
				Text = function()
					return tostring(info().count)
				end,
				Activated = function()
					selectedTag(tagName)
					store.selectTagged(tagName)
				end,
				{ create "UICorner" { CornerRadius = UDim.new(0, 3) } },
			},

			-- Remove tag icon button (Studio Trash icon)
			create "ImageButton" {
				Name = "DeleteTag",
				Position = UDim2.new(1, -24, 0.5, -9),
				Size = UDim2.fromOffset(18, 18),
				AutoButtonColor = false,
				BackgroundColor3 = Color3.fromRGB(205, 65, 65),
				BackgroundTransparency = function()
					return if deleteHovered() then 0.25 else 1
				end,
				BorderSizePixel = 0,
				Image = ICON_TRASH,
				ScaleType = Enum.ScaleType.Fit,
				ImageColor3 = function()
					return if deleteHovered() then Color3.fromRGB(255, 255, 255) else C_DIM()
				end,
				MouseEnter = function() deleteHovered(true) end,
				MouseLeave = function() deleteHovered(false) end,
				Activated = function()
					if selectedTag() == tagName then
						selectedTag(nil)
					end
					store.deleteTag(tagName)
				end,
				{ create "UICorner" { CornerRadius = UDim.new(0, 3) } },
			},
		},
	}
end

local function TagEditorApp()
	local store = TagStore.new(plugin)
	store.subscribe()

	local newTagText = source("")
	local worldView = source(false)
	local settingsOpen = source(false)
	local selectedTag = source(nil :: string?)
	local searchBoxRef: TextBox? = nil
	local newTagBoxRef: TextBox? = nil

	local function submitNewTag()
		local raw = newTagText()
		if store.createTag(raw) then
			selectedTag(raw)
			newTagText("")
			if newTagBoxRef then
				newTagBoxRef.Text = ""
			end
		end
	end

	-- World View: live SelectionBox adornments in CoreGui for all tagged workspace parts/models
	local worldFolder: Folder? = nil
	local function clearWorldAdorns()
		if worldFolder then
			pcall(function()
				worldFolder:Destroy()
			end)
			worldFolder = nil
		end
	end
	Vide.cleanup(clearWorldAdorns)

	Vide.effect(function()
		local active = worldView()
		local tags = store.tagNames()
		local sel = selectedTag()
		clearWorldAdorns()
		if not active then
			return
		end
		local folder = Instance.new("Folder")
		folder.Name = "RusTudioTagWorldView"
		local seen: { [Instance]: boolean } = {}
		local targetTags = if sel and table.find(tags, sel) then { sel } else tags
		for _, tagName in ipairs(targetTags) do
			local info = store.getTagInfo(tagName)
			local ok, tagged = pcall(function()
				return CollectionService:GetTagged(tagName)
			end)
			if ok and type(tagged) == "table" then
				for _, inst in ipairs(tagged) do
					if not seen[inst] and inst:IsDescendantOf(workspace) and (inst:IsA("BasePart") or inst:IsA("Model")) then
						seen[inst] = true
						local box = Instance.new("SelectionBox")
						box.Adornee = inst
						box.Color3 = info.color
						box.SurfaceColor3 = info.color
						box.SurfaceTransparency = 0.78
						box.LineThickness = 0.04
						box.Parent = folder
					end
				end
			end
		end
		folder.Parent = CoreGui
		worldFolder = folder
	end)

	local bottomHeight = Vide.derive(function()
		return if settingsOpen() then 104 else 30
	end)

	return create "Frame" {
		Name = "TagEditorRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- TagTopPane: Row 1 (SearchBar + NewGroup + Delete) & Row 2 (Add New Tag + World View)
			create "Frame" {
				Name = "TagTopPane",
				Size = UDim2.new(1, 0, 0, 72),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					-- Row 1: SearchRowPane
					create "Frame" {
						Name = "SearchRowPane",
						Position = UDim2.fromOffset(8, 8),
						Size = UDim2.new(1, -16, 0, 26),
						BackgroundTransparency = 1,
						{
							create "Frame" {
								Name = "SearchInputWrap",
								Size = UDim2.new(1, -56, 1, 0),
								BackgroundColor3 = C_INPUT,
								BorderSizePixel = 0,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = C_BORDER,
									},
									create "ImageLabel" {
										AnchorPoint = Vector2.new(0, 0.5),
										Position = UDim2.new(0, 7, 0.5, 0),
										Size = UDim2.fromOffset(14, 14),
										BackgroundTransparency = 1,
										Image = ICON_SEARCH,
										ImageColor3 = C_DIM,
									},
									create "TextBox" {
										Name = "SearchBox",
										Position = UDim2.fromOffset(25, 0),
										Size = UDim2.new(1, -45, 1, 0),
										BackgroundTransparency = 1,
										BorderSizePixel = 0,
										ClearTextOnFocus = false,
										Font = Enum.Font.SourceSans,
										TextSize = 13,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = C_TEXT,
										PlaceholderColor3 = C_DIM,
										PlaceholderText = "Search",
										Text = "",
										changed("Text", function(t)
											store.searchQuery(t)
										end),
										action(function(inst)
											searchBoxRef = inst :: TextBox
										end),
									},
									show(function()
										return #store.searchQuery() > 0
									end, function()
										return create "ImageButton" {
											Name = "ClearSearch",
											AnchorPoint = Vector2.new(1, 0.5),
											Position = UDim2.new(1, -4, 0.5, 0),
											Size = UDim2.fromOffset(14, 14),
											BackgroundTransparency = 1,
											Image = ICON_CLEAR,
											ImageColor3 = C_DIM,
											Activated = function()
												store.searchQuery("")
												if searchBoxRef then
													searchBoxRef.Text = ""
												end
											end,
										}
									end),
								},
							},
							-- NewGroup button (Folder icon: starts a grouped "Group/" tag prefix)
							create "ImageButton" {
								Name = "NewGroupButton",
								Position = UDim2.new(1, -50, 0, 1),
								Size = UDim2.fromOffset(22, 24),
								AutoButtonColor = false,
								BackgroundColor3 = C_BUTTON,
								BorderSizePixel = 0,
								Image = ICON_FOLDER,
								ScaleType = Enum.ScaleType.Fit,
								ImageColor3 = C_TEXT,
								Activated = function()
									local prefix = "Group/"
									newTagText(prefix)
									if newTagBoxRef then
										newTagBoxRef.Text = prefix
										pcall(function() newTagBoxRef:CaptureFocus() end)
									end
								end,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIPadding" {
										PaddingLeft = UDim.new(0, 4),
										PaddingRight = UDim.new(0, 4),
										PaddingTop = UDim.new(0, 4),
										PaddingBottom = UDim.new(0, 4),
									},
								},
							},
							-- Delete selected tag button (Trash icon)
							create "ImageButton" {
								Name = "DeleteSelectedButton",
								Position = UDim2.new(1, -24, 0, 1),
								Size = UDim2.fromOffset(24, 24),
								AutoButtonColor = false,
								BackgroundColor3 = C_BUTTON,
								BorderSizePixel = 0,
								Image = ICON_TRASH,
								ScaleType = Enum.ScaleType.Fit,
								ImageColor3 = function()
									return if selectedTag() then C_TEXT() else C_DIM()
								end,
								Activated = function()
									local sel = selectedTag()
									if sel then
										selectedTag(nil)
										store.deleteTag(sel)
									end
								end,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIPadding" {
										PaddingLeft = UDim.new(0, 4),
										PaddingRight = UDim.new(0, 4),
										PaddingTop = UDim.new(0, 4),
										PaddingBottom = UDim.new(0, 4),
									},
								},
							},
						},
					},

					-- Row 2: NewTagRowPane ("+ Add New Tag..." + World View toggle)
					create "Frame" {
						Name = "NewTagRowPane",
						Position = UDim2.fromOffset(8, 40),
						Size = UDim2.new(1, -16, 0, 26),
						BackgroundTransparency = 1,
						{
							create "Frame" {
								Name = "NewTagWrap",
								Size = UDim2.new(1, -102, 1, 0),
								BackgroundColor3 = C_INPUT,
								BorderSizePixel = 0,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = C_BORDER,
									},
									create "ImageButton" {
										Name = "AddTagBtn",
										AnchorPoint = Vector2.new(0, 0.5),
										Position = UDim2.new(0, 6, 0.5, 0),
										Size = UDim2.fromOffset(14, 14),
										BackgroundTransparency = 1,
										Image = ICON_ADD,
										ImageColor3 = C_TEXT,
										Activated = submitNewTag,
									},
									create "TextBox" {
										Name = "NewTagInput",
										Position = UDim2.fromOffset(24, 0),
										Size = UDim2.new(1, -28, 1, 0),
										BackgroundTransparency = 1,
										BorderSizePixel = 0,
										ClearTextOnFocus = false,
										Font = Enum.Font.SourceSans,
										TextSize = 13,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = C_TEXT,
										PlaceholderColor3 = C_DIM,
										PlaceholderText = "Add New Tag...",
										Text = "",
										changed("Text", function(t)
											newTagText(t)
										end),
										FocusLost = function(enterPressed)
											if enterPressed then
												submitNewTag()
											end
										end,
										action(function(inst)
											newTagBoxRef = inst :: TextBox
										end),
									},
								},
							},

							-- World View toggle button
							create "TextButton" {
								Name = "ToggleWorldViewButton",
								Position = UDim2.new(1, -96, 0, 0),
								Size = UDim2.fromOffset(96, 26),
								AutoButtonColor = false,
								BorderSizePixel = 0,
								Text = "",
								BackgroundColor3 = function()
									return if worldView() then C_ACCENT() else C_BUTTON()
								end,
								Activated = function()
									worldView(not worldView())
								end,
								{
									create "UICorner" { CornerRadius = UDim.new(0, 4) },
									create "UIStroke" {
										ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
										Thickness = 1,
										Color = function()
											return if worldView() then C_ACCENT() else C_BORDER()
										end,
									},
									create "ImageLabel" {
										AnchorPoint = Vector2.new(0, 0.5),
										Position = UDim2.new(0, 7, 0.5, 0),
										Size = UDim2.fromOffset(15, 15),
										BackgroundTransparency = 1,
										Image = function()
											return if worldView() then ICON_VIS_ON else ICON_VIS_OFF
										end,
									},
									create "TextLabel" {
										Position = UDim2.fromOffset(26, 0),
										Size = UDim2.new(1, -30, 1, 0),
										BackgroundTransparency = 1,
										Font = Enum.Font.SourceSans,
										TextSize = 12,
										TextXAlignment = Enum.TextXAlignment.Left,
										TextColor3 = function()
											return if worldView() then Color3.fromRGB(255, 255, 255) else C_TEXT()
										end,
										Text = "World View",
									},
								},
							},
						},
					},

					create "Frame" {
						Position = UDim2.new(0, 0, 1, -1),
						Size = UDim2.new(1, 0, 0, 1),
						BackgroundColor3 = C_BORDER,
						BorderSizePixel = 0,
					},
				},
			},

			-- TagListView
			create "ScrollingFrame" {
				Name = "TagList",
				Position = UDim2.fromOffset(0, 72),
				Size = function()
					return UDim2.new(1, 0, 1, -72 - bottomHeight())
				end,
				BackgroundTransparency = 1,
				BorderSizePixel = 0,
				ScrollBarThickness = 6,
				ScrollBarImageColor3 = C_SCROLL,
				CanvasSize = UDim2.fromOffset(0, 0),
				AutomaticCanvasSize = Enum.AutomaticSize.Y,
				{
					create "UIPadding" {
						PaddingLeft = UDim.new(0, 6),
						PaddingRight = UDim.new(0, 6),
						PaddingTop = UDim.new(0, 4),
						PaddingBottom = UDim.new(0, 4),
					},
					create "UIListLayout" {
						SortOrder = Enum.SortOrder.LayoutOrder,
						Padding = UDim.new(0, 2),
					},

					values(store.filteredTags, function(tagName: string, index: () -> number)
						return TagRow(store, tagName, index, selectedTag)
					end),

					show(function()
						return #store.filteredTags() == 0
					end, function()
						return create "TextLabel" {
							Name = "EmptyState",
							LayoutOrder = 999999,
							Size = UDim2.new(1, 0, 0, 44),
							BackgroundTransparency = 1,
							Font = Enum.Font.SourceSans,
							TextSize = 13,
							TextWrapped = true,
							TextColor3 = C_DIM,
							Text = function()
								if #store.searchQuery() > 0 then
									return `No tags match "{store.searchQuery()}"`
								end
								return "No tags yet. Type above to add a tag."
							end,
						}
					end),
				},
			},

			-- Collapsible TagSettingsToggleButton / TagSettingsPane at bottom (30px collapsed)
			create "Frame" {
				Name = "TagSettingsPane",
				Position = function()
					return UDim2.new(0, 0, 1, -bottomHeight())
				end,
				Size = function()
					return UDim2.new(1, 0, 0, bottomHeight())
				end,
				BackgroundColor3 = C_HEADER,
				BorderSizePixel = 0,
				{
					create "Frame" {
						Size = UDim2.new(1, 0, 0, 1),
						BackgroundColor3 = C_BORDER,
						BorderSizePixel = 0,
					},
					create "TextButton" {
						Name = "TagSettingsToggleButton",
						Position = UDim2.fromOffset(0, 1),
						Size = UDim2.new(1, 0, 0, 29),
						AutoButtonColor = false,
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSansSemibold,
						TextSize = 12,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_TEXT,
						Text = function()
							local arrow = if settingsOpen() then "▾ " else "▸ "
							local sel = selectedTag()
							return if sel then `{arrow}Tag Settings — {sel}` else `{arrow}Tag Settings`
						end,
						Activated = function()
							settingsOpen(not settingsOpen())
						end,
						{
							create "UIPadding" { PaddingLeft = UDim.new(0, 10) },
						},
					},
					show(settingsOpen, function()
						return create "Frame" {
							Position = UDim2.fromOffset(10, 34),
							Size = UDim2.new(1, -20, 0, 62),
							BackgroundTransparency = 1,
							{
								create "TextLabel" {
									Size = UDim2.new(1, 0, 0, 18),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 12,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_DIM,
									Text = function()
										local n = store.selectionCount()
										return if n == 0
											then "No instances selected"
											elseif n == 1 then "1 instance selected"
											else `{n} instances selected`
									end,
								},
								create "TextLabel" {
									Position = UDim2.fromOffset(0, 22),
									Size = UDim2.new(1, 0, 0, 18),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 12,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_DIM,
									Text = function()
										local sel = selectedTag()
										if not sel then
											return "Click any tag row to edit color or select instances."
										end
										return `Color: click tag icon to cycle  •  {#store.tagNames()} total tags`
									end,
								},
							},
						}
					end),
				},
			},
		},
	}
end

local unmount: (() -> ())? = nil
local function mount()
	if unmount then return end
	unmount = Vide.mount(TagEditorApp, widget)
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
