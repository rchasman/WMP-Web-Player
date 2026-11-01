using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>Base for every visual skin element (and VIEW itself).</summary>
public class SkinElement : ScriptObject
{
    static readonly Dictionary<string, PType> CommonSchema = new(StringComparer.OrdinalIgnoreCase)
    {
        ["id"] = PType.String, ["left"] = PType.Int, ["top"] = PType.Int, ["width"] = PType.Int, ["height"] = PType.Int,
        ["visible"] = PType.Bool, ["enabled"] = PType.Bool, ["zindex"] = PType.Int, ["alphablend"] = PType.Int,
        ["horizontalalignment"] = PType.String, ["verticalalignment"] = PType.String,
        ["tooltip"] = PType.String, ["uptooltip"] = PType.String, ["downtooltip"] = PType.String, ["cursor"] = PType.String,
        ["passthrough"] = PType.Bool, ["tabstop"] = PType.Bool, ["showfocus"] = PType.Bool,
        ["clippingcolor"] = PType.String, ["clippingimage"] = PType.String, ["transparencycolor"] = PType.String,
        ["hueshift"] = PType.Number, ["saturation"] = PType.Number,
        ["accname"] = PType.String, ["accdescription"] = PType.String, ["acckeyboardshortcut"] = PType.String,
        ["elementtype"] = PType.String,
    };

    public SkinView View { get; }
    public WmsNode Node { get; }
    public string Tag { get; }
    public SkinElement? Parent { get; internal set; }
    public List<SkinElement> Children { get; } = new();
    /// <summary>Lower-case event name ("onclick", "value_onchange") → compiled handler.</summary>
    public Dictionary<string, CompiledHandler> Handlers { get; } = new(StringComparer.OrdinalIgnoreCase);
    /// <summary>Document order, used to break zIndex ties.</summary>
    public int Order { get; internal set; }

    public bool IsHovered { get; internal set; }
    public bool IsPressed { get; internal set; }

    /// <param name="view">Owning view; null only for the view itself.</param>
    public SkinElement(SkinView? view, WmsNode node)
    {
        View = view ?? (SkinView)this;
        Node = node;
        Tag = node.Tag;
        Method("moveTo", a => { MoveTo(ToNumber(Arg(a, 0)), ToNumber(Arg(a, 1)), ToNumber(Arg(a, 2))); return null; });
        Method("alphaBlendTo", a => { AlphaBlendTo(ToNumber(Arg(a, 0)), ToNumber(Arg(a, 1))); return null; });
        Method("resizeTo", a => { ResizeTo(ToNumber(Arg(a, 0)), ToNumber(Arg(a, 1)), ToNumber(Arg(a, 2))); return null; });
        Method("setFocus", _ => { View.Focus(this); return null; });
        Method("removeElement", _ => null);
    }

    protected static object? Arg(object?[] a, int i) => i < a.Length ? a[i] : null;

    public override string ClassName => Tag;
    public string? Id => Values.TryGetValue("id", out var v) ? v as string : null;

    // ---- schema --------------------------------------------------------------------------

    protected virtual Dictionary<string, PType>? Schema => null;

    protected override PType TypeOf(string name) =>
        Schema != null && Schema.TryGetValue(name, out var t) ? t : CommonSchema.TryGetValue(name, out t) ? t : PType.Any;

    protected override bool IsDeclared(string name) => CommonSchema.ContainsKey(name) || (Schema?.ContainsKey(name) ?? false);

    bool _measuring;

    /// <summary>Natural size with a re-entrancy guard (a size that depends on itself is 0, not a stack overflow).</summary>
    double Natural(bool width)
    {
        if (_measuring) return 0;
        _measuring = true;
        try { return width ? NaturalWidth : NaturalHeight; }
        catch (Exception ex) { Log.Write($"{this} size: {ex.Message}"); return 0; }
        finally { _measuring = false; }
    }

