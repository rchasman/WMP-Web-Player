using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Jint.Native;
using WebSkinPlayer.Media;
using WebSkinPlayer.Skins.Elements;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins;

/// <summary>
/// One VIEW of a skin: its element tree, script engine, bindings, timers and (when open) window.
/// The view is itself the root element, and is what scripts see as the "view" object.
/// </summary>
public sealed class SkinView : SubviewElement
{
    static readonly Dictionary<string, PType> ViewSchema = new(StringComparer.OrdinalIgnoreCase)
    {
        ["title"] = PType.String, ["titlebar"] = PType.Bool, ["resizable"] = PType.Bool,
        ["minwidth"] = PType.Int, ["minheight"] = PType.Int, ["maxwidth"] = PType.Int, ["maxheight"] = PType.Int,
        ["timerinterval"] = PType.Int, ["scriptfile"] = PType.String, ["focusobjectid"] = PType.String,
        ["backgroundcolor"] = PType.String, ["backgroundimage"] = PType.String, ["backgroundtiled"] = PType.Bool,
        ["category"] = PType.String, ["stickyborderwidth"] = PType.Int,
    };
    protected override Dictionary<string, PType>? Schema => ViewSchema;

    public SkinTheme Theme { get; }
    public JsBridge Bridge { get; }
    public SkinSurface Surface { get; }
    public SkinWindow? Window { get; private set; }
    public bool IsOpen => Window != null;
    public bool Loading { get; private set; } = true;

    readonly Dictionary<string, SkinElement> _byId = new(StringComparer.OrdinalIgnoreCase);
    readonly List<SkinElement> _all = new();
    readonly List<(ScriptObject Source, string Prop, Action Apply)> _bindings = new();
    readonly List<(ScriptObject Object, string Prop, CompiledHandler Handler, ScriptObject[] Scopes)> _objectHandlers = new();
    readonly List<(string Event, CompiledHandler Handler, ScriptObject[] Scopes)> _playerHandlers = new();
    readonly Dictionary<TextElement, DispatcherTimer> _scrollers = new();
    readonly List<Anim> _anims = new();
    readonly DispatcherTimer _animTimer;
    DispatcherTimer? _viewTimer;
    int _changeDepth;

    SkinElement? _hover, _pressed, _focus;
    readonly System.Windows.Controls.ToolTip _tip = new();
    readonly DispatcherTimer _tipTimer;

    sealed class Anim
    {
        public required SkinElement Element; public required string Key; public double Ms; public DateTime Start;
        public required Action<double> Step; public required Action Done;
    }

    public SkinView(SkinTheme theme, WmsNode node, bool forPreview = false, List<string>? errorSink = null) : base(null, node)
    {
        Theme = theme;
        _forPreview = forPreview;
        Bridge = new JsBridge($"{System.IO.Path.GetFileName(theme.Path)}:{node.Attr("id") ?? "view"}");
        if (errorSink != null) Bridge.ScriptError += errorSink.Add;
        Surface = new SkinSurface(this);
        _animTimer = new DispatcherTimer(DispatcherPriority.Render) { Interval = TimeSpan.FromMilliseconds(15) };
        _animTimer.Tick += (_, _) => StepAnimations();
        _tipTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(500) };
        _tipTimer.Tick += (_, _) => { _tipTimer.Stop(); ShowTip(); };

        Computed("left", () => Window?.Left ?? (Values.TryGetValue("left", out var l) ? l : 0.0), v => { Values["left"] = ToNumber(v); if (Window != null) Window.Left = ToNumber(v); });
        Computed("top", () => Window?.Top ?? (Values.TryGetValue("top", out var t) ? t : 0.0), v => { Values["top"] = ToNumber(v); if (Window != null) Window.Top = ToNumber(v); });
        Method("close", _ => { Dispatcher.CurrentDispatcher.InvokeAsync(() => Theme.CloseView(this)); return null; });
        Method("minimize", _ => { if (Window != null) Window.WindowState = WindowState.Minimized; return null; });
        Method("maximize", _ => { if (Window != null) Window.WindowState = WindowState.Maximized; return null; });
        Method("restore", _ => { if (Window != null) Window.WindowState = WindowState.Normal; return null; });
        Method("returnToMediaCenter", _ => { Dispatcher.CurrentDispatcher.InvokeAsync(() => App.Modes.ShowFullMode()); return null; });
        Method("returnToMedia", _ => { Dispatcher.CurrentDispatcher.InvokeAsync(() => App.Modes.ShowFullMode()); return null; });
        Method("size", a => { Window?.BeginResize(ToStr(Arg(a, 0))); return null; });
        Method("beginMove", _ => { Window?.BeginMove(); return null; });
        Method("setTimeout", _ => null);

