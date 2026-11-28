--!strict
--[[
    Vide-compatible fine-grained reactive UI library for RusTudio's Luau plugin suite.

    Zero virtual DOM and zero per-frame reconciliation: updating a `source` only
    re-runs the exact `effect` / property bindings that read it.

    Core API:
      - Vide.source(initial)          -> reactive getter/setter: s() reads, s(next) writes
      - Vide.derive(fn)               -> memoized reactive computation
      - Vide.effect(fn)               -> reactive side-effect (auto-cleaned on re-run / scope exit)
      - Vide.cleanup(fn | Instance | RBXScriptConnection)
      - Vide.root(fn)                 -> creates an ownership scope, returns destroy()
      - Vide.mount(component, target) -> mounts into a Parent Instance, returns unmount()
      - Vide.create(className)        -> returns builder: create "Frame" { Props..., Children... }
      - Vide.show(cond, truthy, falsy?)
      - Vide.indexes(listSrc, transform)
      - Vide.values(listSrc, transform)
      - Vide.changed(prop, callback)  -> binds GetPropertyChangedSignal(prop)
      - Vide.action(callback)         -> runs callback(instance) after construction
      - Vide.batch(fn)                -> coalesces multiple source writes
      - Vide.untrack(fn)              -> reads sources without subscribing
      - Vide.read(valOrSource)        -> unwraps a function source or returns plain value
]]

export type Source<T> = ((...T) -> T)
export type Cleanup = () -> ()

type Scope = {
	parent: Scope?,
	children: { Scope },
	cleanups: { () -> () },
	sources: { [SourceNode]: boolean }?,
	effect: (() -> ())?,
	alive: boolean,
}

type SourceNode = {
	value: any,
	subs: { [Scope]: boolean },
}

local Vide = {}

local current_scope: Scope? = nil
local current_observer: Scope? = nil
local batch_depth = 0
local queued_effects: { Scope } = {}
local queued_set: { [Scope]: boolean } = {}

local CHANGED_TAG = newproxy(false)
local ACTION_TAG = newproxy(false)

local function make_scope(parent: Scope?, effect_fn: (() -> ())?): Scope
	local scope: Scope = {
		parent = parent,
		children = {},
		cleanups = {},
		sources = if effect_fn then {} else nil,
		effect = effect_fn,
		alive = true,
	}
	if parent then
		table.insert(parent.children, scope)
	end
	return scope
end

local function unsubscribe(scope: Scope)
	local sources = scope.sources
	if not sources then
		return
	end
	for node in pairs(sources) do
		node.subs[scope] = nil
	end
	table.clear(sources)
end

local function run_cleanups(scope: Scope)
	-- Destroy nested child scopes first (reverse order).
	for i = #scope.children, 1, -1 do
		local child = scope.children[i]
		child.parent = nil
		if child.alive then
			child.alive = false
			unsubscribe(child)
			run_cleanups(child)
		end
	end
	table.clear(scope.children)

	for i = #scope.cleanups, 1, -1 do
		local fn = scope.cleanups[i]
		local ok, err = pcall(fn)
		if not ok then
			warn("[Vide cleanup] " .. tostring(err))
		end
	end
	table.clear(scope.cleanups)
end

local function destroy_scope(scope: Scope)
	if not scope.alive then
		return
	end
	scope.alive = false
	unsubscribe(scope)
	run_cleanups(scope)
	local parent = scope.parent
	if parent then
		scope.parent = nil
		local idx = table.find(parent.children, scope)
		if idx then
			table.remove(parent.children, idx)
		end
	end
end

local function run_effect_scope(scope: Scope)
	if not scope.alive or not scope.effect then
		return
	end
	unsubscribe(scope)
	run_cleanups(scope)

	local prev_scope = current_scope
	local prev_observer = current_observer
	current_scope = scope
	current_observer = scope
	local ok, err = pcall(scope.effect)
	current_scope = prev_scope
	current_observer = prev_observer
	if not ok then
		warn("[Vide effect] " .. tostring(err))
	end
