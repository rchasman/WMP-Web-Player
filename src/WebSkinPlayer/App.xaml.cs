using System.IO;
using System.Windows;
using System.Windows.Threading;
using WebSkinPlayer.Audio;
using WebSkinPlayer.Media;
using WebSkinPlayer.Skins;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer;

/// <summary>Process-wide services shared by full mode and every skin window.</summary>
public sealed class Services
{
    public required AppSettings Settings { get; init; }
    public required MediaBridge Media { get; init; }
    public required AppVolume Volume { get; init; }
    public required LoopbackAnalyzer Analyzer { get; init; }
    public required VisualizationService Visualizations { get; init; }
    public required PlayerModel Player { get; init; }
    public required SkinLibrary Skins { get; init; }
}

public partial class App : Application
{
    public static Services Services { get; private set; } = null!;
    public static ModeController Modes { get; private set; } = null!;
    /// <summary>The visualizer screen saver; null in the test modes.</summary>
    public static ScreenSaver? ScreenSaver { get; private set; }

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        DispatcherUnhandledException += OnUnhandled;

        // --import-vis <dll|zip|folder> [label]: adds WMP visualization plug-ins, writes a report, and exits.
        int import = Array.FindIndex(e.Args, a => a.Equals("--import-vis", StringComparison.OrdinalIgnoreCase));
        if (import >= 0 && e.Args.Length > import + 1)
        {
            var lines = PluginLibrary.Import(e.Args[import + 1], e.Args.Length > import + 2 ? e.Args[import + 2] : Path.GetFileName(e.Args[import + 1]));
            foreach (var line in lines) Log.Write("[import] " + line);
            Directory.CreateDirectory(PluginLibrary.Folder);
            File.WriteAllLines(Path.Combine(PluginLibrary.Folder, "import-report.txt"), lines);
            Shutdown();
            return;
        }

        bool isTest = e.Args.Any(a => a.Equals("--test-skins", StringComparison.OrdinalIgnoreCase) || a.Equals("--test-live", StringComparison.OrdinalIgnoreCase));
        if (isTest) Sandbox.Enabled = true;
        if (!isTest)
        {
            if (!SingleInstance.TryBecomePrimary())
            {
                // Another copy is running (perhaps still starting): hand it our arguments and leave.
                // A second full copy would fight it for the pipe and the taskbar widget.
                SingleInstance.ForwardToPrimary(e.Args);
                Shutdown();
                return;
            }
            // Listen straight away so launches during startup aren't lost; act on them once started.
            SingleInstance.Listen(args => Dispatcher.InvokeAsync(async () => { await _started.Task; OnForwarded(args); }));
        }
        // "--exit" (sent by the installer and uninstaller) only closes a running copy.
        if (e.Args.Any(a => a.Equals("--exit", StringComparison.OrdinalIgnoreCase)))
        {
            Shutdown();
            return;
        }
        Log.Write("---- WebSkin Player starting");

        var settings = AppSettings.Load();
        var media = await MediaBridge.CreateAsync(Dispatcher);
        media.PreferredApp = settings.PreferredApp;
        if (media.PreferredApp != null) media.Reselect();
        var volume = new AppVolume();
        var analyzer = new LoopbackAnalyzer();
        var vis = new VisualizationService(analyzer, settings.EffectId, settings.EffectPreset);
        var player = new PlayerModel(media, volume);
        var skins = new SkinLibrary();

        Services = new Services
        {
            Settings = settings, Media = media, Volume = volume, Analyzer = analyzer,
            Visualizations = vis, Player = player, Skins = skins,
        };
        media.TrackChanged += () => vis.UpdateTrack(media.Current);
        vis.UpdateTrack(media.Current);
        vis.EffectChanged += () => { settings.EffectId = vis.EffectId; settings.EffectPreset = vis.Preset; };

        Modes = new ModeController();