    protected override object? DefaultValue(string name) => name.ToLowerInvariant() switch
    {
        "visible" or "enabled" => true,
        "alphablend" => 255.0,
        "saturation" => 1.0,
        "left" or "top" or "zindex" or "hueshift" => 0.0,
        "width" => Natural(true),
        "height" => Natural(false),
        "horizontalalignment" => "left",
        "verticalalignment" => "top",
        "tabstop" => false,
        _ => ElementDefault(name),
    };

    protected virtual object? ElementDefault(string name) => null;

    /// <summary>Size used when the skin doesn't give width/height (usually the image size).</summary>
    public virtual int NaturalWidth => 0;
    public virtual int NaturalHeight => 0;

    // ---- geometry ------------------------------------------------------------------------

    public int Left => GetInt("left");
    public int Top => GetInt("top");
    public int Width => GetInt("width");
    public int Height => GetInt("height");
    public bool Visible => GetBool("visible");
    public bool Enabled => GetBool("enabled") && (Parent?.Enabled ?? true);
    public int ZIndex => GetInt("zindex");
    public double Opacity => Math.Clamp(GetNumber("alphablend"), 0, 255) / 255.0;

    /// <summary>Position in view coordinates.</summary>
    public Point Absolute
    {
        get
        {
            double x = 0, y = 0;
            for (var e = this; e != null && e is not SkinView; e = e.Parent) { x += e.Left; y += e.Top; }
            return new Point(x, y);
        }
    }

    public Rect AbsoluteBounds { get { var p = Absolute; return new Rect(p.X, p.Y, Math.Max(0, Width), Math.Max(0, Height)); } }

    public bool IsEffectivelyVisible
    {
        get
        {
            for (var e = this; e != null; e = e.Parent) if (!e.Visible) return false;
            return true;
        }
    }

    /// <summary>
    /// Children drawn before the container's background image (negative zIndex in a SUBVIEW, seen
    /// through its transparent holes as in Goo and Creed), then those drawn after. A VIEW keeps
    /// all its children on top: its clippingColor pixels are keyed out of the window whatever is
    /// drawn behind them (YIL!OMA2K and Radio keep their screens visible so). A button group draws
    /// its buttons itself.
    /// </summary>
    public (SkinElement[] Behind, SkinElement[] Above) ChildrenBehindAndAbove
    {
        get
        {
            if (this is ButtonGroupElement) return ([], []);
            var all = Children.OrderBy(c => c.ZIndex).ThenBy(c => c.Order).ToArray();
            if (!ClipsChildren || this is SkinView) return ([], all);
            return (all.Where(c => c.ZIndex < 0).ToArray(), all.Where(c => c.ZIndex >= 0).ToArray());
        }
    }

    // Track size so alignment can be applied on parent resize.
    int _lastW = -1, _lastH = -1;

    protected override void OnValueChanged(string name)
    {
        string n = name.ToLowerInvariant();
        if (n is "width" or "height") ApplyResizeToChildren();
        if (n == "visible" && Visible) RestartAnimations();
        OnPropertyChanged(n);
        View.InvalidateVisual();
    }

    protected virtual void OnPropertyChanged(string name) { }

    /// <summary>Records the current size as the baseline for alignment (after load).</summary>
    public void CaptureLayoutBaseline()
    {
        _lastW = Width;
        _lastH = Height;
        foreach (var c in Children) c.CaptureLayoutBaseline();
    }

    void ApplyResizeToChildren()
    {
        if (_lastW < 0) return; // still loading
        int w = Width, h = Height;
        int dw = w - _lastW, dh = h - _lastH;
        _lastW = w; _lastH = h;
        if (dw == 0 && dh == 0) return;
        foreach (var c in Children.ToArray()) c.AlignTo(dw, dh);
    }

    /// <summary>WMP alignment: left/top stay put, right/bottom follow the far edge, center keeps centred, stretch grows.</summary>
    void AlignTo(int dw, int dh)
    {
        if (dw != 0)
            switch (GetString("horizontalalignment").ToLowerInvariant())
            {
                case "right": SetValue("left", Left + dw); break;
                case "center": SetValue("left", Left + dw / 2.0); break;
                case "stretch": SetValue("width", Math.Max(0, Width + dw)); break;
            }
        if (dh != 0)
            switch (GetString("verticalalignment").ToLowerInvariant())
            {
                case "bottom": SetValue("top", Top + dh); break;
                case "center": SetValue("top", Top + dh / 2.0); break;
                case "stretch": SetValue("height", Math.Max(0, Height + dh)); break;
            }
    }