end

local function flush_effects()
	if batch_depth > 0 then
		return
	end
	local guard = 0
	while #queued_effects > 0 do
		guard += 1
		if guard > 1000 then
			table.clear(queued_effects)
			table.clear(queued_set)
			error("Vide: reactive cycle detected (>1000 flushes)", 2)
		end
		local list = queued_effects
		queued_effects = {}
		table.clear(queued_set)
		for _, scope in ipairs(list) do
			if scope.alive then
				run_effect_scope(scope)
			end
		end
	end
end

local function notify_node(node: SourceNode)
	local snapshot = {}
	for sub in pairs(node.subs) do
		if sub.alive then
			table.insert(snapshot, sub)
		end
	end
	for _, sub in ipairs(snapshot) do
		if not queued_set[sub] then
			queued_set[sub] = true
			table.insert(queued_effects, sub)
		end
	end
	flush_effects()
end

function Vide.source<T>(initial: T): Source<T>
	local node: SourceNode = {
		value = initial,
		subs = {},
	}
	return function(...: any): any
		if select("#", ...) == 0 then
			if current_observer and current_observer.alive and current_observer.sources then
				node.subs[current_observer] = true
				current_observer.sources[node] = true
			end
			return node.value
		else
			local next_val = ...
			if node.value ~= next_val or (next_val ~= next_val and node.value ~= node.value) then
				node.value = next_val
				notify_node(node)
			end
			return next_val
		end
	end
end

function Vide.untrack<T>(fn: () -> T): T
	local prev = current_observer
	current_observer = nil
	local ok, res = pcall(fn)
	current_observer = prev
	if not ok then
		error(res, 2)
	end
	return res
end

function Vide.batch(fn: () -> ())
	batch_depth += 1
	local ok, err = pcall(fn)
	batch_depth -= 1
	if batch_depth == 0 then
		flush_effects()
	end
	if not ok then
		error(err, 2)
	end
end

function Vide.cleanup(item: any)
	if not current_scope then
		return
	end
	local fn: () -> ()
	local t = typeof(item)
	if t == "function" then
		fn = item
	elseif t == "Instance" then
		fn = function()
			if item.Parent ~= nil then
				item:Destroy()
			else
				pcall(function()
					item:Destroy()
				end)
			end
		end
	elseif t == "RBXScriptConnection" then
		fn = function()
			item:Disconnect()
		end
	elseif type(item) == "table" then
		if type(item.Disconnect) == "function" then
			fn = function()
				item:Disconnect()
			end
		elseif type(item.Destroy) == "function" then
			fn = function()
				item:Destroy()
			end
		else
			return
		end
	else
		return
	end
	table.insert(current_scope.cleanups, fn)
end

function Vide.effect(fn: () -> ())
	local scope = make_scope(current_scope, fn)
	run_effect_scope(scope)
end

function Vide.derive<T>(fn: () -> T): () -> T
	local out = Vide.source(nil :: any)
	Vide.effect(function()
		out(fn())
	end)
	return function(): T
		return out()
	end
end

function Vide.read<T>(val: T | () -> T): T
	if type(val) == "function" then
		return (val :: any)()
	end
	return val :: T
end

function Vide.root(fn: (destroy: Cleanup) -> ...any): (Cleanup, ...any)
	local scope = make_scope(nil, nil)
	local function destroy()
		destroy_scope(scope)
	end
	local prev_scope = current_scope
	local prev_observer = current_observer
	current_scope = scope
	current_observer = nil
	local results = table.pack(pcall(fn, destroy))
	current_scope = prev_scope
	current_observer = prev_observer
	if not results[1] then
		destroy()
		error(results[2], 2)
	end
	return destroy, table.unpack(results, 2, results.n)
end

function Vide.changed(property: string, callback: (any) -> ())
	return {
		[CHANGED_TAG] = true,
		property = property,
		callback = callback,
	}
end

