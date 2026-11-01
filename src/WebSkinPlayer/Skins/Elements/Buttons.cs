using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>Built-in behaviour for the predefined buttons/elements (PLAYBUTTON, PAUSEELEMENT, ...).</summary>
/// <summary>Sizes WMP gave elements without a picture.</summary>
static class Pictureless
{
    /// <summary>A button or button group with no picture (or a missing one) was 192x192, measured in WMP.</summary>
    public const int Size = 192;
}

public static class ButtonPresets
{
    public sealed record Preset(string Action, string? Enabled, string ToolTip, bool Sticky = false, string? Down = null);

    static readonly Dictionary<string, Preset> Map = new(StringComparer.OrdinalIgnoreCase)
    {
        ["PLAY"] = new("player.controls.play();", "wmpenabled:player.controls.play", "Play"),
        ["PAUSE"] = new("player.controls.pause();", "wmpenabled:player.controls.pause", "Pause"),
        ["STOP"] = new("player.controls.stop();", "wmpenabled:player.controls.stop", "Stop"),
        ["PREV"] = new("player.controls.previous();", "wmpenabled:player.controls.previous", "Previous"),
        ["NEXT"] = new("player.controls.next();", "wmpenabled:player.controls.next", "Next"),
        ["FFWD"] = new("player.controls.fastForward();", "wmpenabled:player.controls.fastForward", "Fast Forward"),
        ["REW"] = new("player.controls.fastReverse();", "wmpenabled:player.controls.fastReverse", "Rewind"),
        ["MUTE"] = new("player.settings.mute = down;", null, "Mute", true, "wmpprop:player.settings.mute"),
        ["OPEN"] = new("theme.openDialog('FILE_OPEN','');", null, "Open"),
        ["RETURN"] = new("view.returnToMediaCenter();", null, "Return to Full Mode"),
        ["SHUFFLE"] = new("player.settings.setMode('shuffle', down);", null, "Shuffle", true, "wmpprop:player.settings.shuffle"),
        ["REPEAT"] = new("player.settings.setMode('loop', down);", null, "Repeat", true, "wmpprop:player.settings.loop"),
        ["LOOP"] = new("player.settings.setMode('loop', down);", null, "Repeat", true, "wmpprop:player.settings.loop"),
    };

    /// <summary>"PLAYBUTTON" / "PLAYELEMENT" → preset.</summary>
    public static Preset? For(string tag)
    {
        string t = tag.ToUpperInvariant();
        if (t.EndsWith("BUTTON")) t = t[..^6];
        else if (t.EndsWith("ELEMENT")) t = t[..^7];
        else return null;
        return Map.TryGetValue(t, out var p) ? p : null;
    }
}

/// <summary>BUTTON and the predefined buttons.</summary>
public class ButtonElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["image"] = PType.String, ["hoverimage"] = PType.String, ["downimage"] = PType.String,
        ["disabledimage"] = PType.String, ["hoverdownimage"] = PType.String,
        ["down"] = PType.Bool, ["sticky"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    public ButtonElement(SkinView view, WmsNode node) : base(view, node) { }

    SkinBitmap? Image => Img("image");
    public override int NaturalWidth => Image?.Width ?? Pictureless.Size;
    public override int NaturalHeight => Image?.Height ?? Pictureless.Size;
    public override bool IsInteractive => true;

    public bool Down => GetBool("down");

    SkinBitmap? CurrentImage()
    {
        if (!Enabled) return Img("disabledimage") ?? Image;
        bool pressedVisual = (IsPressed && IsHovered) || Down;
        if (pressedVisual && IsHovered) return Img("hoverdownimage") ?? Img("downimage") ?? Img("hoverimage") ?? Image;
        if (pressedVisual) return Img("downimage") ?? Image;
        if (IsHovered) return Img("hoverimage") ?? Image;
        return Image;
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        var img = CurrentImage();
        if (img == null) return;
        dc.PushClip(new RectangleGeometry(b));
        // Album art is drawn to fit the button; skin bitmaps at their own size.
        bool fit = SkinImages.IsDynamic(GetString("image")) && HasValue("width");
        dc.DrawImage(FrameOf(img, img.GetHashCode().ToString()), fit ? b : new Rect(b.X, b.Y, img.Width, img.Height));
        dc.Pop();
    }

    public override bool HitTest(Point local)
    {
        if (!base.HitTest(local)) return false;
        var img = Image;
        return img == null || img.Alpha((int)local.X, (int)local.Y) > 0;
    }

    public override string? CurrentToolTip =>
        Down ? NonEmpty(GetString("downtooltip")) ?? base.CurrentToolTip : base.CurrentToolTip;

    public override Cursor? CurrentCursor => base.CurrentCursor ?? (Enabled ? null : null);

    public override void OnClick(Point local)
    {
        if (!Enabled) return;
        if (GetBool("sticky")) SetValue("down", !Down);
        base.OnClick(local);
    }
}

