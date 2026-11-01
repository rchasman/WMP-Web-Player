using System.Windows;
using System.Windows.Media;
using WebSkinPlayer.Audio;
using WebSkinPlayer.Skins.Scripting;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>EFFECTS / WMPEFFECTS: hosts a visualization driven by system audio.</summary>
public class EffectsElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["effecttype"] = PType.String, ["preset"] = PType.Int, ["currenteffecttype"] = PType.String,
        ["currentpreset"] = PType.Int, ["windowed"] = PType.Bool, ["allowall"] = PType.Bool,
        ["fullscreen"] = PType.Bool, ["backgroundcolor"] = PType.String,
    };
    protected override Dictionary<string, PType>? Schema => S;

    protected readonly VisualizerRenderer Renderer = new() { MaxPixels = 640 * 480 };
    bool _subscribed;
    bool _applying;

    public EffectsElement(SkinView view, WmsNode node) : base(view, node)
    {
        Computed("currentEffectTitle", () => Renderer.Descriptor?.Title ?? "");
        Computed("currentPresetTitle", () => Renderer.Presets.Count > Renderer.Preset ? Renderer.Presets[Renderer.Preset] : "");
        Computed("currentEffect", () => EffectKey(Renderer.Descriptor));
        Computed("effectCount", () => (double)VisualizationCatalog.All.Count);
        Computed("presetCount", () => (double)Renderer.Presets.Count);
        Computed("effectCanGoFullScreen", () => false);
        Computed("effectHasPropertyPage", () => false);
        Method("next", _ => { Step(+1); return null; });
        Method("previous", _ => { Step(-1); return null; });
        Method("nextPreset", _ => { StepPreset(+1); return null; });
        Method("previousPreset", _ => { StepPreset(-1); return null; });
        Method("nextEffect", _ => { StepEffect(+1); return null; });
        Method("previousEffect", _ => { StepEffect(-1); return null; });
        Method("settings", _ => { App.Modes.ShowFullMode(); return null; });
        Method("showPropertyPage", _ => null);
        Method("getEffectTitle", a => VisualizationCatalog.All.ElementAtOrDefault(GetIntArg(a))?.Title ?? "");
        Method("getEffectType", a => EffectKey(VisualizationCatalog.All.ElementAtOrDefault(GetIntArg(a))));
        Method("getPresetTitle", a => Renderer.Presets.ElementAtOrDefault(GetIntArg(a)) ?? "");
        Renderer.BitmapChanged += View.InvalidateVisual;
    }

    static int GetIntArg(object?[] a) => (int)ToNumber(Arg(a, 0));

    /// <summary>What skins see in effectType: the plug-in's class id when it has one.</summary>
    public static string EffectKey(VisDescriptor? d) => d == null ? "" : d.ClassId ?? d.Id;

    public override bool IsInteractive => HasHandler("onclick") || HasHandler("ondblclick") || HasHandler("onmousedown");

    /// <summary>
    /// WMP hosted EFFECTS and VIDEO in a window cut to their parent subview's shape. With no clipping
    /// of its own, that shape is the transparencyColor outline (Plus! Bionic Dot's dome); other
    /// children of the subview still paint over those pixels.
    /// </summary>
    public override (SkinBitmap Mask, Point At)? HostMask
    {
        get
        {
            if (Parent is not SubviewElement host || host is SkinView || host.MaskTiled || host.ClipMask != null) return null;
            if (SkinImages.ParseColor(host.GetString("transparencycolor")) is not int key) return null;
            var shape = View.Theme.Images.GetRaw(host.GetString("backgroundimage"))?.ShapeOf(key);
            return shape == null ? null : (shape, host.Absolute);
        }
    }

    public override void OnLoaded()
    {
        var svc = App.Services.Visualizations;
        // Unbound effects follow the global visualization choice, and so does every effect once the skin has a
        // remembered one (the visualization last shown in it outranks the one it names).
        bool remembered = App.Modes?.RestoredSkin == View.Theme.Key;
        string type = remembered ? "" : GetString("currenteffecttype");
        if (type.Length == 0 && !remembered) type = GetString("effecttype");
        int preset = remembered ? svc.Preset : HasValue("currentpreset") ? GetInt("currentpreset") : HasValue("preset") ? GetInt("preset") : svc.Preset;
        var d = VisualizationCatalog.Find(type) ?? svc.Current;
        bool isAlias = type.Length > 0 && !type.Equals(EffectKey(d), StringComparison.OrdinalIgnoreCase) && !type.Equals(d.Id, StringComparison.OrdinalIgnoreCase);
        Apply(d, preset, notify: false, alias: isAlias && VisualizationCatalog.Find(type) != null ? type : null);
        UpdateSubscription();
        svc.Chosen += OnChosen;
    }

    /// <summary>A visualization picked from the player's own lists (the skin's right-click menu) shows here, as in WMP.</summary>
    void OnChosen()
    {
        var svc = App.Services.Visualizations;
        Apply(svc.Current, svc.Preset, notify: true);
    }

    public override void OnUnloaded()
    {
        App.Services.Visualizations.Chosen -= OnChosen;
        if (_subscribed) App.Services.Visualizations.Unsubscribe(OnFrame);
        _subscribed = false;
        Renderer.Dispose();
    }

    /// <summary>The effect name the skin itself set (e.g. "Spikes"), reported back while it maps to the shown effect.</summary>
    string? _alias;

    void Apply(VisDescriptor d, int preset, bool notify, string? alias = null)
    {
        _applying = true;
        if (alias != null) _alias = alias;
        else if (_alias != null && VisualizationCatalog.Find(_alias)?.Id != d.Id) _alias = null;
        Renderer.SetEffect(d, preset);
        SetValue("currenteffecttype", _alias ?? EffectKey(d), notify);
        SetValue("currentpreset", (double)Renderer.Preset, notify);
        _applying = false;
        if (notify)
        {
            Notify("currentEffectTitle");
            Notify("currentPresetTitle");
            App.Services.Visualizations.SetEffect(d.Id, Renderer.Preset);
        }
        View.InvalidateVisual();
    }

    /// <summary>Frames drawn since the visualizer last came into view.</summary>
    int _framesShown;
    bool? _keepRemembered;

    /// <summary>
    /// A skin switching its visualizer on often names one as it does (Kenwood sets "Spikes" each time it changes
    /// from its song display to the visualizer). Once the skin has a visualization remembered, that comes back
    /// instead; one chosen while the visualizer is already showing (a skin's own visualization menu) still applies.
    /// Decided once per script run, so a skin naming an effect and then its preset counts as one choice.
    /// </summary>
    bool KeepsRemembered
    {
        get
        {
            if (IsEffectivelyVisible && _framesShown > 0) return false;
            if (_keepRemembered == null)
            {
                _keepRemembered = App.Modes?.Remembers(View.Theme) == true;
                System.Windows.Threading.Dispatcher.CurrentDispatcher.BeginInvoke(System.Windows.Threading.DispatcherPriority.Background, () => _keepRemembered = null);
            }
            return _keepRemembered.Value;
        }
    }

    protected override void OnPropertyChanged(string name)
    {
        if (name == "visible") UpdateSubscription();
        if (_applying) return;
        if (name is "currenteffecttype" or "effecttype" or "currentpreset" or "preset" && KeepsRemembered)
        {
            var svc = App.Services.Visualizations;
            Apply(svc.Current, svc.Preset, notify: true);
            return;
        }
        if (name is "currenteffecttype" or "effecttype")
        {
            string requested = GetString(name);
            var d = VisualizationCatalog.Find(requested);
            bool isAlias = d != null && !requested.Equals(EffectKey(d), StringComparison.OrdinalIgnoreCase) && !requested.Equals(d.Id, StringComparison.OrdinalIgnoreCase);
            // Skins bound to mediacenter.effectType (Compact) hear of a new visualization before its preset: when it's the
            // player's own choice, its preset comes with it.
            var svc = App.Services.Visualizations;
            int preset = d?.Id == Renderer.EffectId ? Renderer.Preset : d?.Id == svc.EffectId ? svc.Preset : 0;
            if (d != null) Apply(d, preset, notify: true, alias: isAlias ? requested : null);
        }
        else if (name is "currentpreset" or "preset")
        {
            int p = GetInt(name);
            if (Renderer.Descriptor != null && p != Renderer.Preset) Apply(Renderer.Descriptor, p, notify: true);
        }
    }

    void StepPreset(int dir)
    {
        if (Renderer.Descriptor == null) return;
        int n = Math.Max(1, Renderer.Presets.Count);
        Apply(Renderer.Descriptor, ((Renderer.Preset + dir) % n + n) % n, notify: true);
    }

    void StepEffect(int dir)
    {
        var all = VisualizationCatalog.All;
        int i = all.ToList().FindIndex(d => d.Id == Renderer.EffectId);
        var next = all[((i + dir) % all.Count + all.Count) % all.Count];
        Apply(next, dir > 0 ? 0 : next.Presets.Count - 1, notify: true);
    }

    /// <summary>next()/previous(): walk presets, rolling over into the neighbouring visualization.</summary>
    void Step(int dir)
    {
        int n = Renderer.Presets.Count;
        int p = Renderer.Preset + dir;
        if (p >= 0 && p < n) StepPreset(dir);
        else StepEffect(dir);
    }

    void UpdateSubscription()
    {
        bool want = IsEffectivelyVisible && View.IsOpen;
        if (want && !_subscribed) { App.Services.Visualizations.Subscribe(OnFrame); _subscribed = true; _framesShown = 0; }
        else if (!want && _subscribed) { App.Services.Visualizations.Unsubscribe(OnFrame); _subscribed = false; _framesShown = 0; }
    }

    public void RefreshSubscription() => UpdateSubscription();

    void OnFrame(VisFrame frame)
    {
        if (!IsEffectivelyVisible) { UpdateSubscription(); return; }
        if (_framesShown < int.MaxValue) _framesShown++;
        var dpi = VisualTreeHelper.GetDpi(View.Surface);
        Renderer.Resize((int)(Math.Max(1, Width) * dpi.DpiScaleX), (int)(Math.Max(1, Height) * dpi.DpiScaleY));
        Renderer.Render(frame, App.Services.Visualizations.Context);
    }

    /// <summary>Test hook: paint visualizer areas a unique colour so their visible share can be measured.</summary>
    public static bool MarkerMode { get; set; }
    public static readonly Color MarkerColor = Color.FromRgb(0x12, 0xFE, 0x34);

    public override void Render(DrawingContext dc, Rect b)
    {
        if (b.Width < 1 || b.Height < 1) return;
        if (MarkerMode) { dc.DrawRectangle(new SolidColorBrush(MarkerColor), null, b); return; }
        if (Renderer.Bitmap != null)
            dc.DrawImage(Renderer.Bitmap, b);
        else
            dc.DrawRectangle(Brushes.Black, null, b);
    }
}