function Vide.action(callback: (Instance) -> (), priority: number?)
	return {
		[ACTION_TAG] = true,
		callback = callback,
		priority = priority or 0,
	}
end

local function attach_child(parent: Instance, child: any)
	if child == nil then
		return
	end
	local t = typeof(child)
	if t == "Instance" then
		child.Parent = parent
		return
	end
	if type(child) == "function" then
		local active_instances: { Instance } = {}
		local function clear_all()
			for _, inst in ipairs(active_instances) do
				pcall(function()
					if inst.Parent == parent then
						inst.Parent = nil
					end
				end)
			end
			table.clear(active_instances)
		end
		local function collect(v: any, out: { Instance }, seen: { [Instance]: boolean })
			if v == nil then
				return
			end
			if typeof(v) == "Instance" then
				if not seen[v] then
					seen[v] = true
					table.insert(out, v)
				end
			elseif type(v) == "table" then
				for _, item in ipairs(v) do
					collect(item, out, seen)
				end
			end
		end
		Vide.effect(function()
			local next_instances: { Instance } = {}
			local seen: { [Instance]: boolean } = {}
			collect(child(), next_instances, seen)
			-- Only unparent instances that left the list (avoid reparenting churn).
			for _, inst in ipairs(active_instances) do
				if not seen[inst] then
					pcall(function()
						if inst.Parent == parent then
							inst.Parent = nil
						end
					end)
				end
			end
			for _, inst in ipairs(next_instances) do
				if inst.Parent ~= parent then
					inst.Parent = parent
				end
			end
			active_instances = next_instances
		end)
		Vide.cleanup(clear_all)
		return
	end
	if type(child) == "table" then
		if child[CHANGED_TAG] then
			local prop = child.property
			local cb = child.callback
			local conn = parent:GetPropertyChangedSignal(prop):Connect(function()
				cb((parent :: any)[prop])
			end)
			Vide.cleanup(conn)
			return
		end
		if child[ACTION_TAG] then
			child.callback(parent)
			return
		end
		for _, item in ipairs(child) do
			attach_child(parent, item)
		end
	end
end

local function apply_props(inst: Instance, props: { [any]: any })
	local deferred_parent: any = nil
	local actions: { any } = {}
	local children: { any } = {}

	for k, v in pairs(props) do
		if type(k) == "number" then
			if type(v) == "table" and v[ACTION_TAG] then
				table.insert(actions, v)
			else
				table.insert(children, { idx = k, val = v })
			end
		elseif k == "Parent" then
			deferred_parent = v
		else
			local existing = (inst :: any)[k]
			if typeof(existing) == "RBXScriptSignal" then
				local conn = (existing :: RBXScriptSignal):Connect(v)
				Vide.cleanup(conn)
			elseif type(v) == "function" then
				Vide.effect(function()
					(inst :: any)[k] = v()
				end)
			else
				(inst :: any)[k] = v
			end
		end
	end

	table.sort(children, function(a, b)
		return a.idx < b.idx
	end)
	for _, entry in ipairs(children) do
		attach_child(inst, entry.val)
	end

	if #actions > 1 then
		table.sort(actions, function(a, b)
			return a.priority < b.priority
		end)
	end
	for _, act in ipairs(actions) do
		act.callback(inst)
	end

	if deferred_parent ~= nil then
		if type(deferred_parent) == "function" then
			Vide.effect(function()
				inst.Parent = deferred_parent()
			end)
		else
			inst.Parent = deferred_parent
		end
	end
end

function Vide.create(className: string): (props: { [any]: any }) -> Instance
	return function(props: { [any]: any }): Instance
		local inst = Instance.new(className)
		Vide.cleanup(inst)
		if props then
			apply_props(inst, props)
		end
		return inst
	end
end

function Vide.apply(inst: Instance): (props: { [any]: any }) -> Instance
	return function(props: { [any]: any }): Instance
		if props then
			apply_props(inst, props)
		end
		return inst
	end
end