    // ---- animation -----------------------------------------------------------------------

    public void MoveTo(double x, double y, double ms)
    {
        if (ms <= 0) { SetValue("left", x); SetValue("top", y); Fire("onendmove"); return; }
        double x0 = Left, y0 = Top;
        View.Animate(this, "move", ms, t => { SetValue("left", x0 + (x - x0) * t); SetValue("top", y0 + (y - y0) * t); }, () => Fire("onendmove"));
    }

    public void AlphaBlendTo(double alpha, double ms)
    {
        if (ms <= 0) { SetValue("alphablend", alpha); Fire("onendalphablend"); return; }
        double a0 = GetNumber("alphablend");
        View.Animate(this, "alpha", ms, t => SetValue("alphablend", a0 + (alpha - a0) * t), () => Fire("onendalphablend"));
    }

    public void ResizeTo(double w, double h, double ms)
    {
        if (ms <= 0) { SetValue("width", w); SetValue("height", h); Fire("onendresize"); return; }
        double w0 = Width, h0 = Height;
        View.Animate(this, "resize", ms, t => { SetValue("width", w0 + (w - w0) * t); SetValue("height", h0 + (h - h0) * t); }, () => Fire("onendresize"));
    }

    // ---- events --------------------------------------------------------------------------

    public bool HasHandler(string name) => Handlers.ContainsKey(name);

    public object? Fire(string name, params object?[] args)
    {
        if (Handlers.TryGetValue(name, out var h)) return View.Bridge.Invoke(h, this, args);
        // A script may have assigned a function to e.g. element.onclick.
        if (Values.TryGetValue(name, out var v) && v is Jint.Native.JsValue fn && fn is Jint.Native.Function.Function)
            return View.Bridge.CallFunction(fn, this, args);
        return null;
    }

    /// <summary>
    /// Whether clicks pass through to what lies beneath: it has passThrough, or the nearest subview or view
    /// holding it does. Measured in WMP with synthetic clicks: a subview's passThrough reaches its buttons,
    /// text and visualizers (even one set passThrough="false") but not the subviews inside it, which start
    /// afresh. QuickSilver switches its sliders off this way; Plus! Bionic Dot's EQ lies under such a visualizer.
    /// </summary>
    public bool PassesClicks => GetBool("passthrough") || (this is not SubviewElement && Container?.GetBool("passthrough") == true);

    SkinElement? Container
    {
        get
        {
            for (var p = Parent; p != null; p = p.Parent) if (p is SubviewElement) return p;
            return null;
        }
    }

    static readonly string[] MouseHandlers = { "onclick", "onmousedown", "onmouseup", "ondblclick", "onmouseover", "onmouseout", "onmousemove" };

    /// <summary>
    /// Whether the element does something with the mouse: a tooltip, or a handler in its attributes or
    /// assigned by a script. Parts that draw nothing to click take clicks only then.
    /// </summary>
    public bool UsesMouse => CurrentToolTip != null || MouseHandlers.Any(h =>
        HasHandler(h) || (Values.TryGetValue(h, out var v) && v is Jint.Native.Function.Function));

    static readonly HashSet<string> UndrawnAttributes = new(StringComparer.OrdinalIgnoreCase)
    {
        "transparencycolor", "clippingcolor", "clippingimage", "mappingimage", "positionimage",
    };

    /// <summary>
    /// Whether the element may paint <paramref name="rgb"/>: one of its colours is that colour, or a picture it
    /// draws has an opaque pixel of it (keyed as drawn).
    /// </summary>
    public bool MayPaint(int rgb) => Values.Any(kv => kv.Value is string v && v.Length > 0 && !UndrawnAttributes.Contains(kv.Key) && (
        (kv.Key.EndsWith("color", StringComparison.OrdinalIgnoreCase) && SkinImages.ParseColor(v) is int c && SkinImages.MatchesKey(c, rgb)) ||
        (kv.Key.EndsWith("image", StringComparison.OrdinalIgnoreCase) &&
         View.Theme.Images.Get(v, SkinImages.ParseColor(GetString("transparencycolor")), GetNumber("hueshift"), GetNumber("saturation")) is { } img && img.Shows(rgb))));