        Skins.SkinView.DebugInput = e.Args.Any(a => a.Equals("--debug-input", StringComparison.OrdinalIgnoreCase));
        int test = Array.FindIndex(e.Args, a => a.Equals("--test-skins", StringComparison.OrdinalIgnoreCase));
        if (test >= 0 && e.Args.Length > test + 2)
        {
            SkinTestRunner.Run(e.Args[test + 1], e.Args[test + 2]);
            Shutdown();
            return;
        }
        int live = Array.FindIndex(e.Args, a => a.Equals("--test-live", StringComparison.OrdinalIgnoreCase));
        if (live >= 0 && e.Args.Length > live + 2)
        {
            double secs = e.Args.Length > live + 3 && double.TryParse(e.Args[live + 3], System.Globalization.CultureInfo.InvariantCulture, out var s) ? s : 6;
            int faults = await LiveTestRunner.RunAsync(e.Args[live + 1], e.Args[live + 2], secs);
            Shutdown(faults > 0 ? 1 : 0);
            return;
        }

        // Command line: a .wmz path (double-clicked skin) or --skin <path>.
        string? skinArg = e.Args.FirstOrDefault(a => a.EndsWith(".wmz", StringComparison.OrdinalIgnoreCase) || a.EndsWith(".wms", StringComparison.OrdinalIgnoreCase));
        if (skinArg != null)
        {
            var installed = skins.Install(skinArg);
            Modes.ShowSkin(installed ?? skinArg);
        }
        else if (settings.StartInSkinMode && settings.CurrentSkin != null && System.IO.File.Exists(settings.CurrentSkin))
            Modes.ShowSkin(settings.CurrentSkin);
        else
            Modes.ShowFullMode();

        if (skinArg == null && (settings.StartInWidgetMode || e.Args.Any(a => a.Equals("--widget", StringComparison.OrdinalIgnoreCase))))
            Modes.CollapseToWidget();

        int auto = Array.FindIndex(e.Args, a => a.Equals("--automate", StringComparison.OrdinalIgnoreCase));
        if (auto >= 0 && e.Args.Length > auto + 1) AutomationRunner.Run(e.Args[auto + 1]);

        _started.TrySetResult();
        if (!isTest)
        {
            ScreenSaver = new ScreenSaver();
            _ = UpdateChecker.CheckAsync();
        }
    }

    readonly TaskCompletionSource _started = new(TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>Another launch handed us its arguments: open the skin it was given, or just come forward.</summary>
    void OnForwarded(string[] args)
    {
        if (args.Any(a => a.Equals("--exit", StringComparison.OrdinalIgnoreCase)))
        {
            Log.Write("Exit requested by another launch");
            Modes.Exit();
            return;
        }
        string? skin = args.FirstOrDefault(a =>
            (a.EndsWith(".wmz", StringComparison.OrdinalIgnoreCase) || a.EndsWith(".wms", StringComparison.OrdinalIgnoreCase) ||
             a.EndsWith(".zip", StringComparison.OrdinalIgnoreCase)) && System.IO.File.Exists(a));
        Log.Write("Second launch handed over: " + (skin ?? "(no file)"));
        if (skin != null)
        {
            var installed = Services.Skins.Install(skin);
            if (skin.EndsWith(".zip", StringComparison.OrdinalIgnoreCase)) Modes.ShowFullMode();
            else Modes.ShowSkin(installed ?? skin);
            return;
        }
        if (Modes.InWidgetMode) Modes.RestoreFromWidget(null);
        else if (Modes.CurrentTheme?.ActiveView?.Window is { } w) { w.Show(); w.Activate(); }
        else Modes.ShowFullMode();
    }

    void OnUnhandled(object sender, DispatcherUnhandledExceptionEventArgs e)
    {
        Log.Write("Unhandled: " + e.Exception);
        e.Handled = true;
    }

    protected override void OnExit(ExitEventArgs e)
    {
        try
        {
            Services?.Settings.Save();
            Services?.Visualizations.Dispose();
            Services?.Analyzer.Dispose();
            Services?.Media.Dispose();
            Services?.Volume.Dispose();
        }
        catch { }
        base.OnExit(e);
    }
}