/// <summary>
/// VIDEO / WMPVIDEO. There is never video from a web player, so this shows the skin's background
/// colour; if the skin has no EFFECTS element at all it shows the visualization instead.
/// </summary>
public sealed class VideoElement : EffectsElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["backgroundcolor"] = PType.String, ["stretchtofit"] = PType.Bool, ["zoom"] = PType.Int,
        ["windowless"] = PType.Bool, ["maintainaspectratio"] = PType.Bool, ["fullscreen"] = PType.Bool,
        ["cursor"] = PType.String, ["shrinktofit"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;
    bool _showVis;
    public bool ShowsVisualization => _showVis;

    public VideoElement(SkinView view, WmsNode node) : base(view, node) { }

    protected override object? ElementDefault(string name) => name.Equals("backgroundcolor", StringComparison.OrdinalIgnoreCase) ? "#000000" : null;

    public override void OnLoaded()
    {
        _showVis = !View.Theme.HasEffects;
        if (_showVis) base.OnLoaded();
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        int? c = SkinImages.ParseColor(GetString("backgroundcolor"));
        if (c != null && !(MarkerMode && _showVis)) dc.DrawRectangle(new SolidColorBrush(Color.FromRgb((byte)(c >> 16), (byte)(c >> 8), (byte)c)), null, b);
        if (_showVis) base.Render(dc, b);
    }
}
