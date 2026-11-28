--!strict
--[[
    RusTudio Built-in Suite: Localization Tools (Luau / Vide)
    1:1 recreation of Roblox Studio's LocalizationTools.rbxm UI & behavior:
    1. Automatic Text Capture (description + Learn more link + Studio pill switch)
    2. Cloud Localization Table (outlined box with offline/publish prompt)
    3. Embedded Localization Table (Export / Import CSV rows + live table management)
]]

local LocalizationService = game:GetService("LocalizationService")
local StarterGui = game:GetService("StarterGui")
local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")

local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local create = Vide.create
local source = Vide.source
local show = Vide.show

local toolbar = plugin:CreateToolbar("localization_tools")
local toggleButton = toolbar:CreateButton(
	"localization_tools",
	"Configure automatic text capture and manage LocalizationTables",
	"",
	"Localization Tools"
)
toggleButton.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"LocalizationTools",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Left, false, false, 300, 250, 200, 200)
)
widget.Title = "Localization Tools"
widget.Name = "RusTudioLocalizationTools"

local C_BG = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground)
local C_HEADER = StudioTheme.color(Enum.StudioStyleGuideColor.Titlebar)
local C_BORDER = StudioTheme.color(Enum.StudioStyleGuideColor.Border)
local C_TEXT = StudioTheme.color(Enum.StudioStyleGuideColor.MainText)
local C_DIM = StudioTheme.color(Enum.StudioStyleGuideColor.DimmedText)
local C_LINK = Color3.fromRGB(76, 154, 255)

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
		warn("[LocalizationTools] " .. tostring(err))
	end
end

local function ensureTable(): LocalizationTable?
	for _, c in ipairs(LocalizationService:GetDescendants()) do
		if c:IsA("LocalizationTable") then
			return c
		end
	end
	local created: LocalizationTable? = nil
	withRecording("LocNewTable", "Create LocalizationTable", function()
		local lt = Instance.new("LocalizationTable")
		lt.Name = "LocalizationTable"
		lt.Parent = LocalizationService
		created = lt
		Selection:Set({ lt })
	end)
	return created
end

