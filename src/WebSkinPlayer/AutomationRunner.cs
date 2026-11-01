using System.IO;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using WebSkinPlayer.Skins;

namespace WebSkinPlayer;

/// <summary>
/// Developer test hook: "--automate script.txt" replays input against the active skin view
/// through the same code paths real mouse input uses. One command per line (view coordinates):
///   skin &lt;path&gt; | click x y | rclick x y | move x y | drag x1 y1 x2 y2 | wheel x y delta | key Name
///   wait ms | capture file.png | record target frames ms scale dir | eval js-expression | vis id preset |
///   viscolor id spec | play title | fullmode | exit
/// Results go to the app log.
/// </summary>
public static class AutomationRunner
{
    public static async void Run(string scriptPath)
    {
        var lines = File.ReadAllLines(scriptPath);
        foreach (var raw in lines)
        {
            string line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#')) continue;
            var parts = line.Split(' ', 2);
            string cmd = parts[0].ToLowerInvariant();
            string arg = parts.Length > 1 ? parts[1] : "";
            double[] n = arg.Split(' ', StringSplitOptions.RemoveEmptyEntries).Select(s => double.TryParse(s, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var d) ? d : 0).ToArray();
            var view = App.Modes.CurrentTheme?.ActiveView;
            try
            {
                switch (cmd)
                {
                    case "skin": App.Modes.ShowSkin(arg); break;
                    case "fullmode": App.Modes.ShowFullMode(); break;
                    case "play": // play <title> : a 40 s song by that name playing in the followed app (it joins the history)
                        App.Services.Media.Simulate(new Media.MediaSnapshot
                        {
                            Title = arg, SourceApp = "Automation", Status = Media.MediaStatus.Playing, Duration = TimeSpan.FromSeconds(40),
                        }, () => TimeSpan.Zero);
                        break;
                    case "wait": await Task.Delay((int)n[0]); break;
                    case "move": view?.HandleMouseMove(new Point(n[0], n[1])); break;
                    case "click":
                        if (view == null) break;
                        view.HandleMouseMove(new Point(n[0], n[1]));
                        bool taken = view.HandleMouseDown(new Point(n[0], n[1]), MouseButton.Left, 1);
                        await Task.Delay(60);
                        view.HandleMouseUp(new Point(n[0], n[1]), MouseButton.Left);
                        Log.Write($"[auto] click {n[0]},{n[1]} taken={taken}");
                        break;
                    case "rclick":
                        view?.HandleMouseDown(new Point(n[0], n[1]), MouseButton.Right, 1);
                        break;
                    case "drag":
                        if (view == null) break;
                        view.HandleMouseMove(new Point(n[0], n[1]));
                        view.HandleMouseDown(new Point(n[0], n[1]), MouseButton.Left, 1);
                        for (int i = 1; i <= 10; i++)
                        {
                            await Task.Delay(30);
                            view.HandleMouseMove(new Point(n[0] + (n[2] - n[0]) * i / 10, n[1] + (n[3] - n[1]) * i / 10));
                        }
                        view.HandleMouseUp(new Point(n[2], n[3]), MouseButton.Left);
                        Log.Write($"[auto] drag {n[0]},{n[1]} -> {n[2]},{n[3]}");
                        break;
                    case "wheel": view?.HandleMouseWheel(new Point(n[0], n[1]), (int)n[2]); break;
                    case "key":
                        if (view != null && Enum.TryParse<Key>(arg, true, out var k)) view.HandleKeyDown(k);
                        break;
                    case "eval":
                        if (view == null) break;
                        var v = view.Bridge.EvaluateIn(view, arg, "automation");
                        Log.Write($"[auto] eval {arg} = {Skins.Scripting.ScriptObject.ToStr(v)}");
                        break;
                    case "vis":
                        App.Services.Visualizations.SetEffect(parts.Length > 1 ? arg.Split(' ')[0] : null, n.Length > 1 ? (int)n[1] : 0);
                        break;
                    case "choose": // choose <id> [preset] : as picking it from the right-click Visualizations menu
                        App.Services.Visualizations.Choose(arg.Split(' ')[0], n.Length > 1 ? (int)n[1] : 0);
                        break;
                    case "update": // update : as Install Now on an available update, without the question
                        if (UpdateChecker.Available is { } update) UI.UpdateWindow.Start(update, null, quiet: true);
                        else Log.Write("[auto] no update available");
                        break;
                    case "viscolor": // viscolor <id> [palette:Fire|hue:120|custom:#RRGGBB] : as the Colors menu (no spec = original)
                    {
                        var c = arg.Split(' ', 2);
                        Visualization.VisualizationColors.Set(c[0], c.Length > 1 ? c[1].Trim() : "");
                        break;
                    }
                    case "capture":
                        if (Snap("skin", 1) is { } skinShot) { Save(skinShot, arg); Log.Write($"[auto] captured {arg}"); }
                        break;
                    case "capturefull":
                        if (Snap("full", 1) is { } fullShot) Save(fullShot, arg);
                        break;
                    case "record": // record skin|full|saver|widget <frames> <ms> <scale> <dir> : numbered PNG frames
                    {
                        var r = arg.Split(' ', 5);
                        int frames = int.Parse(r[1]), every = int.Parse(r[2]);
                        double scale = double.Parse(r[3], System.Globalization.CultureInfo.InvariantCulture);
                        var shots = new List<BitmapSource>();
                        var clock = System.Diagnostics.Stopwatch.StartNew();
                        for (int i = 0; i < frames; i++)
                        {
                            if (Snap(r[0], scale) is { } shot) shots.Add(shot);
                            var due = TimeSpan.FromMilliseconds(every * (i + 1)) - clock.Elapsed;
                            if (due > TimeSpan.Zero) await Task.Delay(due);
                        }
                        Directory.CreateDirectory(r[4]);
                        for (int i = 0; i < shots.Count; i++) Save(shots[i], Path.Combine(r[4], $"frame{i:0000}.png"));
                        Log.Write($"[auto] recorded {shots.Count} frames of {r[0]} in {clock.ElapsedMilliseconds} ms");
                        break;
                    }
                    case "state":
                        Log.Write($"[auto] state {arg}: widgetMode={App.Modes.InWidgetMode} widgetShown={App.Modes.WidgetShown} " +
                                  $"full={(App.Modes.FullWindow?.IsVisible ?? false)} skin={(App.Modes.CurrentTheme?.ActiveView?.Window?.IsVisible ?? false)}");
                        break;
                    case "widget": App.Modes.CollapseToWidget(); break;
                    case "restore": App.Modes.RestoreFromWidget(null); break;
                    case "tab":
                        App.Modes.FullWindow?.SelectTab(arg);
                        break;
                    case "ontop": // ontop on|off|toggle switches Always on Top; "ontop ?" logs each window
                        if (arg == "?")
                            Log.Write($"[auto] ontop setting={App.Modes.AlwaysOnTop} windows: " + string.Join(", ", Application.Current.Windows.Cast<Window>()
                                .Where(w => w.IsVisible && w.GetType().Name != "Window").Select(w => $"{w.GetType().Name}={(w.Topmost ? "top" : "normal")}")));
                        else App.Modes.SetAlwaysOnTop(arg == "toggle" ? !App.Modes.AlwaysOnTop : arg == "on");
                        break;
                    case "capturesaver": // capturesaver file.png : the first screen saver window
                        if (Snap("saver", 1) is { } saverShot) Save(saverShot, arg);
                        break;
                    case "saver": // saver start|stop|next|? : the visualizer screen saver
                        if (App.ScreenSaver is not { } saver) break;
                        if (arg == "start") saver.Start(manual: true);
                        else if (arg == "stop") saver.Stop(lockAfter: false);
                        else if (arg == "next") saver.Next();
                        else if (arg.StartsWith("show ", StringComparison.OrdinalIgnoreCase)) saver.Show(arg[5..].Trim(), 0);
                        else Log.Write($"[auto] saver running={saver.IsRunning} showing={saver.ShowingName} windows={Application.Current.Windows.OfType<UI.ScreenSaverWindow>().Count()} {saver.Diagnostics}");
                        break;
                    case "uiclick": // uiclick ButtonName (full mode)
                        if (App.Modes.FullWindow?.FindName(arg) is System.Windows.Controls.Primitives.ButtonBase b)
                            b.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.Primitives.ButtonBase.ClickEvent));
                        break;
                    case "uiselect": // uiselect ListName item text
                    {
                        var sel = arg.Split(' ', 2);
                        if (sel.Length == 2 && App.Modes.FullWindow?.FindName(sel[0]) is System.Windows.Controls.Primitives.Selector list)
                            list.SelectedItem = list.Items.Cast<object>().FirstOrDefault(i => i.ToString()!.Trim().StartsWith(sel[1], StringComparison.OrdinalIgnoreCase));
                        break;
                    }
                    case "exit":
                        App.Modes.Exit();
                        return;
                }
            }
            catch (Exception ex) { Log.Write($"[auto] {line} failed: {ex.Message}"); }
            await Dispatcher.Yield(DispatcherPriority.Background);
        }
    }

    /// <summary>The active skin view, full mode, the first screen saver window or the taskbar widget, rendered at
    /// <paramref name="scale"/> × 96 DPI (1 = one pixel per skin pixel).</summary>
    static BitmapSource? Snap(string target, double scale)
    {
        Visual? visual;
        double width, height;
        if (target == "skin")
        {
            if (App.Modes.CurrentTheme?.ActiveView is not { } view) return null;
            var dv = new DrawingVisual();
            using (var dc = dv.RenderOpen()) view.RenderTree(dc);
            (visual, width, height) = (dv, view.Width, view.Height);
        }
        else
        {
            var root = target switch
            {
                "full" => App.Modes.FullWindow?.Content,
                "saver" => Application.Current.Windows.OfType<UI.ScreenSaverWindow>().FirstOrDefault()?.Content,
                "widget" => Application.Current.Windows.Cast<Window>().FirstOrDefault(w => w.Title == "WebSkin Player taskbar widget")?.Content,
                _ => null,
            } as FrameworkElement;
            if (root == null) return null;
            (visual, width, height) = (root, root.ActualWidth, root.ActualHeight);
        }
        var rtb = new RenderTargetBitmap(Math.Max(1, (int)Math.Round(width * scale)), Math.Max(1, (int)Math.Round(height * scale)), 96 * scale, 96 * scale, PixelFormats.Pbgra32);
        rtb.Render(visual);
        rtb.Freeze();
        return rtb;
    }

    static void Save(BitmapSource image, string path)
    {
        var enc = new PngBitmapEncoder();
        enc.Frames.Add(BitmapFrame.Create(image));
        using var fs = File.Create(path);
        enc.Save(fs);
    }
}
