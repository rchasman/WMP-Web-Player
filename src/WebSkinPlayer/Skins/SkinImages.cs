using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace WebSkinPlayer.Skins;

/// <summary>A decoded skin image: original pixels (for colour lookups) plus a ready-to-draw bitmap.</summary>
public sealed class SkinBitmap
{
    public int Width { get; }
    public int Height { get; }
    /// <summary>Source pixels as 0xAARRGGBB before any colour keying.</summary>
    public int[] Original { get; }
    /// <summary>Drawable pixels (premultiplied BGRA) after keying/hue shift.</summary>
    public int[] Pixels { get; }
    public BitmapSource Source { get; }

    /// <summary>Composed frames of an animated GIF (null for still images); Frames[0] == Source.</summary>
    public BitmapSource[]? Frames { get; }
    /// <summary>Per-frame display time in ms.</summary>
    public int[]? Delays { get; }
    /// <summary>GIF loop count: 0 = forever, n = play n times.</summary>
    public int Loops { get; }
    public bool IsAnimated => Frames is { Length: > 1 };

    public SkinBitmap(int w, int h, int[] original, int[] premultiplied, BitmapSource[]? frames = null, int[]? delays = null, int loops = 1)
    {
        Width = w; Height = h; Original = original; Pixels = premultiplied;
        var bmp = BitmapSource.Create(w, h, 96, 96, PixelFormats.Pbgra32, null, premultiplied, w * 4);
        bmp.Freeze();
        Source = bmp;
        if (frames is { Length: > 1 })
        {
            frames[0] = bmp;
            Frames = frames;
            Delays = delays;
            Loops = loops;
        }
    }

    /// <summary>
    /// The frame to show <paramref name="elapsedMs"/> after the animation started, and how long
    /// until it changes (-1 once a finite animation has finished, or for still images).
    /// </summary>
    public BitmapSource FrameAt(double elapsedMs, out double untilNext)
    {
        untilNext = -1;
        if (!IsAnimated) return Source;
        int total = 0;
        foreach (var d in Delays!) total += d;
        if (total <= 0) return Source;
        if (Loops > 0 && elapsedMs >= (double)total * Loops) return Frames![^1];
        double t = elapsedMs % total;
        for (int i = 0; i < Frames!.Length; i++)
        {
            if (t < Delays[i]) { untilNext = Delays[i] - t; return Frames[i]; }
            t -= Delays[i];
        }
        return Frames[^1];
    }

    readonly Dictionary<int, SkinBitmap> _shapes = new();

    /// <summary>
    /// The image's shape: every pixel except the key colour, clear ones included (a frame image's
    /// see-through middle belongs to the shape; its colour-keyed outside does not).
    /// </summary>
    public SkinBitmap ShapeOf(int key)
    {
        if (_shapes.TryGetValue(key, out var shape)) return shape;
        var px = new int[Original.Length];
        for (int i = 0; i < px.Length; i++) px[i] = SkinImages.MatchesKey(Original[i] & 0xFFFFFF, key) ? 0 : unchecked((int)0xFF000000);
        return _shapes[key] = new SkinBitmap(Width, Height, px, px);
    }

    /// <summary>RGB (no alpha) at x,y or -1 when outside.</summary>
    public int Rgb(int x, int y) => (uint)x < (uint)Width && (uint)y < (uint)Height ? Original[y * Width + x] & 0xFFFFFF : -1;

    /// <summary>Drawn alpha at x,y (0 when outside).</summary>
    public int Alpha(int x, int y) => (uint)x < (uint)Width && (uint)y < (uint)Height ? (Pixels[y * Width + x] >>> 24) : 0;

    readonly Dictionary<int, bool> _shows = new();