    /// <summary>Whether a click here should be handled by the element rather than drag the window.</summary>
    public virtual bool IsInteractive =>
        HasHandler("onclick") || HasHandler("onmousedown") || HasHandler("onmouseup") || HasHandler("ondblclick");

    /// <summary>Local hit test (point relative to element origin).</summary>
    public virtual bool HitTest(Point local) => local.X >= 0 && local.Y >= 0 && local.X < Width && local.Y < Height;

    /// <summary>Hit test for what is drawn over the negative-zIndex children (not a backgroundColor).</summary>
    public virtual bool HitTestImage(Point local) => HitTest(local);

    public virtual string? CurrentToolTip => NonEmpty(GetString("tooltip")) ?? NonEmpty(GetString("uptooltip"));
    protected static string? NonEmpty(string? s) => string.IsNullOrEmpty(s) ? null : s;

    public virtual Cursor? CurrentCursor => GetString("cursor").ToLowerInvariant() switch
    {
        "hand" => Cursors.Hand,
        "arrow" or "default" => Cursors.Arrow,
        "ibeam" => Cursors.IBeam,
        "wait" => Cursors.Wait,
        "cross" or "crosshair" => Cursors.Cross,
        "help" => Cursors.Help,
        "no" => Cursors.No,
        "sizeall" or "move" => Cursors.SizeAll,
        "sizenesw" => Cursors.SizeNESW,
        "sizens" => Cursors.SizeNS,
        "sizenwse" => Cursors.SizeNWSE,
        "sizewe" => Cursors.SizeWE,
        "uparrow" => Cursors.UpArrow,
        "appstarting" => Cursors.AppStarting,
        _ => null,
    };

    public virtual void OnMouseEnter() { IsHovered = true; Fire("onmouseover"); View.InvalidateVisual(); }
    public virtual void OnMouseLeave() { IsHovered = false; Fire("onmouseout"); View.InvalidateVisual(); }
    public virtual void OnMouseDown(Point local, MouseButton button) { IsPressed = true; Fire("onmousedown"); View.InvalidateVisual(); }
    public virtual void OnMouseMove(Point local, bool captured) => Fire("onmousemove");
    public virtual void OnMouseUp(Point local, MouseButton button, bool inside)
    {
        IsPressed = false;
        Fire("onmouseup");
        if (inside && button == MouseButton.Left && Enabled) OnClick(local);
        View.InvalidateVisual();
    }
    public virtual void OnClick(Point local) => Fire("onclick");
    public virtual void OnDoubleClick(Point local) => Fire("ondblclick");
    public virtual void OnMouseWheel(int delta) { }
    public virtual bool OnKeyDown(Key key) => false;

    // ---- rendering -----------------------------------------------------------------------

    /// <summary>Draws this element at its absolute position (children are drawn by the caller).</summary>
    public virtual void Render(DrawingContext dc, Rect bounds) { }

    /// <summary>Draws what sits under even the negative-zIndex children (a container's backgroundColor).</summary>
    public virtual void RenderBackdrop(DrawingContext dc, Rect bounds) { }

    /// <summary>Called once after load completes (bindings applied, scripts run).</summary>
    public virtual void OnLoaded() { }

    public virtual void OnUnloaded() { }

    /// <summary>Elements that contain their children visually (subviews, views, button groups).</summary>
    public virtual bool ClipsChildren => false;

    /// <summary>The image whose clippingColor pixels define the shape when no clippingImage is given.</summary>
    protected virtual string PrimaryImage => GetString("image");

