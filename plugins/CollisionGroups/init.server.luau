--!strict
--[[
    RusTudio Built-in Suite: Collision Groups Editor (Luau / Vide)
    1:1 recreation of Roblox Studio's CollisionGroupsEditor.rbxm:
    - ControlsHeaderRow: Table View / List View toggle, + Add Group, Rename, Delete
    - List View (default): left N/32 Groups list with assign (+) icons + right detail pane
    - Table View: 2D collision checkbox matrix across all registered groups
]]

local PhysicsService = game:GetService("PhysicsService")
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

local ICON_ASSIGN = "rbxasset://textures/CollisionGroupsEditor/assign.png"
local ICON_RENAME = "rbxasset://textures/CollisionGroupsEditor/rename.png"
local ICON_DELETE = "rbxasset://textures/CollisionGroupsEditor/delete.png"

local toolbar = plugin:CreateToolbar("collision_groups")
local toggleButton = toolbar:CreateButton(
	"collision_groups",
	"Manage PhysicsService collision groups and collision matrix",
	"rbxasset://studio_svg_textures/Shared/WidgetIcons/Dark/Large/CollisionGroups.png",
	"Collision Groups"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"CollisionGroups",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 360, 440, 260, 240)
)
widget.Title = "Collision Groups Editor"
widget.Name = "RusTudioCollisionGroups"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_INPUT = StudioTheme.color(Enum.StudioStyleGuideColor.InputFieldBackground)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_BUTTON = StudioTheme.color(Enum.StudioStyleGuideColor.Button)
local C_HOVER = StudioTheme.color(Enum.StudioStyleGuideColor.TableItem, Enum.StudioStyleGuideModifier.Hover)
local C_ACCENT = StudioTheme.color(Enum.StudioStyleGuideColor.DialogMainButton)
local C_SCROLL = StudioTheme.color(Enum.StudioStyleGuideColor.ScrollBar)

local function trim(s: string): string
	return (string.gsub(s, "^%s*(.-)%s*$", "%1"))
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

local function readGroupNames(): { string }
	local names = {}
	local ok, groups = pcall(function()
		return PhysicsService:GetRegisteredCollisionGroups()
	end)
	if ok and type(groups) == "table" then
		for _, g in ipairs(groups) do
			if type(g) == "table" and type(g.name) == "string" then
				table.insert(names, g.name)
			end
		end
	end
	if #names == 0 then
		table.insert(names, "Default")
	end
	return names
end

