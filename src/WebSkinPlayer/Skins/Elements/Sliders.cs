using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>
/// SLIDER / SEEKSLIDER / VOLUMESLIDER / BALANCESLIDER / PROGRESSBAR.
/// The background is three-sliced by borderSize (fixed end caps, middle stretched or tiled),
/// the foreground fills up to the thumb, and the thumb's centre travels between the caps.
/// </summary>
public class SliderElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["min"] = PType.Number, ["max"] = PType.Number, ["value"] = PType.Number,
        ["direction"] = PType.String, ["backgroundimage"] = PType.String, ["foregroundimage"] = PType.String,
        ["thumbimage"] = PType.String, ["thumbhoverimage"] = PType.String, ["thumbdownimage"] = PType.String,
        ["thumbdisabledimage"] = PType.String, ["disabledimage"] = PType.String, ["backgroundhoverimage"] = PType.String,
        ["foregroundhoverimage"] = PType.String,
        ["bordersize"] = PType.Int, ["tiled"] = PType.Bool, ["slide"] = PType.Bool,
        ["foregroundprogress"] = PType.Number, ["useforegroundprogress"] = PType.Bool,
        ["keyboardincrement"] = PType.Number, ["invert"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    public bool Dragging { get; private set; }
    double _dragOffset;

    public SliderElement(SkinView view, WmsNode node) : base(view, node) { }

    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "min" => 0.0,
        "max" => 100.0,
        "value" => 0.0,
        "direction" => "horizontal",
        "slide" => true,
        "bordersize" => 0.0,
        _ => null,
    };

    public override bool IsInteractive => true;
    bool Vertical => GetString("direction").StartsWith("v", StringComparison.OrdinalIgnoreCase);
    SkinBitmap? Background => Img("backgroundimage");
    SkinBitmap? Thumb
    {
        get
        {
            if (!Enabled) return Img("thumbdisabledimage") ?? Img("thumbimage");
            if (Dragging || (IsPressed && IsHovered)) return Img("thumbdownimage") ?? Img("thumbhoverimage") ?? Img("thumbimage");
            if (IsHovered) return Img("thumbhoverimage") ?? Img("thumbimage");
            return Img("thumbimage");
        }
    }

    // Its track's size, measured in WMP; a taller thumb is clipped to it.
    public override int NaturalWidth => Background?.Width ?? Img("thumbimage")?.Width ?? 0;
    public override int NaturalHeight => Background?.Height ?? Img("thumbimage")?.Height ?? 0;

    double Min => GetNumber("min");
    double Max => GetNumber("max");
    double Length => Vertical ? Height : Width;
    int Border => Math.Max(0, GetInt("bordersize"));

    double Ratio(double value)
    {
        double range = Max - Min;
        return range == 0 ? 0 : Math.Clamp((value - Min) / range, 0, 1);
    }

    /// <summary>Thumb centre along the track for a value (pixels from the start).</summary>
    double CenterFor(double value)
    {
        double b = Border, len = Length;
        double r = Ratio(value);
        if (Vertical) r = 1 - r; // min at the bottom
        return b + r * Math.Max(0, len - 2 * b);
    }

    double ValueAt(double pos)
    {
        double b = Border, len = Length;
        double r = len - 2 * b <= 0 ? 0 : Math.Clamp((pos - b) / (len - 2 * b), 0, 1);
        if (Vertical) r = 1 - r;
        return Min + r * (Max - Min);
    }

    Rect ThumbRect(Rect bounds)
    {
        var t = Thumb;
        if (t == null) return Rect.Empty;
        double c = CenterFor(GetNumber("value"));
        if (Vertical)
        {
            double y = Math.Clamp(c - t.Height / 2.0, 0, Math.Max(0, Height - t.Height));
            return new Rect(bounds.X + Math.Max(0, (Width - t.Width) / 2.0), bounds.Y + y, t.Width, t.Height);
        }
        double x = Math.Clamp(c - t.Width / 2.0, 0, Math.Max(0, Width - t.Width));
        // Centred across the track but never above its top: a taller thumb hangs from it (anime, Crystalball).
        return new Rect(bounds.X + x, bounds.Y + Math.Max(0, (Height - t.Height) / 2.0), t.Width, t.Height);
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        dc.PushClip(new RectangleGeometry(b));
        var bg = (!Enabled ? Img("disabledimage") : null) ?? (IsHovered ? Img("backgroundhoverimage") : null) ?? Background;
        if (bg != null) DrawSliced(dc, bg, b, Border, GetBool("tiled"));

        var fg = (IsHovered ? Img("foregroundhoverimage") : null) ?? Img("foregroundimage");
        if (fg != null)
        {
            double v = GetBool("useforegroundprogress") ? GetNumber("foregroundprogress") : GetNumber("value");
            double end = CenterFor(v);
            Rect fill = Vertical ? new Rect(b.X, b.Y + end, b.Width, Math.Max(0, b.Height - end)) : new Rect(b.X, b.Y, Math.Max(0, end), b.Height);
            if (fill.Width > 0 && fill.Height > 0)
            {
                dc.PushClip(new RectangleGeometry(fill));
                if (GetBool("tiled") || (Vertical ? fg.Height < b.Height / 2 : fg.Width < b.Width / 2))
                    DrawSliced(dc, fg, b, Border, tiled: true);
                else
                    DrawSliced(dc, fg, b, Border, tiled: false);
                dc.Pop();
            }
        }

        var thumb = Thumb;
        if (thumb != null) dc.DrawImage(thumb.Source, ThumbRect(b));
        dc.Pop();
    }

    /// <summary>Draws an image across bounds keeping borderSize end caps and filling the middle.</summary>
    void DrawSliced(DrawingContext dc, SkinBitmap img, Rect b, int border, bool tiled)
    {
        double len = Vertical ? b.Height : b.Width;
        int imgLen = Vertical ? img.Height : img.Width;
        double cross = Vertical ? img.Width : img.Height;
        double crossOff = Vertical ? (b.Width - img.Width) / 2.0 : (b.Height - img.Height) / 2.0;
        if (crossOff < 0) crossOff = 0;

        if (Math.Abs(imgLen - len) < 0.5 || (border == 0 && !tiled && imgLen >= len))
        {
            Put(dc, img, 0, imgLen, 0, imgLen, b, crossOff, cross);
            return;
        }
        int cap = Math.Min(border, imgLen / 2);
        int mid = imgLen - 2 * cap;
        if (cap > 0)
        {
            Put(dc, img, 0, cap, 0, cap, b, crossOff, cross);
            Put(dc, img, imgLen - cap, cap, len - cap, cap, b, crossOff, cross);
        }
        double midLen = len - 2 * cap;
        if (mid <= 0 || midLen <= 0) return;
        if (tiled)
        {
            for (double p = 0; p < midLen; p += mid)
                Put(dc, img, cap, (int)Math.Min(mid, Math.Ceiling(midLen - p)), cap + p, Math.Min(mid, midLen - p), b, crossOff, cross);
        }
        else Put(dc, img, cap, mid, cap, midLen, b, crossOff, cross);
    }

    readonly Dictionary<(BitmapSource, int, int), BitmapSource> _crops = new();

    void Put(DrawingContext dc, SkinBitmap img, int srcStart, int srcLen, double dstStart, double dstLen, Rect b, double crossOff, double cross)
    {
        if (srcLen <= 0 || dstLen <= 0) return;
        var key = (img.Source, srcStart, srcLen);
        if (!_crops.TryGetValue(key, out var crop))
        {
            var rect = Vertical ? new Int32Rect(0, srcStart, img.Width, Math.Min(srcLen, img.Height - srcStart)) : new Int32Rect(srcStart, 0, Math.Min(srcLen, img.Width - srcStart), img.Height);
            if (rect.Width <= 0 || rect.Height <= 0) return;
            crop = new CroppedBitmap(img.Source, rect);
            crop.Freeze();
            if (_crops.Count > 64) _crops.Clear();
            _crops[key] = crop;
        }
        var dst = Vertical ? new Rect(b.X + crossOff, b.Y + dstStart, cross, dstLen) : new Rect(b.X + dstStart, b.Y + crossOff, dstLen, cross);
        dc.DrawImage(crop, dst);
    }

    public override void OnMouseDown(Point local, MouseButton button)
    {
        base.OnMouseDown(local, button);
        if (button != MouseButton.Left || !Enabled) return;
        var thumb = ThumbRect(new Rect(0, 0, Width, Height));
        double pos = Vertical ? local.Y : local.X;
        if (!thumb.IsEmpty && thumb.Contains(local))
            _dragOffset = pos - (Vertical ? thumb.Top + thumb.Height / 2 : thumb.Left + thumb.Width / 2);
        else
        {
            _dragOffset = 0;
            if (GetBool("slide"))
                SetValue("value", ValueAt(pos));
            else
            {
                // Page toward the click.
                double cur = GetNumber("value"), target = ValueAt(pos), step = (Max - Min) / 10;
                SetValue("value", target > cur ? Math.Min(target, cur + step) : Math.Max(target, cur - step));
            }
        }
        Dragging = true;
        Fire("ondragbegin");
    }

    public override void OnMouseMove(Point local, bool captured)
    {
        base.OnMouseMove(local, captured);
        if (!Dragging || !captured) return;
        double pos = (Vertical ? local.Y : local.X) - _dragOffset;
        SetValue("value", ValueAt(pos));
    }

    public override void OnMouseUp(Point local, MouseButton button, bool inside)
    {
        bool wasDragging = Dragging;
        Dragging = false;
        base.OnMouseUp(local, button, inside);
        if (wasDragging) Fire("ondragend");
    }

    public override void OnMouseWheel(int delta)
    {
        if (!Enabled) return;
        double step = GetNumber("keyboardincrement");
        if (step <= 0) step = (Max - Min) / 20;
        SetValue("value", Math.Clamp(GetNumber("value") + Math.Sign(delta) * step * (Vertical ? 1 : 1), Math.Min(Min, Max), Math.Max(Min, Max)));
        Fire("ondragend");
    }

    public override bool OnKeyDown(Key key)
    {
        double step = GetNumber("keyboardincrement");
        if (step <= 0) step = (Max - Min) / 20;
        double v = GetNumber("value");
        switch (key)
        {
            case Key.Left: case Key.Down: SetValue("value", Math.Max(Min, v - step)); return true;
            case Key.Right: case Key.Up: SetValue("value", Math.Min(Max, v + step)); return true;
            case Key.Home: SetValue("value", Min); return true;
            case Key.End: SetValue("value", Max); return true;
        }
        return false;
    }
}