function Vide.show(
	condition: () -> any,
	truthy: () -> any,
	falsy: (() -> any)?
): () -> any
	local owner_scope = current_scope
	local out = Vide.source(nil :: any)
	local branch_scope: Scope? = nil
	local last_truthy: boolean? = nil

	Vide.effect(function()
		local is_true = not not condition()
		if last_truthy == is_true then
			return
		end
		last_truthy = is_true
		if branch_scope then
			destroy_scope(branch_scope)
			branch_scope = nil
		end
		local builder = if is_true then truthy else falsy
		if builder then
			local scope = make_scope(owner_scope, nil)
			branch_scope = scope
			local prev_scope = current_scope
			local prev_observer = current_observer
			current_scope = scope
			current_observer = nil
			local ok, result = pcall(builder)
			current_scope = prev_scope
			current_observer = prev_observer
			if ok then
				out(result)
			else
				warn("[Vide show] " .. tostring(result))
				out(nil)
			end
		else
			out(nil)
		end
	end)

	return function()
		return out()
	end
end

function Vide.indexes<K, V, R>(
	input: () -> { [K]: V },
	transform: (value: () -> V, key: K) -> R
): () -> { R }
	local owner_scope = current_scope
	local out = Vide.source({} :: { R })
	local entries: { [K]: { scope: Scope, value_src: Source<V>, result: R } } = {}

	Vide.effect(function()
		local map = input() or {}
		for k, entry in pairs(entries) do
			if map[k] == nil then
				destroy_scope(entry.scope)
				entries[k] = nil
			end
		end
		local next_list: { R } = {}
		for k, v in pairs(map) do
			local entry = entries[k]
			if entry then
				entry.value_src(v)
				table.insert(next_list, entry.result)
			else
				local scope = make_scope(owner_scope, nil)
				local value_src = Vide.source(v)
				local prev_scope = current_scope
				local prev_observer = current_observer
				current_scope = scope
				current_observer = nil
				local ok, res = pcall(transform, function()
					return value_src()
				end, k)
				current_scope = prev_scope
				current_observer = prev_observer
				if ok then
					entries[k] = { scope = scope, value_src = value_src, result = res }
					table.insert(next_list, res)
				else
					destroy_scope(scope)
					warn("[Vide indexes] " .. tostring(res))
				end
			end
		end
		out(next_list)
	end)

	return function()
		return out()
	end
end

function Vide.values<K, V, R>(
	input: () -> { [K]: V },
	transform: (value: V, key: () -> K) -> R
): () -> { R }
	local owner_scope = current_scope
	local out = Vide.source({} :: { R })
	local entries: { [V]: { scope: Scope, key_src: Source<K>, result: R } } = {}

	Vide.effect(function()
		local map = input() or {}
		local alive_vals: { [V]: K } = {}
		for k, v in pairs(map) do
			alive_vals[v] = k
		end
		for v, entry in pairs(entries) do
			if alive_vals[v] == nil then
				destroy_scope(entry.scope)
				entries[v] = nil
			end
		end
		local next_list: { R } = {}
		for k, v in pairs(map) do
			local entry = entries[v]
			if entry then
				entry.key_src(k)
				table.insert(next_list, entry.result)
			else
				local scope = make_scope(owner_scope, nil)
				local key_src = Vide.source(k)
				local prev_scope = current_scope
				local prev_observer = current_observer
				current_scope = scope
				current_observer = nil
				local ok, res = pcall(transform, v, function()
					return key_src()
				end)
				current_scope = prev_scope
				current_observer = prev_observer
				if ok then
					entries[v] = { scope = scope, key_src = key_src, result = res }
					table.insert(next_list, res)
				else
					destroy_scope(scope)
					warn("[Vide values] " .. tostring(res))
				end
			end
		end
		out(next_list)
	end)

	return function()
		return out()
	end
end

function Vide.mount(component: () -> any, target: Instance?): Cleanup
	local destroy = Vide.root(function()
		local res = component()
		if target then
			attach_child(target, res)
		end
	end)
	return destroy
end

return Vide