    /// <summary>Whether any frame of the image has an opaque pixel of <paramref name="rgb"/> or one of its 16-bit forms.</summary>
    public bool Shows(int rgb)
    {
        if (_shows.TryGetValue(rgb, out bool known)) return known;
        bool In(int[] px) => px.Any(c => (c >>> 24) == 255 && SkinImages.MatchesKey(c & 0xFFFFFF, rgb));
        bool InFrame(BitmapSource f)
        {
            var px = new int[f.PixelWidth * f.PixelHeight];
            f.CopyPixels(px, f.PixelWidth * 4, 0);
            return In(px);
        }
        return _shows[rgb] = In(Pixels) || (Frames ?? []).Skip(1).Any(InFrame);
    }

    /// <summary>Alpha at x,y in one of this image's frames (0 when outside).</summary>
    public int Alpha(BitmapSource frame, int x, int y)
    {
        if (frame == Source || (uint)x >= (uint)frame.PixelWidth || (uint)y >= (uint)frame.PixelHeight) return Alpha(x, y);
        var px = new int[1];
        frame.CopyPixels(new System.Windows.Int32Rect(x, y, 1, 1), px, 4, 0);
        return px[0] >>> 24;
    }
}

/// <summary>Per-skin image cache with colour-key and hue/saturation variants.</summary>
public sealed class SkinImages
{
    readonly SkinPackage _pkg;
    sealed record Decoded(int W, int H, int[] Px, List<int[]>? Frames = null, int[]? Delays = null, int Loops = 1);

    readonly Dictionary<string, Decoded?> _decoded = new(StringComparer.OrdinalIgnoreCase);
    readonly Dictionary<string, SkinBitmap?> _variants = new(StringComparer.OrdinalIgnoreCase);

    public SkinImages(SkinPackage pkg) => _pkg = pkg;

    /// <summary>
    /// Whether <paramref name="rgb"/> is the key colour <paramref name="key"/>. The key's 16-bit forms
    /// (5-5-5 and 5-6-5) match too: bitmaps saved from a 16-bit desktop hold those (YIL Online Music
    /// Awards' speakers are #639CFF for the key #6699FF).
    /// </summary>
    public static bool MatchesKey(int rgb, int key) => rgb == key || rgb == SixteenBit(key, 5) || rgb == SixteenBit(key, 6);

    /// <summary><paramref name="rgb"/> with each channel cut to 5 bits (green to <paramref name="greenBits"/>) and widened back to 8.</summary>
    static int SixteenBit(int rgb, int greenBits)
    {
        int Channel(int shift, int bits)
        {
            int v = ((rgb >> shift) & 255) >> (8 - bits);
            return ((v << (8 - bits)) | (v >> (2 * bits - 8))) << shift;
        }
        return Channel(16, 5) | Channel(8, greenBits) | Channel(0, 5);
    }

    public static int? ParseColor(string? s)
    {
        if (string.IsNullOrWhiteSpace(s)) return null;
        s = s.Trim();
        if (s.Equals("none", StringComparison.OrdinalIgnoreCase) || s.Equals("transparent", StringComparison.OrdinalIgnoreCase)) return null;
        if (s.StartsWith('#') && s.Length == 7 && int.TryParse(s[1..], System.Globalization.NumberStyles.HexNumber, null, out int hex)) return hex;
        if (s.Length == 6 && int.TryParse(s, System.Globalization.NumberStyles.HexNumber, null, out hex)) return hex;
        try
        {
            var c = (Color)ColorConverter.ConvertFromString(s);
            return (c.R << 16) | (c.G << 8) | c.B;
        }
        catch { return null; }
    }