local function CollisionGroupsApp()
	local groups = source(readGroupNames())
	local viewMode = source("List" :: "List" | "Table")
	local selectedGroup = source(nil :: string?)
	local addingGroup = source(false)
	local renamingGroup = source(false)
	local newGroupText = source("")
	local renameText = source("")
	local rev = source(0)
	local addBoxRef: TextBox? = nil
	local renameBoxRef: TextBox? = nil

	local function refresh()
		local list = readGroupNames()
		local sel = selectedGroup()
		if sel and not table.find(list, sel) then
			selectedGroup(nil)
		end
		Vide.batch(function()
			groups(list)
			rev(rev() + 1)
		end)
	end

	pcall(function()
		Vide.cleanup(ChangeHistoryService.OnUndo:Connect(refresh))
		Vide.cleanup(ChangeHistoryService.OnRedo:Connect(refresh))
	end)

	local function commitAddGroup()
		local name = trim(newGroupText())
		addingGroup(false)
		newGroupText("")
		if addBoxRef then
			addBoxRef.Text = ""
		end
		if name == "" then
			return
		end
		pcall(function()
			PhysicsService:RegisterCollisionGroup(name)
		end)
		selectedGroup(name)
		refresh()
	end

	local function startRenameGroup()
		local sel = selectedGroup()
		if not sel or sel == "Default" or sel == "StudioSelectable" then
			return
		end
		addingGroup(false)
		renameText(sel)
		renamingGroup(true)
	end

	local function commitRenameGroup()
		local oldName = selectedGroup()
		local nextName = trim(renameText())
		renamingGroup(false)
		renameText("")
		if not oldName or oldName == "Default" or oldName == "StudioSelectable" then
			return
		end
		if nextName == "" or nextName == oldName then
			return
		end
		local renamed = pcall(function()
			(PhysicsService :: any):RenameCollisionGroup(oldName, nextName)
		end)
		if not renamed then
			pcall(function()
				PhysicsService:RegisterCollisionGroup(nextName)
				for _, other in ipairs(readGroupNames()) do
					local okC, col = pcall(function()
						return PhysicsService:CollisionGroupsAreCollidable(oldName, other)
					end)
					if okC then
						pcall(function()
							PhysicsService:CollisionGroupSetCollidable(nextName, other, col)
						end)
					end
				end
				for _, d in ipairs(workspace:GetDescendants()) do
					if d:IsA("BasePart") and d.CollisionGroup == oldName then
						d.CollisionGroup = nextName
					end
				end
				PhysicsService:UnregisterCollisionGroup(oldName)
			end)
		end
		selectedGroup(nextName)
		refresh()
	end

	local function deleteSelectedGroup()
		local sel = selectedGroup()
		if not sel or sel == "Default" or sel == "StudioSelectable" then
			return
		end
		pcall(function()
			PhysicsService:UnregisterCollisionGroup(sel)
		end)
		selectedGroup(nil)
		refresh()
	end

	local function assignSelection(groupName: string)
		local parts = selectedParts()
		if #parts == 0 then
			return
		end
		local rec = nil
		pcall(function()
			rec = ChangeHistoryService:TryBeginRecording("CollisionGroupAssign", `Assign "{groupName}"`)
		end)
		local ok = pcall(function()
			for _, p in ipairs(parts) do
				p.CollisionGroup = groupName
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
				ChangeHistoryService:SetWaypoint(`Assign "{groupName}"`)
			end)
		end
		rev(rev() + 1)
	end

	local function toggleCollision(a: string, b: string)
		local cur = true
		pcall(function()
			cur = PhysicsService:CollisionGroupsAreCollidable(a, b)
		end)
		pcall(function()
			PhysicsService:CollisionGroupSetCollidable(a, b, not cur)
		end)
		rev(rev() + 1)
	end

	local function CollisionCheckbox(aSrc: string | () -> string?, b: string, name: string)
		local function readA(): string?
			if type(aSrc) == "function" then
				return aSrc()
			end
			return aSrc
		end
		local collides = Vide.derive(function()
			local _ = rev()
			local a = readA()
			if not a then
				return true
			end
			local ok, v = pcall(function()
				return PhysicsService:CollisionGroupsAreCollidable(a, b)
			end)
			return if ok then v else true
		end)
		return create "TextButton" {
			Name = name,
			AnchorPoint = Vector2.new(0.5, 0.5),
			Position = UDim2.fromScale(0.5, 0.5),
			Size = UDim2.fromOffset(18, 18),
			AutoButtonColor = false,
			BorderSizePixel = 0,
			BackgroundColor3 = function()
				return if collides() then C_ACCENT() else C_INPUT()
			end,
			Font = Enum.Font.SourceSansBold,
			TextSize = 13,
			TextColor3 = Color3.fromRGB(255, 255, 255),
			Text = function()
				return if collides() then "✔" else ""
			end,
			Activated = function()
				local a = readA()
				if a then
					toggleCollision(a, b)
				end
			end,
			{
				create "UICorner" { CornerRadius = UDim.new(0, 3) },
				create "UIStroke" {
					ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
					Thickness = 1,
					Color = function()
						return if collides() then C_ACCENT() else C_BORDER()
					end,
				},
			},
		}
	end

	return create "Frame" {
		Name = "CollisionGroupsRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		{
			-- ControlsHeaderRow (exact Studio top bar: [Table View] [+ Add Group] [Rename] [Delete])
			create "Frame" {
				Name = "ControlsHeaderRow",
				Size = UDim2.new(1, 0, 0, 42),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					create "TextButton" {
						Name = "ViewToggleBtn",
						Position = UDim2.fromOffset(6, 6),
						Size = UDim2.fromOffset(92, 30),
						AutoButtonColor = false,
						BorderSizePixel = 0,
						BackgroundColor3 = C_HEADER,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextColor3 = C_TEXT,
						Text = function()
							return if viewMode() == "List" then "Table View" else "List View"
						end,
						Activated = function()
							viewMode(if viewMode() == "List" then "Table" else "List")
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

					show(function() return not addingGroup() and not renamingGroup() end, function()
						return create "TextButton" {
							Name = "AddGroupButton",
							Position = UDim2.fromOffset(104, 6),
							Size = UDim2.new(1, -172, 0, 30),
							AutoButtonColor = false,
							BorderSizePixel = 0,
							BackgroundColor3 = C_HEADER,
							Font = Enum.Font.SourceSans,
							TextSize = 13,
							TextColor3 = C_TEXT,
							Text = "+ Add Group",
							Activated = function()
								renamingGroup(false)
								addingGroup(true)
							end,
							{
								create "UICorner" { CornerRadius = UDim.new(0, 4) },
								create "UIStroke" {
									ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
									Thickness = 1,
									Color = C_BORDER,
								},
							},
						}
					end, function()
						local isRename = renamingGroup()
						return create "Frame" {
							Position = UDim2.fromOffset(104, 6),
							Size = UDim2.new(1, -172, 0, 30),
							BackgroundColor3 = C_INPUT,
							BorderSizePixel = 0,
							{
								create "UICorner" { CornerRadius = UDim.new(0, 4) },
								create "UIStroke" {
									ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
									Thickness = 1,
									Color = C_ACCENT,
								},
								create "TextBox" {
									Name = if isRename then "RenameGroupInput" else "NewGroupInput",
									Position = UDim2.fromOffset(8, 0),
									Size = UDim2.new(1, -14, 1, 0),
									BackgroundTransparency = 1,
									BorderSizePixel = 0,
									ClearTextOnFocus = false,
									Font = Enum.Font.SourceSans,
									TextSize = 13,
									TextXAlignment = Enum.TextXAlignment.Left,
									TextColor3 = C_TEXT,
									PlaceholderColor3 = C_DIM,
									PlaceholderText = if isRename then "New name + Enter…" else "Group name + Enter…",
									Text = if isRename then renameText() else "",
									changed("Text", function(t)
										if isRename then
											renameText(t)
										else
											newGroupText(t)
										end
									end),
									FocusLost = function(enter)
										if isRename then
											if enter then commitRenameGroup() else renamingGroup(false) end
										else
											if enter then commitAddGroup() else addingGroup(false) end
										end
									end,
									action(function(inst)
										local tb = inst :: TextBox
										if isRename then
											renameBoxRef = tb
										else
											addBoxRef = tb
										end
										task.defer(function()
											if tb.Parent then
												pcall(function() tb:CaptureFocus() end)
											end
										end)
									end),
								},
							},
						}
					end),

					create "ImageButton" {
						Name = "RenameBtn",
						Position = UDim2.new(1, -60, 0, 11),
						Size = UDim2.fromOffset(20, 20),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						Image = ICON_RENAME,
						ImageTransparency = function()
							local sel = selectedGroup()
							return if sel and sel ~= "Default" and sel ~= "StudioSelectable" then 0.15 else 0.65
						end,
						Activated = startRenameGroup,
					},
					create "ImageButton" {
						Name = "DeleteBtn",
						Position = UDim2.new(1, -30, 0, 11),
						Size = UDim2.fromOffset(20, 20),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						Image = ICON_DELETE,
						ImageTransparency = function()
							local sel = selectedGroup()
							return if sel and sel ~= "Default" and sel ~= "StudioSelectable" then 0.15 else 0.65
						end,
						Activated = deleteSelectedGroup,
					},
				},
			},

			-- Body: List View (Studio default) OR Table View (2D matrix)
			show(function() return viewMode() == "List" end, function()
				return create "Frame" {
					Name = "ListViewBody",
					Position = UDim2.fromOffset(0, 44),
					Size = UDim2.new(1, 0, 1, -44),
					BackgroundTransparency = 1,
					{
						-- Left GroupLabelColumn (114px wide)
						create "Frame" {
							Name = "LeftColumn",
							Size = UDim2.new(0, 114, 1, 0),
							BackgroundColor3 = C_BG,
							BorderSizePixel = 0,
							{
								create "Frame" {
									Size = UDim2.new(1, 0, 0, 32),
									BackgroundColor3 = C_HEADER,
									BorderSizePixel = 0,
									{
										create "TextLabel" {
											Name = "GroupCountLabel",
											Size = UDim2.fromScale(1, 1),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSans,
											TextSize = 13,
											TextColor3 = C_TEXT,
											Text = function()
												return `{#groups()}/32 Groups`
											end,
										},
									},
								},
								create "ScrollingFrame" {
									Name = "GroupList",
									Position = UDim2.fromOffset(0, 32),
									Size = UDim2.new(1, 0, 1, -32),
									BackgroundTransparency = 1,
									BorderSizePixel = 0,
									CanvasSize = UDim2.fromOffset(0, 0),
									AutomaticCanvasSize = Enum.AutomaticSize.Y,
									ScrollBarThickness = 4,
									ScrollBarImageColor3 = C_SCROLL,
									{
										create "UIListLayout" {
											FillDirection = Enum.FillDirection.Vertical,
											SortOrder = Enum.SortOrder.LayoutOrder,
											Padding = UDim.new(0, 1),
										},
										values(groups, function(name: string, idx: () -> number)
											local hovered = source(false)
											return create "Frame" {
												Name = "GroupRow_" .. name,
												LayoutOrder = idx,
												Size = UDim2.new(1, 0, 0, 30),
												BackgroundColor3 = function()
													if selectedGroup() == name then
														return C_HOVER()
													elseif hovered() then
														return C_HEADER()
													end
													return Color3.fromRGB(36, 38, 44)
												end,
												BorderSizePixel = 0,
												MouseEnter = function() hovered(true) end,
												MouseLeave = function() hovered(false) end,
												{
													create "ImageButton" {
														Name = "AssignBtn_" .. name,
														Position = UDim2.new(0, 6, 0.5, -10),
														Size = UDim2.fromOffset(20, 20),
														BackgroundTransparency = 1,
														BorderSizePixel = 0,
														Visible = function()
															return name == "Default" or hovered() or selectedGroup() == name
														end,
														Image = ICON_ASSIGN,
														Activated = function()
															assignSelection(name)
														end,
													},
													create "TextButton" {
														Name = "SelectGroup_" .. name,
														Position = UDim2.fromOffset(28, 0),
														Size = UDim2.new(1, -34, 1, 0),
														BackgroundTransparency = 1,
														BorderSizePixel = 0,
														AutoButtonColor = false,
														Font = Enum.Font.SourceSans,
														TextSize = 13,
														TextXAlignment = Enum.TextXAlignment.Right,
														TextTruncate = Enum.TextTruncate.AtEnd,
														TextColor3 = C_TEXT,
														Text = name,
														Activated = function()
															selectedGroup(name)
														end,
													},
												},
											}
										end),
									},
								},
							},
						},

						-- Vertical divider
						create "Frame" {
							Position = UDim2.fromOffset(114, 0),
							Size = UDim2.new(0, 1, 1, 0),
							BackgroundColor3 = C_BORDER,
							BorderSizePixel = 0,
						},

						-- Right GroupListView pane
						create "Frame" {
							Name = "RightPane",
							Position = UDim2.fromOffset(115, 0),
							Size = UDim2.new(1, -115, 1, 0),
							BackgroundTransparency = 1,
							{
								create "Frame" {
									Size = UDim2.new(1, 0, 0, 32),
									BackgroundColor3 = C_HEADER,
									BorderSizePixel = 0,
									{
										create "TextLabel" {
											Position = UDim2.fromOffset(10, 0),
											Size = UDim2.new(1, -20, 1, 0),
											BackgroundTransparency = 1,
											Font = Enum.Font.SourceSansSemibold,
											TextSize = 13,
											TextXAlignment = Enum.TextXAlignment.Left,
											TextColor3 = C_TEXT,
											Text = function()
												local sel = selectedGroup()
												return if sel then `{sel} collides with` else ""
											end,
										},
									},
								},
								show(function() return selectedGroup() == nil end, function()
									return create "TextLabel" {
										Name = "EmptyPrompt",
										Position = UDim2.fromOffset(0, 32),
										Size = UDim2.new(1, 0, 1, -32),
										BackgroundTransparency = 1,
										Font = Enum.Font.SourceSans,
										TextSize = 14,
										TextWrapped = true,
										TextColor3 = C_DIM,
										Text = "Select a group from the left side.",
										{
											create "UIPadding" {
												PaddingLeft = UDim.new(0, 10),
												PaddingRight = UDim.new(0, 10),
											},
										},
									}
								end, function()
									return create "ScrollingFrame" {
										Name = "CollideChecklist",
										Position = UDim2.fromOffset(0, 32),
										Size = UDim2.new(1, 0, 1, -32),
										BackgroundTransparency = 1,
										BorderSizePixel = 0,
										CanvasSize = UDim2.fromOffset(0, 0),
										AutomaticCanvasSize = Enum.AutomaticSize.Y,
										ScrollBarThickness = 5,
										{
											create "UIPadding" {
												PaddingLeft = UDim.new(0, 12),
												PaddingRight = UDim.new(0, 12),
												PaddingTop = UDim.new(0, 8),
											},
											create "UIListLayout" {
												FillDirection = Enum.FillDirection.Vertical,
												SortOrder = Enum.SortOrder.LayoutOrder,
												Padding = UDim.new(0, 6),
											},
											values(groups, function(other: string, oIdx: () -> number)
												return create "Frame" {
													LayoutOrder = oIdx,
													Size = UDim2.new(1, 0, 0, 24),
													BackgroundTransparency = 1,
													{
														create "Frame" {
															Size = UDim2.fromOffset(22, 24),
															BackgroundTransparency = 1,
															{ CollisionCheckbox(selectedGroup, other, "Toggle_" .. other) },
														},
														create "TextLabel" {
															Position = UDim2.fromOffset(28, 0),
															Size = UDim2.new(1, -28, 1, 0),
															BackgroundTransparency = 1,
															Font = Enum.Font.SourceSans,
															TextSize = 14,
															TextXAlignment = Enum.TextXAlignment.Left,
															TextColor3 = C_TEXT,
															Text = other,
														},
													},
												}
											end),
										},
									}
								end),
							},
						},
					},
				}
			end, function()
				-- True 2D Studio Table View matrix
				return create "ScrollingFrame" {
					Name = "TableViewBody",
					Position = UDim2.fromOffset(0, 44),
					Size = UDim2.new(1, 0, 1, -44),
					BackgroundTransparency = 1,
					BorderSizePixel = 0,
					CanvasSize = UDim2.fromOffset(0, 0),
					AutomaticCanvasSize = Enum.AutomaticSize.XY,
					ScrollBarThickness = 6,
					{
						create "UIListLayout" {
							FillDirection = Enum.FillDirection.Vertical,
							SortOrder = Enum.SortOrder.LayoutOrder,
							Padding = UDim.new(0, 1),
						},
						-- Column headers row
						create "Frame" {
							LayoutOrder = 0,
							Size = function()
								return UDim2.fromOffset(110 + #groups() * 68, 30)
							end,
							BackgroundColor3 = C_HEADER,
							BorderSizePixel = 0,
							{
								create "TextLabel" {
									Size = UDim2.fromOffset(110, 30),
									BackgroundTransparency = 1,
									Font = Enum.Font.SourceSans,
									TextSize = 12,
									TextColor3 = C_DIM,
									Text = function() return `{#groups()}/32 Groups` end,
								},
								create "Frame" {
									Position = UDim2.fromOffset(110, 0),
									Size = UDim2.new(1, -110, 1, 0),
									BackgroundTransparency = 1,
									{
										create "UIListLayout" {
											FillDirection = Enum.FillDirection.Horizontal,
											SortOrder = Enum.SortOrder.LayoutOrder,
										},
										values(groups, function(colName: string, cIdx: () -> number)
											return create "TextLabel" {
												LayoutOrder = cIdx,
												Size = UDim2.fromOffset(68, 30),
												BackgroundTransparency = 1,
												Font = Enum.Font.SourceSansSemibold,
												TextSize = 11,
												TextTruncate = Enum.TextTruncate.AtEnd,
												TextColor3 = C_TEXT,
												Text = colName,
											}
										end),
									},
								},
							},
						},
						-- Matrix rows
						values(groups, function(rowName: string, rIdx: () -> number)
							return create "Frame" {
								Name = "MatrixRow_" .. rowName,
								LayoutOrder = rIdx,
								Size = function()
									return UDim2.fromOffset(110 + #groups() * 68, 28)
								end,
								BackgroundColor3 = C_BG,
								BorderSizePixel = 0,
								{
									create "TextLabel" {
										Position = UDim2.fromOffset(8, 0),
										Size = UDim2.fromOffset(100, 28),
										BackgroundTransparency = 1,
										Font = Enum.Font.SourceSans,
										TextSize = 12,
										TextXAlignment = Enum.TextXAlignment.Right,
										TextTruncate = Enum.TextTruncate.AtEnd,
										TextColor3 = C_TEXT,
										Text = rowName,
									},
									create "Frame" {
										Position = UDim2.fromOffset(110, 0),
										Size = UDim2.new(1, -110, 1, 0),
										BackgroundTransparency = 1,
										{
											create "UIListLayout" {
												FillDirection = Enum.FillDirection.Horizontal,
												SortOrder = Enum.SortOrder.LayoutOrder,
											},
											values(groups, function(colName: string, cIdx: () -> number)
												return create "Frame" {
													LayoutOrder = cIdx,
													Size = UDim2.fromOffset(68, 28),
													BackgroundTransparency = 1,
													{ CollisionCheckbox(rowName, colName, `Cell_{rowName}_{colName}`) },
												}
											end),
										},
									},
								},
							}
						end),
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
		unmount = Vide.mount(CollisionGroupsApp, widget)
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
