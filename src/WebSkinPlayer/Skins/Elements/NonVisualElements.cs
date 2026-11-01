using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>Elements that exist only for scripts (no drawing, no hit testing).</summary>
public class NonVisualElement : SkinElement
{
    public NonVisualElement(SkinView view, WmsNode node) : base(view, node) { }
    public override bool HitTest(Point local) => false;
}

/// <summary>
/// EQUALIZERSETTINGS. Settings are kept and shown faithfully, but a web player's audio can't be
/// processed from outside the browser, so they don't change the sound.
/// </summary>
public sealed class EqualizerSettingsElement : NonVisualElement
{
    // WMP's stock presets: name and ten gains (-14..+14 dB).
    static readonly (string Name, double[] Gains)[] Presets =
    {
        ("Acoustic", new double[] { 3, 3, 2, 1, 0, 0, 1, 2, 3, 3 }),
        ("Blues", new double[] { 4, 3, 1, 0, -1, -1, 0, 1, 2, 3 }),
        ("Classical", new double[] { 4, 3, 2, 1, -1, -1, 0, 2, 3, 4 }),
        ("Country", new double[] { 2, 2, 1, 0, -1, 0, 1, 2, 2, 2 }),
        ("Dance", new double[] { 6, 5, 2, 0, 0, -2, -2, -2, 0, 0 }),
        ("Grunge", new double[] { 4, 2, 0, -2, -1, 0, 2, 3, 2, 1 }),
        ("Jazz", new double[] { 3, 2, 1, 2, -1, -1, 0, 1, 2, 3 }),
        ("Metal", new double[] { 5, 4, 3, 1, 0, 0, 1, 3, 4, 5 }),
        ("New Age", new double[] { 3, 3, 1, 0, 0, 0, 1, 2, 3, 3 }),
        ("Oldies", new double[] { -2, 0, 2, 3, 3, 2, 1, 0, -1, -2 }),
        ("Opera", new double[] { 0, 0, 1, 3, 4, 3, 2, 1, 0, 0 }),
        ("Rap", new double[] { 6, 5, 3, 1, -1, -1, 0, 1, 2, 3 }),
        ("Reggae", new double[] { 3, 2, 0, -2, 0, 2, 3, 3, 2, 1 }),
        ("Rock", new double[] { 5, 4, 3, 1, -1, -1, 1, 3, 4, 5 }),
        ("Speech", new double[] { -3, -2, 0, 2, 4, 4, 3, 1, 0, -2 }),
        ("Swing", new double[] { 2, 1, 0, 1, 2, 2, 1, 0, 1, 2 }),
        ("Techno", new double[] { 5, 4, 1, -1, -1, 0, 1, 3, 4, 5 }),
        ("Custom", new double[10]),
    };

    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["gainlevel1"] = PType.Number, ["gainlevel2"] = PType.Number, ["gainlevel3"] = PType.Number, ["gainlevel4"] = PType.Number,
        ["gainlevel5"] = PType.Number, ["gainlevel6"] = PType.Number, ["gainlevel7"] = PType.Number, ["gainlevel8"] = PType.Number,
        ["gainlevel9"] = PType.Number, ["gainlevel10"] = PType.Number,
        ["enabled"] = PType.Bool, ["enable"] = PType.Bool, ["bypass"] = PType.Bool, ["currentpreset"] = PType.Int,
        ["enhancedaudio"] = PType.Bool, ["trubasslevel"] = PType.Int, ["wowlevel"] = PType.Int,
        ["splinetension"] = PType.Number, ["enablesplinetension"] = PType.Bool, ["speakersize"] = PType.Int,
        ["normalization"] = PType.Bool, ["truebass"] = PType.Int,
    };
    protected override Dictionary<string, PType>? Schema => S;
    bool _applying;

    public EqualizerSettingsElement(SkinView view, WmsNode node) : base(view, node)
    {
        Computed("presetCount", () => (double)Presets.Length);
        Computed("currentPresetTitle", () => Presets[Math.Clamp(GetInt("currentpreset"), 0, Presets.Length - 1)].Name);
        Computed("currentSpeakerName", () => new[] { "Small", "Medium", "Large" }[Math.Clamp(GetInt("speakersize"), 0, 2)]);
        Method("presetTitle", a => Presets.ElementAtOrDefault((int)ToNumber(Arg(a, 0))).Name ?? "");
        Method("reset", _ => { for (int i = 1; i <= 10; i++) SetValue("gainlevel" + i, 0.0); return null; });
        Method("nextPreset", _ => { SetValue("currentpreset", (double)((GetInt("currentpreset") + 1) % Presets.Length)); return null; });
        Method("previousPreset", _ => { SetValue("currentpreset", (double)((GetInt("currentpreset") - 1 + Presets.Length) % Presets.Length)); return null; });
        Method("nextSpeaker", _ => { SetValue("speakersize", (double)((GetInt("speakersize") + 1) % 3)); return null; });
        Method("previousSpeaker", _ => { SetValue("speakersize", (double)((GetInt("speakersize") + 2) % 3)); return null; });
        Method("resetSettings", _ => null);
    }

    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "currentpreset" => (double)(Presets.Length - 1),
        "enabled" or "enable" => false,
        "splinetension" => 1.0,
        "speakersize" => 1.0,
        _ when name.StartsWith("gainlevel", StringComparison.OrdinalIgnoreCase) => 0.0,
        _ => null,
    };

    protected override void OnPropertyChanged(string name)
    {
        if (_applying) return;
        if (name == "currentpreset")
        {
            int p = Math.Clamp(GetInt("currentpreset"), 0, Presets.Length - 1);
            if (Presets[p].Name == "Custom") return;
            _applying = true;
            for (int i = 0; i < 10; i++) SetValue("gainlevel" + (i + 1), Presets[p].Gains[i]);
            _applying = false;
            Notify("currentPresetTitle");
        }
        else if (name.StartsWith("gainlevel") && GetInt("currentpreset") != Presets.Length - 1)
        {
            _applying = true;
            SetValue("currentpreset", (double)(Presets.Length - 1));
            _applying = false;
            Notify("currentPresetTitle");
        }
        else if (name == "enable") SetValue("enabled", GetBool("enable"));
    }
}