    Decoded? Decode(string name)
    {
        if (_decoded.TryGetValue(name, out var d)) return d;
        d = null;
        try
        {
            byte[]? bytes = WmpResources.IsResourceUrl(name) ? WmpResources.ReadBytes(name) : _pkg.ReadBytes(name);
            if (bytes != null)
            {
                var decoder = BitmapDecoder.Create(new MemoryStream(bytes), BitmapCreateOptions.PreservePixelFormat | BitmapCreateOptions.IgnoreColorProfile, BitmapCacheOption.OnLoad);
                if (decoder is GifBitmapDecoder gif && gif.Frames.Count > 1 && DecodeGif(gif) is { } anim)
                {
                    _decoded[name] = anim;
                    return anim;
                }
                BitmapSource frame = decoder.Frames[0];
                bool hasAlpha = frame.Format.Masks.Count == 4 || frame.Format == PixelFormats.Indexed8 || frame.Format == PixelFormats.Indexed4 || frame.Format == PixelFormats.Indexed2 || frame.Format == PixelFormats.Indexed1;
                var conv = new FormatConvertedBitmap(frame, PixelFormats.Bgra32, null, 0);
                int w = conv.PixelWidth, h = conv.PixelHeight;
                var px = new int[w * h];
                conv.CopyPixels(px, w * 4, 0);
                // 32bpp BMPs often carry a garbage/zero alpha channel; WMP ignored it.
                bool isBmp = bytes.Length > 2 && bytes[0] == 'B' && bytes[1] == 'M';
                if (isBmp || !hasAlpha)
                    for (int i = 0; i < px.Length; i++) px[i] |= unchecked((int)0xFF000000);
                d = new Decoded(w, h, px);
            }
            else Log.Write($"Skin image not found: {name}");
        }
        catch (Exception ex) { Log.Write($"Skin image {name} failed: {ex.Message}"); }
        _decoded[name] = d;
        return d;
    }

    /// <summary>
    /// Composes an animated GIF into full-size frames (frame offsets + disposal methods), with
    /// per-frame delays and the NETSCAPE loop count — WMP played these in skins.
    /// </summary>
    static Decoded? DecodeGif(GifBitmapDecoder gif)
    {
        int W = 0, H = 0;
        try { W = Convert.ToInt32(gif.Metadata?.GetQuery("/logscrdesc/Width")); H = Convert.ToInt32(gif.Metadata?.GetQuery("/logscrdesc/Height")); } catch { }
        if (W <= 0 || H <= 0) { W = gif.Frames[0].PixelWidth; H = gif.Frames[0].PixelHeight; }
        int count = gif.Frames.Count;
        // Keep memory sane for huge animations: drop frames beyond ~40M pixels.
        int maxFrames = Math.Clamp(40_000_000 / Math.Max(1, W * H), 2, 400);
        int step = count > maxFrames ? (int)Math.Ceiling(count / (double)maxFrames) : 1;

        var canvas = new int[W * H];
        var frames = new List<int[]>();
        var delays = new List<int>();
        var disposals = new int[count];
        int pendingDelay = 0;
        for (int i = 0; i < count; i++)
        {
            var f = gif.Frames[i];
            var meta = f.Metadata as BitmapMetadata;
            int left = Q(meta, "/imgdesc/Left"), top = Q(meta, "/imgdesc/Top");
            int disposal = disposals[i] = Q(meta, "/grctlext/Disposal");
            int delay = Q(meta, "/grctlext/Delay") * 10;
            if (delay < 20) delay = 100; // browsers' convention for 0/too-fast delays

            int[]? previous = disposal == 3 ? (int[])canvas.Clone() : null;
            var conv = new FormatConvertedBitmap(f, PixelFormats.Bgra32, null, 0);
            int fw = conv.PixelWidth, fh = conv.PixelHeight;
            var fp = new int[fw * fh];
            conv.CopyPixels(fp, fw * 4, 0);
            for (int y = 0; y < fh; y++)
            {
                int cy = top + y;
                if ((uint)cy >= (uint)H) continue;
                for (int x = 0; x < fw; x++)
                {
                    int cx = left + x;
                    if ((uint)cx >= (uint)W) continue;
                    int c = fp[y * fw + x];
                    if ((c >>> 24) != 0) canvas[cy * W + cx] = c;
                }
            }
            pendingDelay += delay;
            if (i % step == 0 || i == count - 1)
            {
                frames.Add((int[])canvas.Clone());
                delays.Add(pendingDelay);
                pendingDelay = 0;
            }
            if (disposal == 2)
                for (int y = Math.Max(0, top); y < Math.Min(H, top + fh); y++)
                    Array.Clear(canvas, y * W + Math.Max(0, left), Math.Max(0, Math.Min(W, left + fw) - Math.Max(0, left)));
            else if (previous != null) canvas = previous;
        }

        int loops = 1; // no NETSCAPE block: play once
        try
        {
            if (gif.Metadata?.GetQuery("/appext/Data") is byte[] ext && ext.Length >= 4 && ext[1] == 1)
                loops = ext[2] | (ext[3] << 8); // 0 = forever
        }
        catch { }
        // A finite animation whose last frame alone asks to be cleared (background or previous) ends
        // cleared: QuickSilver's shutter opens onto the display under it. When every frame asks, that
        // was only how the GIF was saved, and the last frame stays (Xbox Live's buttons, in its preview).
        int last = disposals[^1];
        if (loops > 0 && last is 2 or 3 && disposals.Any(d => d != last))
        {
            frames.Add(canvas);
            delays.Add(0);
        }
        return new Decoded(W, H, frames[0], frames, delays.ToArray(), loops);

        static int Q(BitmapMetadata? m, string q)
        {
            try { return m?.GetQuery(q) is { } v ? Convert.ToInt32(v) : 0; } catch { return 0; }
        }
    }

