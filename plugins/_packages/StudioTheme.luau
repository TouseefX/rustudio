--!strict
--[[
    Reactive Studio theme bridge for RusTudio's bundled Luau/Vide plugins.

    Usage:
      local Vide = require(script.Parent.Packages.Vide)
      local StudioTheme = require(script.Parent.Packages.StudioTheme)

      Vide.create "Frame" {
          BackgroundColor3 = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground),
      }
]]

local Vide = require(script.Parent.Vide)

local StudioTheme = {}

local function current_theme()
	local ok, studio = pcall(function()
		return settings().Studio
	end)
	if ok and studio and studio.Theme then
		return studio.Theme
	end
	return nil
end

local theme_source = Vide.source(current_theme())

pcall(function()
	local studio = settings().Studio
	if studio and studio.ThemeChanged then
		studio.ThemeChanged:Connect(function()
			theme_source(studio.Theme)
		end)
	end
end)

function StudioTheme.theme()
	return theme_source()
end

function StudioTheme.color(
	guideColor: Enum.StudioStyleGuideColor,
	modifier: Enum.StudioStyleGuideModifier?
): () -> Color3
	local mod = modifier or Enum.StudioStyleGuideModifier.Default
	return function(): Color3
		local t = theme_source()
		if t then
			return t:GetColor(guideColor, mod)
		end
		return Color3.fromRGB(46, 46, 46)
	end
end

return StudioTheme