        Build(node);
        LoadScripts();
        EvaluateDeferredAttributes();
        Loading = false;
        CaptureLayoutBaseline();
        if (!forPreview) Theme.Objects.PlayerEvent += OnPlayerEvent;
    }

    public override string ClassName => "view";
    public new string? Id => Node.Attr("id");
    readonly bool _forPreview;
    public IEnumerable<SkinElement> AllElements => _all;

    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "titlebar" => true,
        "resizable" => true,
        "title" => Theme.Title,
        _ => base.ElementDefault(name),
    };

    // ---- building ------------------------------------------------------------------------

    static readonly HashSet<string> PseudoTags = new(StringComparer.OrdinalIgnoreCase)
    {
        "PLAYER", "CONTROLS", "SETTINGS", "NETWORK", "CURRENTMEDIA", "MEDIACENTER", "CURRENTPLAYLIST", "DVD", "ERROR", "CDROMCOLLECTION",
    };

    readonly List<(SkinElement El, string Attr, string Value)> _deferred = new();

    void Build(WmsNode viewNode)
    {
        int order = 0;
        ApplyAttributes(this, viewNode);
        foreach (var child in viewNode.Children) BuildNode(child, this, ref order);

        // Globals: element ids, then the WMP objects.
        var o = Theme.Objects;
        // WMP's Skin Chooser ran the preview without a player object; some skins rely on that
        // ("if(player){}" throws there, leaving a dedicated preview image on screen).
        if (!_forPreview) Bridge.SetGlobal("player", o.Player);
        Bridge.SetGlobal("mediacenter", o.MediaCenter);
        Bridge.SetGlobal("mediaCenter", o.MediaCenter);
        Bridge.SetGlobal("theme", o.Theme);
        Bridge.SetGlobal("event", o.Event);
        Bridge.SetGlobal("view", this);
        foreach (var name in typeof(WmpState).GetFields().Select(f => f.Name))
            Bridge.Engine.SetValue(name, Convert.ToDouble(typeof(WmpState).GetField(name)!.GetValue(null)));
        foreach (var (id, el) in _byId) Bridge.SetGlobal(id, el);
        if (Id != null) Bridge.SetGlobal(Id, this);
    }

    void BuildNode(WmsNode n, SkinElement parent, ref int order)
    {
        if (PseudoTags.Contains(n.Tag))
        {
            BuildPseudo(n, Array.Empty<ScriptObject>());
            return;
        }
        var el = Create(n, parent);
        if (el == null) return;
        el.Parent = parent;
        el.Order = order++;
        parent.Children.Add(el);
        _all.Add(el);
        ApplyPresets(el);
        ApplyAttributes(el, n);
        // WMP applies attributes in order: a repeated property keeps its last value, and every id names the
        // element, so one given two ids answers to both (Official Xbox's intro uses both names).
        foreach (var a in n.Attributes)
            if (a.Key.Equals("id", StringComparison.OrdinalIgnoreCase) && a.Value.Length > 0) _byId[a.Value] = el;
        foreach (var c in n.Children) BuildNode(c, el, ref order);
    }

    SkinElement? Create(WmsNode n, SkinElement parent)
    {
        string t = n.Tag;
        switch (t)
        {
            case "SUBVIEW": return new SubviewElement(this, n);
            case "BUTTONGROUP": return new ButtonGroupElement(this, n);
            case "SLIDER": case "SEEKSLIDER": case "VOLUMESLIDER": case "BALANCESLIDER": case "PROGRESSBAR": return new SliderElement(this, n);
            case "CUSTOMSLIDER": return new CustomSliderElement(this, n);
            case "TEXT": case "STATUSTEXT": case "CURRENTPOSITIONTEXT": case "METADATATEXT": case "DURATIONTEXT": return new TextElement(this, n);
            case "EFFECTS": case "WMPEFFECTS": return new EffectsElement(this, n);
            case "VIDEO": case "WMPVIDEO": return new VideoElement(this, n);
            case "PLAYLIST": case "ITEMSPLAYLIST": case "DROPDOWNPLAYLIST": case "LISTBOX": return new PlaylistElement(this, n);
            case "EQUALIZERSETTINGS": return new EqualizerSettingsElement(this, n);
            case "VIDEOSETTINGS": return new VideoSettingsElement(this, n);
            case "POPUP": return new PopupElement(this, n);
            case "AUTOMENU": return new AutoMenuElement(this, n);
            case "VIEW": case "THEME": return null;
        }
        if (parent is ButtonGroupElement && (t == "BUTTONELEMENT" || t.EndsWith("ELEMENT"))) return new GroupButtonElement(this, n);
        if (t == "BUTTON" || t.EndsWith("BUTTON")) return new ButtonElement(this, n);
        if (t.EndsWith("ELEMENT")) return new GroupButtonElement(this, n);
        Log.Write($"[{System.IO.Path.GetFileName(Theme.Path)}] unknown element <{t}> line {n.Line}; treating as container");
        return n.Children.Count > 0 ? new SubviewElement(this, n) : new NonVisualElement(this, n);
    }

    void ApplyPresets(SkinElement el)
    {
        var n = el.Node;
        bool Has(string a) => n.Attr(a) != null;
        var preset = ButtonPresets.For(el.Tag);
        if (preset != null)
        {
            if (!Has("onclick")) AddHandler(el, "onclick", preset.Action);
            if (!Has("enabled") && preset.Enabled != null) _deferred.Add((el, "enabled", preset.Enabled));
            if (!Has("uptooltip") && !Has("tooltip")) el.SetValue("uptooltip", preset.ToolTip, false);
            if (preset.Sticky && !Has("sticky")) el.SetValue("sticky", true, false);
            if (preset.Down != null && !Has("down")) _deferred.Add((el, "down", preset.Down));
        }
        switch (el.Tag)
        {
            case "SEEKSLIDER":
                if (!Has("min")) el.SetValue("min", 0.0, false);
                if (!Has("max")) _deferred.Add((el, "max", "wmpprop:player.currentMedia.duration"));
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.controls.currentPosition"));
                if (!Has("ondragend")) AddHandler(el, "ondragend", "player.controls.currentPosition = value;");
                if (!Has("enabled")) _deferred.Add((el, "enabled", "wmpenabled:player.controls.currentPosition"));
                if (!Has("tooltip")) el.SetValue("tooltip", "Seek", false);
                break;
            case "VOLUMESLIDER":
                if (!Has("min")) el.SetValue("min", 0.0, false);
                if (!Has("max")) el.SetValue("max", 100.0, false);
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.settings.volume"));
                if (!Has("value_onchange")) AddHandler(el, "value_onchange", "player.settings.volume = value;");
                if (!Has("tooltip")) el.SetValue("tooltip", "Volume", false);
                break;
            case "BALANCESLIDER":
                if (!Has("min")) el.SetValue("min", -100.0, false);
                if (!Has("max")) el.SetValue("max", 100.0, false);
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.settings.balance"));
                if (!Has("value_onchange")) AddHandler(el, "value_onchange", "player.settings.balance = value;");
                if (!Has("tooltip")) el.SetValue("tooltip", "Balance", false);
                break;
            case "STATUSTEXT":
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.status"));
                break;
            case "CURRENTPOSITIONTEXT":
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.controls.currentPositionString"));
                break;
            case "DURATIONTEXT":
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.currentMedia.durationString"));
                break;
            case "METADATATEXT":
                if (!Has("value")) _deferred.Add((el, "value", "wmpprop:player.currentMedia.name"));
                break;
        }
    }

    static bool IsHandlerName(string name) =>
        name.EndsWith("_onchange", StringComparison.OrdinalIgnoreCase) ||
        (name.Length > 2 && name.StartsWith("on", StringComparison.OrdinalIgnoreCase) && char.IsLetter(name[2]));

    void ApplyAttributes(SkinElement el, WmsNode n)
    {
        foreach (var (name, raw) in n.Attributes)
        {
            if (IsHandlerName(name)) { AddHandler(el, name, raw); continue; }
            if (name.Equals("scriptfile", StringComparison.OrdinalIgnoreCase) && el != this) continue;
            string v = raw.Trim();
            if (v.StartsWith("jscript:", StringComparison.OrdinalIgnoreCase) || v.StartsWith("wmpprop:", StringComparison.OrdinalIgnoreCase) ||
                v.StartsWith("wmpenabled:", StringComparison.OrdinalIgnoreCase) || v.StartsWith("wmpdisabled:", StringComparison.OrdinalIgnoreCase))
                _deferred.Add((el, name, v));
            // A string resource stands for its text; scriptFile is a list of files that may start with one (Summate).
            else if (WmpResources.IsResourceUrl(v) && !IsImageAttribute(name) && !name.Equals("scriptfile", StringComparison.OrdinalIgnoreCase))
                el.SetValue(name, WmpResources.ReadText(v) ?? "", false);
            else
                el.SetValue(name, raw, false);
        }
    }

    static bool IsImageAttribute(string name) => name.EndsWith("image", StringComparison.OrdinalIgnoreCase) || name.Equals("cursor", StringComparison.OrdinalIgnoreCase);

    void AddHandler(SkinElement el, string name, string code)
    {
        var h = Bridge.Compile(code, $"{el.Tag}{(el.Id != null ? "#" + el.Id : "")}.{name}");
        if (h != null) el.Handlers[name.ToLowerInvariant()] = h;
    }

    /// <summary>&lt;PLAYER playStateChange="..." status_onchange="..."&gt; and its nested CONTROLS/NETWORK/...</summary>
    void BuildPseudo(WmsNode n, ScriptObject[] outer)
    {
        var o = Theme.Objects;
        ScriptObject? target = n.Tag switch
        {
            "PLAYER" => o.Player, "CONTROLS" => o.Controls, "SETTINGS" => o.Settings, "NETWORK" => o.Network,
            "CURRENTMEDIA" => o.CurrentMedia, "MEDIACENTER" => o.MediaCenter, "CURRENTPLAYLIST" => o.CurrentPlaylist,
            "DVD" => o.Dvd, "ERROR" => o.Error, "CDROMCOLLECTION" => o.Cdroms, _ => null,
        };
        if (target == null) return;
        var scopes = outer.Append(target).ToArray();
        foreach (var (name, code) in n.Attributes)
        {
            if (name.EndsWith("_onchange", StringComparison.OrdinalIgnoreCase))
            {
                string prop = name[..^9];
                var h = CompileScoped(code, $"{n.Tag}.{name}", scopes.Length, Array.Empty<string>());
                if (h != null) _objectHandlers.Add((target, prop.ToLowerInvariant(), h, scopes));
            }
            else if (n.Tag == "PLAYER" || name.StartsWith("on", StringComparison.OrdinalIgnoreCase))
            {
                string ev = name.StartsWith("on", StringComparison.OrdinalIgnoreCase) && !WmpObjects.EventParameters.ContainsKey(name) ? name[2..] : name;
                var pars = WmpObjects.EventParameters.TryGetValue(ev, out var p) ? p : Array.Empty<string>();
                var h = CompileScoped(code, $"{n.Tag}.{name}", scopes.Length, pars);
                if (h != null) _playerHandlers.Add((ev, h, scopes));
            }
        }
        foreach (var c in n.Children) BuildPseudo(c, scopes);
    }

    CompiledHandler? CompileScoped(string code, string name, int scopeCount, string[] parameters)
    {
        // Nested with() so <NETWORK> handlers can still say "network.bufferingProgress".
        code = JScriptCompat.StripPrefix(code);
        if (string.IsNullOrWhiteSpace(code)) return null;
        var scopeNames = Enumerable.Range(0, scopeCount).Select(i => "__s" + i).ToArray();
        string withs = string.Concat(scopeNames.Select(s => $"with({s})"));
        string src = $"(function({string.Join(",", scopeNames.Concat(parameters))}){{ {withs}{{ {JScriptCompat.Prepare(code)}\n}} }})";
        try
        {
            var fn = Bridge.Engine.Evaluate(src, name);
            return new CompiledHandler { Function = fn, Source = code, Name = name };
        }
        catch (Exception ex)
        {
            Log.Write($"[{System.IO.Path.GetFileName(Theme.Path)}] compile error in {name}: {ex.Message}");
            return null;
        }
    }

    object? InvokeScoped(CompiledHandler h, ScriptObject[] scopes, object?[] args)
    {
        var all = scopes.Cast<object?>().Concat(args).ToArray();
        return Bridge.CallFunction(h.Function, null, all);
    }

    void LoadScripts()
    {
        var sources = new List<string>();
        string files = GetString("scriptfile");
        // Without scriptFile, WMP loaded the script named like the skin definition (skin.wms → skin.js).
        if (!HasValue("scriptfile"))
        {
            string sameName = System.IO.Path.ChangeExtension(Theme.Package.DefinitionFile, ".js");
            if (Theme.Package.Exists(sameName)) files = sameName;
        }
        foreach (var f in files.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            string? code = WmpResources.IsResourceUrl(f) ? WmpResources.ReadText(f) : Theme.Package.Exists(f) ? Theme.Package.ReadText(f) : null;
            if (code == null) { Log.Write($"[{System.IO.Path.GetFileName(Theme.Path)}] script not found: {f}"); continue; }
            Bridge.Execute(code, f);
            sources.Add(code);
        }
        // Handler and jscript: attribute code, too.
        void Collect(WmsNode n)
        {
            foreach (var a in n.Attributes) if (IsHandlerName(a.Key) || a.Value.StartsWith("jscript:", StringComparison.OrdinalIgnoreCase)) sources.Add(a.Value);
            foreach (var c in n.Children) Collect(c);
        }
        Collect(Node);
        Bridge.AddCaseAliases(sources);
    }

    void EvaluateDeferredAttributes()
    {
        // jscript: first (document order), then live bindings.
        foreach (var (el, attr, v) in _deferred.Where(d => d.Value.StartsWith("jscript:", StringComparison.OrdinalIgnoreCase)).ToList())
        {
            var value = Bridge.EvaluateIn(el, v[8..], $"{el}.{attr}");
            if (value is JsValue js && !(js is Jint.Native.Function.Function)) value = null;
            if (value != null || !el.HasValue(attr)) el.SetValue(attr, value, false);
        }
        foreach (var (el, attr, v) in _deferred.Where(d => !d.Value.StartsWith("jscript:", StringComparison.OrdinalIgnoreCase)).ToList())
            Bind(el, attr, v);
        _deferred.Clear();

        // Element *_onchange handlers and redraws.
        foreach (var el in _all.Prepend(this)) el.Changed += OnElementChanged;
        foreach (var h in _objectHandlers)
            h.Object.Changed += (src, prop) => { if (prop == h.Prop && !Loading && IsOpen) InvokeScoped(h.Handler, h.Scopes, Array.Empty<object?>()); };
    }

    void OnElementChanged(ScriptObject src, string prop)
    {
        if (Loading || src is not SkinElement el) return;
        if (el.Handlers.ContainsKey(prop + "_onchange") && _changeDepth < 8)
        {
            _changeDepth++;
            try { el.Fire(prop + "_onchange"); } finally { _changeDepth--; }
        }
        if (el == this && prop is "width" or "height") Window?.SyncSizeFromView();
        if (el == this && prop == "title" && Window != null) Window.Title = GetString("title");
        if (el == this && prop == "timerinterval") RestartTimer();
        if (prop == "visible") foreach (var e in _all.OfType<EffectsElement>()) e.RefreshSubscription();
        if (prop == "visible" || prop == "width") foreach (var t in _all.OfType<TextElement>()) UpdateScrolling(t);
    }

    // ---- bindings --------------------------------------------------------------------------

    ScriptObject? ResolveRoot(string name)
    {
        if (_byId.TryGetValue(name, out var el)) return el;
        var o = Theme.Objects;
        switch (name.ToLowerInvariant())
        {
            case "player": return o.Player;
            case "mediacenter": return o.MediaCenter;
            case "theme": return o.Theme;
            case "view": return this;
            case "event": return o.Event;
        }
        if (Id != null && Id.Equals(name, StringComparison.OrdinalIgnoreCase)) return this;
        return Bridge.FromJs(Bridge.GetGlobal(name)) as ScriptObject;
    }

    void Bind(SkinElement el, string attr, string spec)
    {
        int colon = spec.IndexOf(':');
        string kind = spec[..colon].ToLowerInvariant();
        string path = spec[(colon + 1)..].Trim();
        var parts = path.Split('.', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length == 0) return;

        ScriptObject? obj = ResolveRoot(parts[0]);
        int last = parts.Length - 1;
        for (int i = 1; i < last && obj != null; i++) obj = obj.Get(parts[i]) as ScriptObject;
        if (obj == null || parts.Length < 2)
        {
            Log.Write($"[{System.IO.Path.GetFileName(Theme.Path)}] can't bind {el}.{attr} to {spec}");
            return;
        }
        var source = obj;
        string prop = parts[last];

        Action apply;
        string watch;
        if (kind == "wmpprop")
        {
            watch = prop.ToLowerInvariant();
            apply = () =>
            {
                if (attr.Equals("value", StringComparison.OrdinalIgnoreCase) && el is SliderElement { Dragging: true }) return;
                el.SetValue(attr, source.Get(prop), notify: !Loading);
            };
        }
        else
        {
            // wmpenabled:player.controls.play → controls.isAvailable("play")
            bool invert = kind == "wmpdisabled";
            watch = "isavailable";
            apply = () =>
            {
                bool on = source.TryGetMethod("isAvailable", out var m) ? ToBool(m(new object?[] { prop })) : ToBool(source.Get(prop));
                el.SetValue(attr, invert ? !on : on, notify: !Loading);
            };
        }
        apply();
        source.Changed += (_, changed) => { if (changed == watch) apply(); };
        _bindings.Add((source, watch, apply));
    }

    // ---- player events ---------------------------------------------------------------------

    void OnPlayerEvent(string name, object?[] args)
    {
        if (Loading || !IsOpen) return;
        foreach (var h in _playerHandlers)
            if (h.Event.Equals(name, StringComparison.OrdinalIgnoreCase))
                InvokeScoped(h.Handler, h.Scopes, args);
    }

    // ---- lifetime --------------------------------------------------------------------------

    public void Open(double? x, double? y)
    {
        Window = new SkinWindow(this);
        Window.Place(x, y);
        foreach (var el in _all) el.OnLoaded();
        if (!Sandbox.Enabled) Window.Show();
        RestartTimer();
        foreach (var t in _all.OfType<TextElement>()) UpdateScrolling(t);
        Fire("onload");
        if (Window == null) return; // the view closed or switched away during onLoad
        string focus = GetString("focusobjectid");
        if (focus.Length > 0 && _byId.TryGetValue(focus, out var f)) Focus(f);
        InvalidateVisual();
    }

    /// <summary>Loads the view offscreen (for skin previews): runs onLoad but never shows a window.</summary>
    public void LoadForPreview() => Fire("onload");

    public void CloseWindow()
    {
        Fire("onclose");
        _viewTimer?.Stop();
        _animTimer.Stop();
        _frameTimer?.Stop();
        foreach (var t in _scrollers.Values) t.Stop();
        _scrollers.Clear();
        _tip.IsOpen = false;
        Theme.Objects.PlayerEvent -= OnPlayerEvent;
        foreach (var el in _all) el.OnUnloaded();
        var w = Window;
        Window = null;
        w?.CloseFromView();
    }

    void RestartTimer()
    {
        _viewTimer?.Stop();
        int ms = GetInt("timerinterval");
        if (ms <= 0 || !HasHandler("ontimer") || !IsOpen) return;
        _viewTimer ??= new DispatcherTimer(DispatcherPriority.Background);
        _viewTimer.Interval = TimeSpan.FromMilliseconds(Math.Max(15, ms));
        _viewTimer.Tick -= OnViewTimer;
        _viewTimer.Tick += OnViewTimer;
        _viewTimer.Start();
    }

    void OnViewTimer(object? s, EventArgs e) => Fire("ontimer");

    public void UpdateScrolling(TextElement t)
    {
        if (!IsOpen) return;
        bool want = t.IsScrolling;
        if (want && !_scrollers.ContainsKey(t))
        {
            var timer = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(Math.Max(15, t.GetInt("scrollingdelay"))) };
            timer.Tick += (_, _) => t.ScrollStep();
            _scrollers[t] = timer;
            timer.Start();
        }
        else if (!want && _scrollers.Remove(t, out var old)) old.Stop();
        else if (want && _scrollers.TryGetValue(t, out var cur)) cur.Interval = TimeSpan.FromMilliseconds(Math.Max(15, t.GetInt("scrollingdelay")));
    }

    public void Animate(SkinElement el, string key, double ms, Action<double> step, Action done)
    {
        _anims.RemoveAll(a => a.Element == el && a.Key == key);
        _anims.Add(new Anim { Element = el, Key = key, Ms = ms, Start = DateTime.UtcNow, Step = step, Done = done });
        _animTimer.Start();
    }

    void StepAnimations()
    {
        var now = DateTime.UtcNow;
        foreach (var a in _anims.ToArray())
        {
            double t = Math.Clamp((now - a.Start).TotalMilliseconds / a.Ms, 0, 1);
            a.Step(t);
            if (t >= 1) { _anims.Remove(a); a.Done(); }
        }
        if (_anims.Count == 0) _animTimer.Stop();
    }

    public void InvalidateVisual()
    {
        _maskDirty = true;
        Surface.InvalidateVisual();
    }

    /// <summary>Redraw for changes that can't alter the window shape (marquee scrolling).</summary>
    public void InvalidateContent() => Surface.InvalidateVisual();

    DispatcherTimer? _frameTimer;
    DateTime _frameDue = DateTime.MaxValue;

    /// <summary>An animated GIF needs a redraw in <paramref name="ms"/> milliseconds.</summary>
    public void RequestAnimationFrame(double ms)
    {
        if (!IsOpen) return;
        var due = DateTime.UtcNow.AddMilliseconds(Math.Max(15, ms));
        if (_frameTimer != null && _frameTimer.IsEnabled && due >= _frameDue) return;
        _frameDue = due;
        if (_frameTimer == null)
        {
            _frameTimer = new DispatcherTimer(DispatcherPriority.Render);
            _frameTimer.Tick += (_, _) => { _frameTimer!.Stop(); _frameDue = DateTime.MaxValue; InvalidateContent(); };
        }
        _frameTimer.Stop();
        _frameTimer.Interval = TimeSpan.FromMilliseconds(Math.Max(15, ms));
        _frameTimer.Start();
    }

    public void Focus(SkinElement? el)
    {
        if (_focus == el) return;
        _focus?.Fire("onblur");
        _focus = el;
        el?.Fire("onfocus");
    }

    // ---- rendering ------------------------------------------------------------------------

    // WMP keyed the composed window (like a layered window colour key). A VIEW's clippingColor and
    // transparencyColor cut every pixel of that colour, whichever element drew it. A SUBVIEW's
    // transparencyColor cut only its holes, so a button or slider painted in that colour over a
    // hole was cut too (World of Warcraft's sliders). WMP also filled a frameless window's empty
    // background with magenta and keyed it out, so magenta vanished wherever it was drawn (WWC's
    // intro over its pipes).
    const int Magenta = 0xFF00FF;
    ImageBrush? _compositeMask;
    bool _maskDirty = true;
    DateTime _maskBuilt;
    DispatcherTimer? _maskTimer;

    (int? clip, int? trans) ViewKeys =>
        (GetString("clippingimage").Length == 0 ? SkinImages.ParseColor(GetString("clippingcolor")) : null,
         SkinImages.ParseColor(GetString("transparencycolor")));

    bool KeysWindow { get { var (c, t) = ViewKeys; return c != null || t != null; } }

    bool UsesCompositeMask => !GetBool("titlebar") && Width > 0 && Height > 0;

    bool HasKeyedSubview => _all.Any(e => e is SubviewElement s && Shown(s) &&
        SkinImages.ParseColor(s.GetString("transparencycolor")) != null && s.GetString("backgroundimage").Length > 0);

    /// <summary>Whether anything shown may paint magenta. Visualizations and video are left out: WMP never keyed them.</summary>
    bool MayShowMagenta => _all.Prepend(this).Any(e => e is not EffectsElement && Shown(e) && e.MayPaint(Magenta));

    /// <summary>Visible up to the VIEW, whose own visible="false" WMP ignored.</summary>
    static bool Shown(SkinElement e)
    {
        for (SkinElement? p = e; p is { } and not SkinView; p = p.Parent) if (!p.Visible) return false;
        return true;
    }

    /// <summary>A VIEW's keys cut the window only without a title bar; otherwise its clipping mask shapes it.</summary>
    bool ShapedByClipMask => GetBool("titlebar") || !KeysWindow;

    public void RenderTree(DrawingContext dc)
    {
        bool viewMask = ShapedByClipMask;
        if (UsesCompositeMask)
        {
            EnsureCompositeMask(viewMask);
            if (_compositeMask != null)
            {
                dc.PushOpacityMask(_compositeMask);
                RenderElement(dc, this, viewMask);
                dc.Pop();
                return;
            }
        }
        RenderElement(dc, this, viewMask);
    }

    void EnsureCompositeMask(bool viewMask)
    {
        if (!_maskDirty) return;
        // Rebuilding means a software render of the whole view; cap it at ~10 per second.
        if (DateTime.UtcNow - _maskBuilt < TimeSpan.FromMilliseconds(100))
        {
            if (_maskTimer == null)
            {
                _maskTimer = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(110) };
                _maskTimer.Tick += (_, _) => { _maskTimer!.Stop(); Surface.InvalidateVisual(); };
            }
            if (!_maskTimer.IsEnabled) _maskTimer.Start();
            return;
        }
        _maskDirty = false;
        _maskBuilt = DateTime.UtcNow;
        // A window keyed only by magenta needs no software render while nothing shown can paint it: New Super
        // Mario Bros redraws ten times a second and shows none.
        if (!KeysWindow && !HasKeyedSubview && !MayShowMagenta) { _compositeMask = null; return; }
        int w = Math.Min(4000, Width), h = Math.Min(4000, Height);
        var drawn = new DrawPass();
        var px = RenderPixels(new Int32Rect(0, 0, w, h), viewMask, drawn);
        var holes = FindHoles(drawn, px, w, h, viewMask);
        var (clip, trans) = ViewKeys;
        // Only key-coloured pixels are cut; everything else stays unmasked so content that
        // changes between rebuilds (GIF frames, visualizers) is never hidden by a stale mask.
        bool any = false;
        for (int i = 0; i < px.Length; i++)
        {
            int c = px[i];
            int a = (c >>> 24) & 255, rgb = c & 0xFFFFFF, hole = holes?[i] ?? 0;
            bool keyed = a == 255 && ((clip is int ck && SkinImages.MatchesKey(rgb, ck)) || (trans is int tk && SkinImages.MatchesKey(rgb, tk)) ||
                (hole != 0 && SkinImages.MatchesKey(rgb, hole & 0xFFFFFF)) || SkinImages.MatchesKey(rgb, Magenta));
            px[i] = keyed ? 0 : unchecked((int)0xFF000000);
            any |= keyed;
        }
        if (!any) { _compositeMask = null; return; }
        var mask = BitmapSource.Create(w, h, 96, 96, PixelFormats.Pbgra32, null, px, w * 4);
        mask.Freeze();
        var brush = new ImageBrush(mask) { ViewportUnits = BrushMappingMode.Absolute, Viewport = new Rect(0, 0, w, h), Stretch = Stretch.Fill };
        brush.Freeze();
        _compositeMask = brush;
    }

    /// <summary>
    /// A software render of <paramref name="area"/> with nothing cut, leaving out <paramref name="hidden"/>
    /// without changing it (the test run's window checks compare the two).
    /// </summary>
    public int[] RenderArea(Int32Rect area, SkinElement? hidden = null) =>
        RenderPixels(area, ShapedByClipMask, new DrawPass { Hidden = hidden });

    /// <summary>A software render of the view: the elements in the order they drew, where to stop, and one to leave out.</summary>
    sealed class DrawPass
    {
        public SkinElement? StopAfter;
        public SkinElement? Hidden;
        public bool Stopped;
        public readonly List<(SkinElement El, bool Backdrop)> Steps = new();
    }

    int[] RenderPixels(Int32Rect area, bool viewMask, DrawPass pass)
    {
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen())
        {
            dc.PushTransform(new TranslateTransform(-area.X, -area.Y));
            RenderElement(dc, this, viewMask, pass);
            dc.Pop();
        }
        var rtb = new RenderTargetBitmap(area.Width, area.Height, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        var px = new int[area.Width * area.Height];
        rtb.CopyPixels(px, area.Width * 4, 0);
        return px;
    }

    const int HoleBit = 1 << 24;

    /// <summary>
    /// Per pixel, the transparencyColor a subview left open there with nothing under it (with
    /// <see cref="HoleBit"/> set, so a black key counts), or 0. Replays <paramref name="drawn"/>: a
    /// subview's image opens holes where it shows its key over nothing and fills them where it is
    /// visible; its backgroundColor, EFFECTS and VIDEO fill them too (WMP showed visualizations and
    /// video in windows of their own, never keyed with the skin). Null when there is none.
    /// </summary>
    int[]? FindHoles(DrawPass drawn, int[] final, int w, int h, bool viewMask)
    {
        // A hole is cut only where the window shows its key, so most views need no search at all.
        var keys = drawn.Steps.Select(s => s.El).OfType<SubviewElement>().Where(s => s != this)
            .Select(s => SkinImages.ParseColor(s.GetString("transparencycolor"))).OfType<int>().Distinct().ToArray();
        if (!final.Any(c => (c >>> 24) == 255 && keys.Any(k => SkinImages.MatchesKey(c & 0xFFFFFF, k)))) return null;
        int[]? holes = null;
        var view = new Rect(0, 0, w, h);
        foreach (var (el, backdrop) in drawn.Steps)
        {
            bool fills = backdrop ? el is SubviewElement { BackgroundColor: not null } : el is EffectsElement;
            var opener = !backdrop && el is SubviewElement sub && sub != this ? sub : null;
            if (!fills && opener == null) continue;
            var bounds = el.AbsoluteBounds;
            var shown = bounds;
            for (var p = el.Parent; p != null; p = p.Parent) if (p.ClipsChildren) shown.Intersect(p.AbsoluteBounds);
            shown.Intersect(view);
            if (shown.IsEmpty) continue;
            int x0 = (int)Math.Floor(shown.Left), y0 = (int)Math.Floor(shown.Top);
            var area = new Int32Rect(x0, y0, (int)Math.Ceiling(shown.Right) - x0, (int)Math.Ceiling(shown.Bottom) - y0);
            if (area.Width <= 0 || area.Height <= 0) continue;
            IEnumerable<int> Pixels() =>
                Enumerable.Range(area.Y, area.Height).SelectMany(y => Enumerable.Range(area.X, area.Width).Select(x => y * w + x));
            if (opener == null)
            {
                if (holes != null) foreach (int i in Pixels()) holes[i] = 0;
                continue;
            }
            int? key = SkinImages.ParseColor(opener.GetString("transparencycolor"));
            if ((key == null && holes == null) || opener.BackdropMap() is not { } at) continue;
            var looks = Pixels().Select(i => (i, at(new Point(i % w + 0.5 - bounds.X, i / w + 0.5 - bounds.Y)))).ToArray();
            // What is drawn under the open pixels only matters where the final window shows the key
            // or an earlier hole could be replaced, so most keyed subviews need no extra render.
            bool open = looks.Any(l => l.Item2 == Backdrop.Open);
            bool matters = open && key is int openKey && looks.Any(l => holes?[l.i] is > 0 || ((final[l.i] >>> 24) == 255 && SkinImages.MatchesKey(final[l.i] & 0xFFFFFF, openKey)));
            int[]? under = matters ? RenderPixels(area, viewMask, new DrawPass { StopAfter = opener }) : null;
            foreach (var (i, look) in looks)
            {
                if (look == Backdrop.Open && key is int k && under != null && (under[(i / w - area.Y) * area.Width + i % w - area.X] >>> 24) == 0)
                {
                    holes ??= new int[w * h];
                    holes[i] = k | HoleBit;
                }
                else if (look == Backdrop.Covered && holes != null) holes[i] = 0;
            }
        }
        return holes;
    }

    void RenderElement(DrawingContext dc, SkinElement e, bool viewMask = true, DrawPass? pass = null)
    {
        // A VIEW is the window itself; WMP ignored visible="false" on it.
        if ((!e.Visible && e != this) || pass?.Stopped == true || (pass != null && e == pass.Hidden)) return;
        var b = e == this ? new Rect(0, 0, Width, Height) : e.AbsoluteBounds;
        int pushes = 0;
        double op = e.Opacity;
        if (op < 0.999) { dc.PushOpacity(op); pushes++; }
        var mask = e == this && !viewMask ? null : e.ClipMask;
        if (mask != null)
        {
            var brush = new System.Windows.Media.ImageBrush(mask.Source)
            {
                ViewportUnits = BrushMappingMode.Absolute,
                Viewport = new Rect(b.X, b.Y, mask.Width, mask.Height),
                Stretch = Stretch.Fill,
                TileMode = e.MaskTiled ? TileMode.Tile : TileMode.None,
            };
            brush.Freeze();
            dc.PushOpacityMask(brush);
            pushes++;
        }
        // Stacked opacity masks multiply, so this combines with the element's own clipping mask.
        if (e.HostMask is { } host)
        {
            var brush = new System.Windows.Media.ImageBrush(host.Mask.Source)
            {
                ViewportUnits = BrushMappingMode.Absolute,
                Viewport = new Rect(host.At.X, host.At.Y, host.Mask.Width, host.Mask.Height),
                Stretch = Stretch.Fill,
            };
            brush.Freeze();
            dc.PushOpacityMask(brush);
            pushes++;
        }
        if (e.ClipsChildren) { dc.PushClip(new RectangleGeometry(b)); pushes++; }
        var (behind, above) = e.ChildrenBehindAndAbove;
        e.RenderBackdrop(dc, b);
        pass?.Steps.Add((e, true));
        foreach (var c in behind) RenderElement(dc, c, pass: pass);
        if (pass?.Stopped != true)
        {
            e.Render(dc, b);
            pass?.Steps.Add((e, false));
            if (pass != null && e == pass.StopAfter) pass.Stopped = true;
        }
        foreach (var c in above) RenderElement(dc, c, pass: pass);
        for (int i = 0; i < pushes; i++) dc.Pop();
    }

    // ---- input -----------------------------------------------------------------------------

    public SkinElement? HitTestPoint(Point p)
    {
        var (behind, above) = ChildrenBehindAndAbove;
        return HitIn(above, p) ?? (HitTest(p) ? this : null) ?? HitIn(behind, p);
    }

    /// <summary>
    /// The topmost of <paramref name="children"/> (in draw order) under the point, searching into containers
    /// in the order they draw: children above, the container itself, then children behind it.
    /// </summary>
    SkinElement? HitIn(SkinElement[] children, Point p)
    {
        foreach (var c in Enumerable.Reverse(children))
        {
            if (!c.Visible) continue;
            var abs = c.Absolute;
            var local = new Point(p.X - abs.X, p.Y - abs.Y);
            if (c is ButtonGroupElement g)
            {
                if (g.HitTest(local) && g.MaskAllows(local)) return g.ElementAt(local);
                continue;
            }
            if (c.ClipsChildren && (local.X < 0 || local.Y < 0 || local.X >= c.Width || local.Y >= c.Height || !c.MaskAllows(local))) continue;
            var (behind, above) = c.ChildrenBehindAndAbove;
            var hit = HitIn(above, p);
            if (hit != null) return hit;
            // A disabled button lets clicks through: QuickSilver lays disabled buttons over its real ones as artwork.
            bool own = !c.GetBool("passthrough") && c is not ButtonElement { Enabled: false } && c.MaskAllows(local);
            if (own && c.HitTestImage(local)) return c;
            hit = HitIn(behind, p);
            if (hit != null) return hit;
            if (own && c.HitTest(local)) return c;
        }
        return null;
    }

    Point LocalTo(SkinElement el, Point p)
    {
        if (el == this) return p;
        var a = el.Absolute;
        return new Point(p.X - a.X, p.Y - a.Y);
    }

    void SetEvent(SkinElement el, Point p, int button)
    {
        var screen = Surface.PointToScreenDip(p);
        Theme.Objects.Event.SetMouse(LocalTo(el, p), p, screen, button);
    }

    public void HandleMouseMove(Point p)
    {
        if (_pressed != null)
        {
            SetEvent(_pressed, p, 1);
            bool inside = HitTestPoint(p) == _pressed;
            if (_pressed.IsHovered != inside) { _pressed.IsHovered = inside; InvalidateVisual(); }
            _pressed.OnMouseMove(LocalTo(_pressed, p), captured: true);
            return;
        }
        var hit = HitTestPoint(p);
        if (hit != _hover)
        {
            MoveHover(hit, p);
            UpdateTip();
            Surface.Cursor = _hover?.CurrentCursor;
        }
        else if (hit != null)
        {
            SetEvent(hit, p, 0);
            hit.OnMouseMove(LocalTo(hit, p), captured: false);
        }
    }

    /// <summary>
    /// onmouseout fires on the elements the pointer really left and onmouseover on the ones it entered,
    /// so a container, the view included, isn't left when the pointer moves onto one of its children.
    /// Revert fades its window on the view's onmouseout and would otherwise fade while a button is hovered.
    /// </summary>
    void MoveHover(SkinElement? hit, Point p)
    {
        var from = Ancestry(_hover);
        var to = Ancestry(hit);
        _hover = hit;
        foreach (var e in from)
            if (!to.Contains(e)) { SetEvent(e, p, 0); e.OnMouseLeave(); }
        for (int i = to.Count - 1; i >= 0; i--)
            if (!from.Contains(to[i])) { SetEvent(to[i], p, 0); to[i].OnMouseEnter(); }
    }

    static List<SkinElement> Ancestry(SkinElement? e)
    {
        var chain = new List<SkinElement>();
        for (; e != null; e = e.Parent) chain.Add(e);
        return chain;
    }

    public void HandleMouseLeave()
    {
        if (_pressed != null) return;
        foreach (var e in Ancestry(_hover)) e.OnMouseLeave();
        _hover = null;
        _tipTimer.Stop();
        _tip.IsOpen = false;
    }

    /// <summary>Returns true if the press was taken by an element (otherwise the window is dragged).</summary>
    public static bool DebugInput { get; set; }

    public bool HandleMouseDown(Point p, MouseButton button, int clicks)
    {
        _tipTimer.Stop();
        _tip.IsOpen = false;
        var hit = HitTestPoint(p);
        if (DebugInput) Log.Write($"[input] down {button} at {p} -> {hit?.ToString() ?? "(nothing)"} interactive={hit?.IsInteractive} enabled={hit?.Enabled} handlers={string.Join(",", hit?.Handlers.Keys ?? Enumerable.Empty<string>())}");
        if (hit == null) return false;
        SetEvent(hit, p, button == MouseButton.Left ? 1 : button == MouseButton.Right ? 2 : 4);
        if (clicks == 2 && hit.HasHandler("ondblclick")) { hit.OnDoubleClick(LocalTo(hit, p)); return true; }
        if (!hit.IsInteractive)
        {
            if (hit != this) hit.Fire("onmousedown");
            if (HasHandler("onmousedown") && hit != this) Fire("onmousedown");
            return false;
        }
        if (button != MouseButton.Left)
        {
            hit.Fire("onmousedown");
            return button == MouseButton.Right ? false : true;
        }
        _pressed = hit;
        _hover = hit;
        hit.IsHovered = true;
        Focus(hit);
        hit.OnMouseDown(LocalTo(hit, p), button);
        return true;
    }

    public void HandleMouseUp(Point p, MouseButton button)
    {
        var pressed = _pressed;
        _pressed = null;
        if (pressed == null) return;
        SetEvent(pressed, p, 1);
        bool inside = HitTestPoint(p) == pressed;
        pressed.OnMouseUp(LocalTo(pressed, p), button, inside);
        pressed.IsHovered = inside;
        HandleMouseMove(p);
    }

    public void HandleMouseWheel(Point p, int delta)
    {
        var hit = HitTestPoint(p) ?? _focus;
        if (hit is SliderElement or CustomSliderElement or PlaylistElement) hit.OnMouseWheel(delta);
        else if (hit == null || !hit.IsInteractive) App.Services.Player.Volume += delta > 0 ? 5 : -5;
        else hit.OnMouseWheel(delta);
    }

    public bool HandleKeyDown(Key key)
    {
        int vk = KeyInterop.VirtualKeyFromKey(key);
        Theme.Objects.Event.SetKey(vk);
        if (_focus != null)
        {
            _focus.Fire("onkeydown");
            if (_focus.OnKeyDown(key)) return true;
        }
        if (HasHandler("onkeydown")) { Fire("onkeydown"); if (!ToBool(Theme.Objects.Event.Get("returnValue"))) return true; }
        var pm = App.Services.Player;
        bool ctrl = Keyboard.Modifiers.HasFlag(ModifierKeys.Control);
        if (ctrl && key == Key.P) { pm.TogglePlayPause(); return true; }
        if (ctrl && key == Key.S) { pm.Stop(); return true; }
        if (ctrl && key == Key.B) { pm.Previous(); return true; }
        if (ctrl && key == Key.F) { pm.Next(); return true; }
        if (ctrl && key == Key.D1) { App.Modes.ShowFullMode(); return true; }
        if (ctrl && key == Key.D3) { App.Modes.CollapseToWidget(); return true; }
        if (key == Key.F8) { pm.Mute = !pm.Mute; return true; }
        if (key == Key.F9) { pm.Volume -= 5; return true; }
        if (key == Key.F10) { pm.Volume += 5; return true; }
        return false;
    }

    public void HandleKeyUp(Key key)
    {
        Theme.Objects.Event.SetKey(KeyInterop.VirtualKeyFromKey(key));
        _focus?.Fire("onkeyup");
        if (HasHandler("onkeyup")) Fire("onkeyup");
    }

    void UpdateTip()
    {
        _tip.IsOpen = false;
        _tipTimer.Stop();
        if (_hover?.CurrentToolTip != null) _tipTimer.Start();
    }

    void ShowTip()
    {
        string? text = _hover?.CurrentToolTip;
        if (text == null || Window == null) return;
        _tip.Content = text;
        _tip.PlacementTarget = Surface;
        _tip.Placement = System.Windows.Controls.Primitives.PlacementMode.Mouse;
        _tip.IsOpen = true;
    }
}