    /// <summary>Image without keying, for mapping/position/clipping lookups.</summary>
    public SkinBitmap? GetRaw(string? name) => Get(name, null);

    /// <summary>
    /// Image with <paramref name="keyColor"/> (transparencyColor or clippingColor) made fully transparent,
    /// and optional hue shift (fraction of a turn) / saturation multiplier applied.
    /// </summary>
    /// <summary>WMP's built-in dynamic images ("WMPImage_AlbumArtLarge", "WMPImage_AdBanner", ...).</summary>
    public static bool IsDynamic(string? name) => name != null && name.TrimStart().StartsWith("WMPImage_", StringComparison.OrdinalIgnoreCase);

    SkinBitmap? _art;
    string _artKey = "";

    /// <summary>The current track's album art, square, or WMP's placeholder when there is none.</summary>
    SkinBitmap? AlbumArt(string name)
    {
        var ctx = App.Services.Visualizations.Context;
        int size = name.EndsWith("Small", StringComparison.OrdinalIgnoreCase) ? 75 : 200;
        string key = $"{size}|{ctx.ArtworkVersion}";
        if (key == _artKey && _art != null) return _art;
        var px = new int[size * size];
        if (ctx.Artwork != null && ctx.ArtworkWidth > 0)
        {
            int side = Math.Min(ctx.ArtworkWidth, ctx.ArtworkHeight);
            int ox = (ctx.ArtworkWidth - side) / 2, oy = (ctx.ArtworkHeight - side) / 2;
            for (int y = 0; y < size; y++)
                for (int x = 0; x < size; x++)
                {
                    int sx0 = ox + x * side / size, sx1 = Math.Max(sx0 + 1, ox + (x + 1) * side / size);
                    int sy0 = oy + y * side / size, sy1 = Math.Max(sy0 + 1, oy + (y + 1) * side / size);
                    int r = 0, g = 0, b = 0, n = 0;
                    for (int sy = sy0; sy < sy1; sy++)
                        for (int sx = sx0; sx < sx1; sx++)
                        {
                            int c = ctx.Artwork[sy * ctx.ArtworkWidth + sx];
                            r += (c >> 16) & 255; g += (c >> 8) & 255; b += c & 255; n++;
                        }
                    px[y * size + x] = unchecked((int)0xFF000000) | ((r / n) << 16) | ((g / n) << 8) | (b / n);
                }
        }
        else
        {
            // Placeholder: dark blue gradient disc, like WMP 11's "no album art".
            float c = size / 2f;
            for (int y = 0; y < size; y++)
                for (int x = 0; x < size; x++)
                {
                    float d = MathF.Sqrt((x - c) * (x - c) + (y - c) * (y - c)) / c;
                    int v = (int)(40 + 70 * (1 - y / (float)size));
                    int ring = d < 0.8f && d > 0.12f ? (int)(25 * MathF.Sin(d * 40)) + 20 : 0;
                    px[y * size + x] = unchecked((int)0xFF000000) | (Math.Clamp(v / 3 + ring, 0, 255) << 16) | (Math.Clamp(v / 2 + ring, 0, 255) << 8) | Math.Clamp(v + ring, 0, 255);
                }
        }
        _art = new SkinBitmap(size, size, px, px);
        _artKey = key;
        return _art;
    }