/// <summary>BUTTONGROUP: one set of images; each BUTTONELEMENT owns the pixels of its mapping colour.</summary>
public sealed class ButtonGroupElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["image"] = PType.String, ["hoverimage"] = PType.String, ["downimage"] = PType.String,
        ["disabledimage"] = PType.String, ["hoverdownimage"] = PType.String, ["mappingimage"] = PType.String,
        ["showbackground"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    readonly Dictionary<string, SkinBitmap?> _masked = new();

    public ButtonGroupElement(SkinView view, WmsNode node) : base(view, node) { }

    SkinBitmap? Mapping => Img("mappingimage", keyed: false);
    public override int NaturalWidth => Mapping?.Width ?? Img("image")?.Width ?? Pictureless.Size;
    public override int NaturalHeight => Mapping?.Height ?? Img("image")?.Height ?? Pictureless.Size;
    // Without showBackground the group image only appears inside each element's mapped area.
    protected override object? ElementDefault(string name) => name.Equals("showbackground", StringComparison.OrdinalIgnoreCase) ? false : null;
    public override bool IsInteractive => true;

    protected override void OnPropertyChanged(string name)
    {
        if (name.EndsWith("image") || name is "transparencycolor" or "hueshift" or "saturation") _masked.Clear();
    }

    /// <summary>The element under a group-local point, by mapping colour.</summary>
    public GroupButtonElement? ElementAt(Point local)
    {
        var map = Mapping;
        if (map == null) return null;
        int rgb = map.Rgb((int)local.X, (int)local.Y);
        if (rgb < 0) return null;
        foreach (var c in Children)
            if (c is GroupButtonElement g && g.Visible && g.MappingColor == rgb) return g;
        return null;
    }

    public override bool HitTest(Point local) => base.HitTest(local) && ElementAt(local) != null;

    /// <summary>The state image restricted to one mapping colour.</summary>
    SkinBitmap? Masked(string attr, int color)
    {
        string key = attr + "|" + color;
        if (_masked.TryGetValue(key, out var m)) return m;
        var img = Img(attr);
        var map = Mapping;
        if (img == null || map == null) { _masked[key] = null; return null; }
        var px = (int[])img.Pixels.Clone();
        for (int y = 0; y < img.Height; y++)
            for (int x = 0; x < img.Width; x++)
                if (map.Rgb(x, y) != color) px[y * img.Width + x] = 0;
        m = new SkinBitmap(img.Width, img.Height, img.Original, px);
        _masked[key] = m;
        return m;
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        bool background = GetBool("showbackground");
        if (background) DrawBitmap(dc, Img("image"), b.X, b.Y);
        foreach (var c in Children)
        {
            if (c is not GroupButtonElement g || !g.Visible || g.MappingColor < 0) continue;
            string attr = "image";
            if (!g.Enabled) attr = Img("disabledimage") != null ? "disabledimage" : "image";
            else
            {
                bool pressed = (g.IsPressed && g.IsHovered) || g.Down;
                if (pressed && g.IsHovered) attr = Img("hoverdownimage") != null ? "hoverdownimage" : Img("downimage") != null ? "downimage" : "image";
                else if (pressed) attr = Img("downimage") != null ? "downimage" : "image";
                else if (g.IsHovered && Img("hoverimage") != null) attr = "hoverimage";
            }
            if (attr == "image" && background) continue;
            var m = Masked(attr, g.MappingColor);
            if (m != null) DrawBitmap(dc, m, b.X, b.Y);
        }
    }
}

/// <summary>BUTTONELEMENT and the predefined *ELEMENTs inside a BUTTONGROUP.</summary>
public sealed class GroupButtonElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["mappingcolor"] = PType.String, ["down"] = PType.Bool, ["sticky"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    public GroupButtonElement(SkinView view, WmsNode node) : base(view, node) { }

    public ButtonGroupElement? Group => Parent as ButtonGroupElement;
    public int MappingColor => SkinImages.ParseColor(GetString("mappingcolor")) ?? -1;
    public bool Down => GetBool("down");
    public override bool IsInteractive => true;
    public override int NaturalWidth => Group?.Width ?? 0;
    public override int NaturalHeight => Group?.Height ?? 0;

    public override string? CurrentToolTip =>
        Down ? NonEmpty(GetString("downtooltip")) ?? base.CurrentToolTip : base.CurrentToolTip;

    public override void OnClick(Point local)
    {
        if (!Enabled) return;
        if (GetBool("sticky")) SetValue("down", !Down);
        base.OnClick(local);
    }

    public override void OnMouseEnter() { base.OnMouseEnter(); Group?.Notify("hover"); }
}