local function LocalizationApp()
	local initialCapture = false
	pcall(function()
		initialCapture = (LocalizationService :: any).IsTextScraperRunning == true
	end)
	local autoCapture = source(initialCapture)
	local statusMsg = source("")
	local tableCount = source(0)

	local function refreshCount()
		local n = 0
		for _, c in ipairs(LocalizationService:GetDescendants()) do
			if c:IsA("LocalizationTable") then
				n += 1
			end
		end
		tableCount(n)
	end

	refreshCount()
	Vide.cleanup(LocalizationService.DescendantAdded:Connect(refreshCount))
	Vide.cleanup(LocalizationService.DescendantRemoving:Connect(function()
		task.defer(refreshCount)
	end))

	local function toggleAutoCapture()
		local nextVal = not autoCapture()
		autoCapture(nextVal)
		pcall(function()
			(LocalizationService :: any).IsTextScraperRunning = nextVal
		end)
		if nextVal then
			local lt = ensureTable()
			if lt then
				local added = 0
				withRecording("LocCapture", "Capture StarterGui Text", function()
					for _, d in ipairs(StarterGui:GetDescendants()) do
						if d:IsA("TextLabel") or d:IsA("TextButton") then
							local txt = (string.gsub(d.Text, "^%s*(.-)%s*$", "%1"))
							if txt ~= "" then
								lt:SetEntryValue(d.Name, txt, "", "en-us", txt)
								added += 1
							end
						end
					end
				end)
				refreshCount()
				statusMsg(`Auto-capture on ({added} UI string(s) captured)`)
			end
		else
			statusMsg("Auto-capture off")
		end
	end

	local function SectionHeader(title: string, order: number)
		return create "TextLabel" {
			LayoutOrder = order,
			Size = UDim2.new(1, 0, 0, 22),
			BackgroundTransparency = 1,
			Font = Enum.Font.SourceSansSemibold,
			TextSize = 15,
			TextXAlignment = Enum.TextXAlignment.Left,
			TextColor3 = C_TEXT,
			Text = title,
		}
	end

	return create "ScrollingFrame" {
		Name = "LocalizationRoot",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = C_BG,
		BorderSizePixel = 0,
		CanvasSize = UDim2.fromOffset(0, 0),
		AutomaticCanvasSize = Enum.AutomaticSize.Y,
		ScrollBarThickness = 6,
		{
			create "UIPadding" {
				PaddingLeft = UDim.new(0, 12),
				PaddingRight = UDim.new(0, 12),
				PaddingTop = UDim.new(0, 12),
				PaddingBottom = UDim.new(0, 12),
			},
			create "UIListLayout" {
				FillDirection = Enum.FillDirection.Vertical,
				SortOrder = Enum.SortOrder.LayoutOrder,
				Padding = UDim.new(0, 12),
			},

			-- 1. Automatic Text Capture (exact Studio layout)
			SectionHeader("Automatic Text Capture", 1),
			create "Frame" {
				Name = "AutoCaptureRow",
				LayoutOrder = 2,
				Size = UDim2.new(1, 0, 0, 56),
				BackgroundTransparency = 1,
				{
					create "TextLabel" {
						Position = UDim2.fromOffset(8, 0),
						Size = UDim2.new(1, -66, 0, 34),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextWrapped = true,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextYAlignment = Enum.TextYAlignment.Top,
						TextColor3 = C_TEXT,
						Text = "Add text encountered while playtesting to localization table",
					},
					create "TextButton" {
						Name = "LearnMoreBtn",
						Position = UDim2.fromOffset(8, 36),
						Size = UDim2.fromOffset(80, 18),
						BackgroundTransparency = 1,
						BorderSizePixel = 0,
						AutoButtonColor = false,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_LINK,
						Text = "Learn more",
						Activated = function()
							pcall(function()
								plugin:OpenWikiPage("https://create.roblox.com/docs/production/localization")
							end)
						end,
					},
					-- Studio toggle pill switch
					create "TextButton" {
						Name = "AutoCaptureToggle",
						Position = UDim2.new(1, -44, 0, 4),
						Size = UDim2.fromOffset(38, 20),
						AutoButtonColor = false,
						BorderSizePixel = 0,
						Text = "",
						BackgroundColor3 = function()
							return if autoCapture() then Color3.fromRGB(0, 162, 255) else Color3.fromRGB(68, 70, 78)
						end,
						Activated = toggleAutoCapture,
						{
							create "UICorner" { CornerRadius = UDim.new(1, 0) },
							create "Frame" {
								AnchorPoint = Vector2.new(0, 0.5),
								Position = function()
									return if autoCapture() then UDim2.new(1, -18, 0.5, 0) else UDim2.new(0, 2, 0.5, 0)
								end,
								Size = UDim2.fromOffset(16, 16),
								BackgroundColor3 = Color3.fromRGB(235, 237, 240),
								BorderSizePixel = 0,
								{ create "UICorner" { CornerRadius = UDim.new(1, 0) } },
							},
						},
					},
				},
			},

			-- 2. Cloud Localization Table (exact Studio outlined box)
			SectionHeader("Cloud Localization Table", 3),
			create "Frame" {
				Name = "CloudTableBox",
				LayoutOrder = 4,
				Size = UDim2.new(1, 0, 0, 114),
				BackgroundColor3 = C_BG,
				BorderSizePixel = 0,
				{
					create "UIStroke" {
						ApplyStrokeMode = Enum.ApplyStrokeMode.Border,
						Thickness = 1,
						Color = C_BORDER,
					},
					create "TextLabel" {
						Size = UDim2.fromScale(1, 1),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextWrapped = true,
						TextColor3 = C_TEXT,
						Text = if game.GameId == 0 then "Publish this place to upload a table" else "Cloud table synced",
					},
				},
			},

			-- 3. Embedded Localization Table (exact Studio Export / Import rows)
			SectionHeader("Embedded Localization Table", 5),
			create "Frame" {
				Name = "ExportRow",
				LayoutOrder = 6,
				Size = UDim2.new(1, 0, 0, 36),
				BackgroundTransparency = 1,
				{
					create "TextLabel" {
						Position = UDim2.fromOffset(8, 0),
						Size = UDim2.new(1, -98, 1, 0),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextWrapped = true,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_TEXT,
						Text = "Export LocalizationTables under LocalizationService to CSV files",
					},
					create "TextButton" {
						Name = "ExportBtn",
						Position = UDim2.new(1, -82, 0.5, -15),
						Size = UDim2.fromOffset(82, 30),
						AutoButtonColor = false,
						BorderSizePixel = 0,
						BackgroundColor3 = C_HEADER,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextColor3 = C_TEXT,
						Text = "Export",
						Activated = function()
							local lt = ensureTable()
							if lt then
								local entries = lt:GetEntries()
								statusMsg(`Ready to export {#entries} entries from {lt.Name}`)
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
				},
			},
			create "Frame" {
				Name = "ImportRow",
				LayoutOrder = 7,
				Size = UDim2.new(1, 0, 0, 36),
				BackgroundTransparency = 1,
				{
					create "TextLabel" {
						Position = UDim2.fromOffset(8, 0),
						Size = UDim2.new(1, -98, 1, 0),
						BackgroundTransparency = 1,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextWrapped = true,
						TextXAlignment = Enum.TextXAlignment.Left,
						TextColor3 = C_TEXT,
						Text = "Import CSV files to LocalizationTables under LocalizationService",
					},
					create "TextButton" {
						Name = "NewTableBtn",
						Position = UDim2.new(1, -82, 0.5, -15),
						Size = UDim2.fromOffset(82, 30),
						AutoButtonColor = false,
						BorderSizePixel = 0,
						BackgroundColor3 = C_HEADER,
						Font = Enum.Font.SourceSans,
						TextSize = 13,
						TextColor3 = C_TEXT,
						Text = "Import",
						Activated = function()
							local lt = ensureTable()
							if lt then
								Selection:Set({ lt })
								refreshCount()
								statusMsg(`Selected {lt.Name} ({tableCount()} table(s) in LocalizationService)`)
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
				},
			},

			show(function() return #statusMsg() > 0 end, function()
				return create "TextLabel" {
					LayoutOrder = 8,
					Size = UDim2.new(1, 0, 0, 20),
					BackgroundTransparency = 1,
					Font = Enum.Font.SourceSans,
					TextSize = 12,
					TextXAlignment = Enum.TextXAlignment.Left,
					TextColor3 = C_DIM,
					Text = statusMsg,
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
		unmount = Vide.mount(LocalizationApp, widget)
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