    public SkinBitmap? Get(string? name, int? keyColor, double hueShift = 0, double saturation = 1, int? secondKey = null)
    {
        if (string.IsNullOrWhiteSpace(name)) return null;
        name = name.Trim();
        if (IsDynamic(name))
            return name.StartsWith("WMPImage_AlbumArt", StringComparison.OrdinalIgnoreCase) ? AlbumArt(name) : null;
        string key = $"{name}|{keyColor}|{secondKey}|{hueShift:F3}|{saturation:F3}";
        if (_variants.TryGetValue(key, out var v)) return v;
        var d = Decode(name);
        if (d == null) { _variants[key] = null; return null; }
        bool tint = Math.Abs(hueShift) > 0.0001 || Math.Abs(saturation - 1) > 0.0001;
        int[] Process(int[] src)
        {
            var outPx = new int[src.Length];
            for (int i = 0; i < src.Length; i++)
            {
                int c = src[i];
                int rgb = c & 0xFFFFFF;
                int a = (c >>> 24) & 255;
                if ((keyColor is int k1 && MatchesKey(rgb, k1)) || (secondKey is int k2 && MatchesKey(rgb, k2))) { outPx[i] = 0; continue; }
                if (tint) rgb = Tint(rgb, hueShift, saturation);
                int r = (rgb >> 16) & 255, g = (rgb >> 8) & 255, b = rgb & 255;
                if (a < 255) { r = r * a / 255; g = g * a / 255; b = b * a / 255; }
                outPx[i] = (a << 24) | (r << 16) | (g << 8) | b;
            }
            return outPx;
        }
        var first = Process(d.Px);
        BitmapSource[]? frames = null;
        if (d.Frames is { Count: > 1 })
        {
            frames = new BitmapSource[d.Frames.Count];
            for (int i = 1; i < frames.Length; i++)
            {
                var f = BitmapSource.Create(d.W, d.H, 96, 96, PixelFormats.Pbgra32, null, Process(d.Frames[i]), d.W * 4);
                f.Freeze();
                frames[i] = f;
            }
        }
        v = new SkinBitmap(d.W, d.H, d.Px, first, frames, d.Delays, d.Loops);
        _variants[key] = v;
        return v;
    }

    static int Tint(int rgb, double hueShift, double saturation)
    {
        double r = ((rgb >> 16) & 255) / 255.0, g = ((rgb >> 8) & 255) / 255.0, b = (rgb & 255) / 255.0;
        double max = Math.Max(r, Math.Max(g, b)), min = Math.Min(r, Math.Min(g, b));
        double l = (max + min) / 2, h = 0, s = 0;
        if (max != min)
        {
            double dd = max - min;
            s = l > 0.5 ? dd / (2 - max - min) : dd / (max + min);
            h = max == r ? (g - b) / dd + (g < b ? 6 : 0) : max == g ? (b - r) / dd + 2 : (r - g) / dd + 4;
            h /= 6;
        }
        h = ((h + hueShift) % 1 + 1) % 1;
        s = Math.Clamp(s * saturation, 0, 1);
        double q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q;
        double Hue(double t)
        {
            t = (t % 1 + 1) % 1;
            if (t < 1 / 6.0) return p + (q - p) * 6 * t;
            if (t < 0.5) return q;
            if (t < 2 / 3.0) return p + (q - p) * (2 / 3.0 - t) * 6;
            return p;
        }
        int R = (int)Math.Round((s == 0 ? l : Hue(h + 1 / 3.0)) * 255), G = (int)Math.Round((s == 0 ? l : Hue(h)) * 255), B = (int)Math.Round((s == 0 ? l : Hue(h - 1 / 3.0)) * 255);
        return (R << 16) | (G << 8) | B;
    }
}