/// <summary>
/// CUSTOMSLIDER: <c>image</c> is a strip of frames (one per state, frame width = element width);
/// <c>positionImage</c> is a greyscale map where a pixel's grey level gives the value under the mouse.
/// </summary>
public sealed class CustomSliderElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["min"] = PType.Number, ["max"] = PType.Number, ["value"] = PType.Number,
        ["image"] = PType.String, ["positionimage"] = PType.String,
    };
    protected override Dictionary<string, PType>? Schema => S;
    readonly Dictionary<int, BitmapSource> _frames = new();
    bool _dragging;

    public CustomSliderElement(SkinView view, WmsNode node) : base(view, node) { }

    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "min" => 0.0, "max" => 100.0, "value" => 0.0, _ => null,
    };

    SkinBitmap? Strip => Img("image");
    SkinBitmap? Map => Img("positionimage", keyed: false);
    public override int NaturalWidth => Map?.Width ?? Strip?.Width ?? 0;
    public override int NaturalHeight => Map?.Height ?? Strip?.Height ?? 0;
    public override bool IsInteractive => true;

    protected override void OnPropertyChanged(string name)
    {
        if (name is "image" or "transparencycolor" or "width" or "height" or "hueshift" or "saturation") _frames.Clear();
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        var strip = Strip;
        if (strip == null) return;
        int fw = Math.Max(1, Width > 0 ? Width : strip.Width);
        int fh = Math.Max(1, Height > 0 ? Height : strip.Height);
        // Frames run left to right, or top to bottom when the strip is one frame wide
        // (Windows XP Media Center Edition's seek bar).
        bool vertical = strip.Width / fw <= 1 && strip.Height / fh > 1;
        int len = vertical ? strip.Height : strip.Width, step = vertical ? fh : fw;
        int frames = Math.Max(1, len / step);
        double range = GetNumber("max") - GetNumber("min");
        double r = range == 0 ? 0 : Math.Clamp((GetNumber("value") - GetNumber("min")) / range, 0, 1);
        int f = (int)Math.Round(r * (frames - 1));
        if (!_frames.TryGetValue(f, out var crop))
        {
            int at = Math.Min(f * step, Math.Max(0, len - step));
            int size = Math.Min(step, len - at);
            crop = new CroppedBitmap(strip.Source, vertical
                ? new Int32Rect(0, at, strip.Width, size)
                : new Int32Rect(at, 0, size, strip.Height));
            crop.Freeze();
            _frames[f] = crop;
        }
        dc.DrawImage(crop, new Rect(b.X, b.Y, crop.PixelWidth, crop.PixelHeight));
    }

    bool OnMap(Point local, out double value)
    {
        value = 0;
        var map = Map;
        if (map == null) return false;
        int rgb = map.Rgb((int)local.X, (int)local.Y);
        if (rgb < 0) return false;
        int? key = SkinImages.ParseColor(GetString("transparencycolor"));
        if (key is int k && SkinImages.MatchesKey(rgb, k)) return false;
        int gray = ((rgb >> 16) & 255) * 30 / 100 + ((rgb >> 8) & 255) * 59 / 100 + (rgb & 255) * 11 / 100;
        value = GetNumber("min") + gray / 255.0 * (GetNumber("max") - GetNumber("min"));
        return true;
    }

    public override bool HitTest(Point local) => base.HitTest(local) && OnMap(local, out _);

    public override void OnMouseDown(Point local, MouseButton button)
    {
        base.OnMouseDown(local, button);
        if (button != MouseButton.Left || !Enabled) return;
        _dragging = true;
        Fire("ondragbegin");
        if (OnMap(local, out double v)) SetValue("value", v);
    }

    public override void OnMouseMove(Point local, bool captured)
    {
        base.OnMouseMove(local, captured);
        if (_dragging && captured && OnMap(local, out double v)) SetValue("value", v);
    }

    public override void OnMouseUp(Point local, MouseButton button, bool inside)
    {
        bool was = _dragging;
        _dragging = false;
        base.OnMouseUp(local, button, inside);
        if (was) Fire("ondragend");
    }

    public override void OnMouseWheel(int delta)
    {
        double step = (GetNumber("max") - GetNumber("min")) / 20;
        SetValue("value", Math.Clamp(GetNumber("value") + Math.Sign(delta) * step, GetNumber("min"), GetNumber("max")));
    }
}