    /// <summary>
    /// Mask for clippingColor/clippingImage, in element-local pixels (null = no mask). Pixels equal
    /// to clippingColor are cut out; a clippingImage without a colour clips by its own transparency.
    /// </summary>
    public SkinBitmap? ClipMask
    {
        get
        {
            string img = GetString("clippingimage");
            int? clip = SkinImages.ParseColor(GetString("clippingcolor"));
            if (img.Length == 0)
            {
                if (clip == null) return null;
                img = PrimaryImage;
                if (img.Length == 0) return null;
                // A subview's clippingColor cuts its background image into the subview's shape, which
                // clips its children too; clear pixels stay inside (Professional frames its visualizer so).
                if (this is SubviewElement and not SkinView) return View.Theme.Images.GetRaw(img)?.ShapeOf(clip.Value);
            }
            else if (clip == null)
            {
                // Black-and-white mask with no colour given: white is cut (unless it has real transparency).
                var raw = View.Theme.Images.GetRaw(img);
                if (raw == null) return null;
                bool hasAlpha = raw.Original.Any(p => (p >>> 24) < 255);
                return hasAlpha ? raw : View.Theme.Images.Get(img, 0xFFFFFF);
            }
            return View.Theme.Images.Get(img, clip);
        }
    }

    /// <summary>True when the mask repeats (a tiled background that doubles as the clipping shape).</summary>
    public virtual bool MaskTiled => false;

    /// <summary>A shape the element is also cut to, and where its top-left sits in view coordinates (null = none).</summary>
    public virtual (SkinBitmap Mask, Point At)? HostMask => null;

    public bool MaskAllows(Point local)
    {
        if (HostMask is { } host)
        {
            var abs = Absolute;
            if (host.Mask.Alpha((int)(local.X + abs.X - host.At.X), (int)(local.Y + abs.Y - host.At.Y)) == 0) return false;
        }
        var m = ClipMask;
        if (m == null) return true;
        int x = (int)local.X, y = (int)local.Y;
        if (MaskTiled && m.Width > 0 && m.Height > 0) { x = ((x % m.Width) + m.Width) % m.Width; y = ((y % m.Height) + m.Height) % m.Height; }
        return m.Alpha(x, y) > 0;
    }

    protected SkinBitmap? Img(string attr, bool keyed = true)
    {
        string name = GetString(attr);
        if (name.Length == 0) return null;
        return keyed
            ? View.Theme.Images.Get(name, SkinImages.ParseColor(GetString("transparencycolor")), GetNumber("hueshift"), GetNumber("saturation"))
            : View.Theme.Images.GetRaw(name);
    }

    readonly Dictionary<string, DateTime> _animStart = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// The frame of <paramref name="bmp"/> to draw now. Animated GIFs run from when this element
    /// first showed them (restarting when it is shown again), and the view is asked to redraw
    /// when the next frame is due.
    /// </summary>
    protected ImageSource FrameOf(SkinBitmap bmp, string key)
    {
        if (!bmp.IsAnimated) return bmp.Source;
        var now = View.HeldAt ?? DateTime.UtcNow;
        if (!_animStart.TryGetValue(key, out var start)) _animStart[key] = start = now;
        var frame = bmp.FrameAt((now - start).TotalMilliseconds, out double next);
        if (next >= 0) View.RequestAnimationFrame(next);
        _shown[key] = frame;
        return frame;
    }

    readonly Dictionary<string, BitmapSource> _shown = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>The frame of <paramref name="bmp"/> last drawn under <paramref name="key"/> (its first before any).</summary>
    protected BitmapSource ShownFrame(SkinBitmap bmp, string key) => bmp.IsAnimated && _shown.TryGetValue(key, out var f) ? f : bmp.Source;

    /// <summary>Restart animated GIFs (an element shown again plays its animation from the top).</summary>
    public void RestartAnimations()
    {
        _animStart.Clear();
        _shown.Clear();
        foreach (var c in Children) c.RestartAnimations();
    }

    protected static void DrawBitmap(DrawingContext dc, SkinBitmap? bmp, double x, double y)
    {
        if (bmp == null) return;
        dc.DrawImage(bmp.Source, new Rect(x, y, bmp.Width, bmp.Height));
    }

    public override string ToString() => $"<{Tag}{(Id != null ? " id=" + Id : "")}>";
}
