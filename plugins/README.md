# RusTudio Built-in Plugin Suite (`plugins/`)

Drop lightweight Luau/Vide built-in plugins here to run them directly inside RusTudio's `PluginHost` (at `BuiltInPlugin` security level, zero React-Lua / DeveloperFramework overhead).

## Folder layout

```text
plugins/
├── _packages/            # Shared Luau packages auto-mounted at script.Parent.Packages
│   ├── Vide.luau         # Fine-grained reactive UI (source, derive, effect, create, show, mount)
│   └── StudioTheme.luau  # Reactive settings().Studio.Theme color bindings
└── MyTool/               # Each subfolder with init.server.luau loads as `rustudio_MyTool`
    ├── init.server.luau  # Entry Script
    └── Helpers.luau      # Sibling .luau files become ModuleScripts
```

Single `.luau` / `.lua` / `.rbxm` / `.rbxmx` files directly inside `plugins/` are also supported.

## Minimal Vide plugin example

```lua
local Vide = require(script.Parent.Packages.Vide)
local StudioTheme = require(script.Parent.Packages.StudioTheme)

local toolbar = plugin:CreateToolbar("MyTool")
local button = toolbar:CreateButton("toggle", "My Tool", "")

local widget = plugin:CreateDockWidgetPluginGui(
    "MyToolWidget",
    DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 260, 320, 180, 160)
)
widget.Title = "My Tool"

local count = Vide.source(0)
local unmount = Vide.mount(function()
    return Vide.create "Frame" {
        Size = UDim2.fromScale(1, 1),
        BackgroundColor3 = StudioTheme.color(Enum.StudioStyleGuideColor.MainBackground),
        {
            Vide.create "TextButton" {
                Size = UDim2.new(1, -16, 0, 32),
                Position = UDim2.fromOffset(8, 8),
                BackgroundColor3 = StudioTheme.color(Enum.StudioStyleGuideColor.Button),
                TextColor3 = StudioTheme.color(Enum.StudioStyleGuideColor.MainText),
                Text = function() return `Clicks: {count()}` end,
                Activated = function() count(count() + 1) end,
            },
        },
    }
end, widget)

button.Click:Connect(function()
    widget.Enabled = not widget.Enabled
    button:SetActive(widget.Enabled)
end)

plugin.Unloading:Connect(unmount)
```

Toggle or reload any plugin in this folder live via **Plugins → Manage Plugins → Built-in Suite**.