/// <summary>VIDEOSETTINGS: stored brightness/contrast/hue/saturation (no video to apply them to).</summary>
public sealed class VideoSettingsElement : NonVisualElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["brightness"] = PType.Int, ["contrast"] = PType.Int, ["hue"] = PType.Int, ["saturation"] = PType.Int,
    };
    protected override Dictionary<string, PType>? Schema => S;
    public VideoSettingsElement(SkinView view, WmsNode node) : base(view, node)
    {
        Method("reset", _ => { foreach (var k in S.Keys) SetValue(k, 0.0); return null; });
    }
}

/// <summary>POPUP: a script-built menu; show() pops it up and selectedItem reports the choice.</summary>
public sealed class PopupElement : NonVisualElement
{
    readonly List<string> _items;
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["selecteditem"] = PType.Int, ["backgroundcolor"] = PType.String, ["foregroundcolor"] = PType.String,
    };
    protected override Dictionary<string, PType>? Schema => S;

    public PopupElement(SkinView view, WmsNode node) : base(view, node)
    {
        _items = ListBoxElement.ItemLines(node);
        Method("appendItem", a => { _items.Add(ToStr(Arg(a, 0))); return (double)(_items.Count - 1); });
        Method("deleteItem", a => { int i = (int)ToNumber(Arg(a, 0)); if (i >= 0 && i < _items.Count) _items.RemoveAt(i); return null; });
        Method("deleteAll", _ => { _items.Clear(); return null; });
        Method("show", a => { Show(a.Length >= 2 ? ToNumber(a[0]) : (double?)null, a.Length >= 2 ? ToNumber(a[1]) : null); return null; });
        Method("dismiss", _ => null);
        Computed("itemCount", () => (double)_items.Count);
    }

    protected override object? ElementDefault(string name) => name.Equals("selecteditem", StringComparison.OrdinalIgnoreCase) ? -1.0 : null;

    void Show(double? x, double? y)
    {
        var menu = new ContextMenu();
        int selected = GetInt("selecteditem");
        for (int i = 0; i < _items.Count; i++)
        {
            int index = i;
            if (_items[i] == "-" || _items[i].Length == 0) { menu.Items.Add(new Separator()); continue; }
            var mi = new MenuItem { Header = _items[i].Replace("&", "_"), IsCheckable = false, IsChecked = i == selected };
            // Raise selectedItem_onchange even if the same item is chosen again.
            mi.Click += (_, _) => { SetValue("selecteditem", -2.0, notify: false); SetValue("selecteditem", (double)index); };
            menu.Items.Add(mi);
        }
        if (menu.Items.Count == 0) return;
        var p = new Point(x ?? Absolute.X, y ?? (Absolute.Y));
        menu.Placement = PlacementMode.Relative;
        menu.PlacementTarget = View.Surface;
        menu.HorizontalOffset = p.X;
        menu.VerticalOffset = p.Y;
        menu.IsOpen = true;
    }
}

/// <summary>AUTOMENU: WMP's built-in menus (Play, View, ...), mapped onto what this app can do.</summary>
public sealed class AutoMenuElement : NonVisualElement
{
    public AutoMenuElement(SkinView view, WmsNode node) : base(view, node)
    {
        Method("show", a => { View.Theme.ShowAppMenu(View, ToStr(Arg(a, 0)), Absolute); return null; });
    }
}
