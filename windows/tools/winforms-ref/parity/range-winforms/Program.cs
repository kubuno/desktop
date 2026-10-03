// Numeric parity probe — the REFERENCE half.
//
// Drives the real `System.Windows.Forms` range controls through every case in
// `..\range-cases.txt` and writes `..\range-winforms.json`. The port replays the
// same file through `kubuno-controls` and writes `range-port.json` in the same
// shape; `compare-range.ps1` diffs the two.
//
// Three deliberate choices make the comparison meaningful:
//
//  * **Real handles.** Every control is parented to a Form and its handle is
//    forced, so the native common control is in play — its scroll info, its
//    clamping, its tick marks. A model-only probe would be comparing two models.
//  * **Real scrolling.** A scroll bar's page/line arithmetic is not reachable
//    through any public setter: it lives in `ScrollBar.WmReflectScroll`, which
//    runs when the native bar sends `WM_HSCROLL`/`WM_VSCROLL` to its parent and
//    the parent reflects it back (`WM_REFLECT` = 0x2000). Posting that reflected
//    message directly therefore runs the toolkit's own scrolling code, and is
//    the only honest way to measure the reachable ceiling.
//  * **fr-FR.** `NumericUpDown` formats through `CultureInfo.CurrentCulture`
//    while the port formats French unconditionally. The culture is pinned to
//    fr-FR here so a difference in the output is a difference in the ARITHMETIC
//    and not merely a difference in locale.

using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;

namespace RangeParity;

internal static class Program
{
    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    private static extern IntPtr SendMessage(IntPtr hWnd, int msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool GetScrollInfo(IntPtr hWnd, int nBar, ref SCROLLINFO lpsi);

    [StructLayout(LayoutKind.Sequential)]
    private struct SCROLLINFO
    {
        public uint cbSize;
        public uint fMask;
        public int nMin;
        public int nMax;
        public uint nPage;
        public int nPos;
        public int nTrackPos;
    }

    private const int WM_REFLECT = 0x2000;   // WM_USER + 0x1C00, WinForms' reflection bit
    private const int WM_HSCROLL = 0x0114;
    private const int WM_VSCROLL = 0x0115;
    private const int TBM_GETNUMTICS = 0x0400 + 16;
    private const int SB_CTL = 2;
    private const uint SIF_ALL = 0x17;

    // SB_* scroll codes; these are also the numeric values of ScrollEventType.
    private static readonly Dictionary<string, int> ScrollCodes = new()
    {
        ["lineup"] = 0,
        ["linedown"] = 1,
        ["pageup"] = 2,
        ["pagedown"] = 3,
        ["first"] = 6,
        ["last"] = 7,
    };

    [STAThread]
    private static int Main(string[] args)
    {
        // An exception raised inside a control's `WndProc` does NOT come back
        // out of the `SendMessage` that provoked it: `Control.WndProc` hands it
        // to `Application.OnThreadException`, which puts up a modal dialog and
        // waits — hanging an unattended probe forever. `ThrowException` makes it
        // propagate instead, so the `try`/`catch` around each step sees it and
        // records an honest "err".
        //
        // It must be set BEFORE the first window exists on this thread.
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException);

        // French, explicitly: see the header comment.
        var fr = new CultureInfo("fr-FR");
        CultureInfo.CurrentCulture = fr;
        CultureInfo.DefaultThreadCurrentCulture = fr;

        string parityDir = args.Length > 0 ? args[0] : FindParityDir();
        string casesPath = Path.Combine(parityDir, "range-cases.txt");
        string outPath = Path.Combine(parityDir, "range-winforms.json");

        if (!File.Exists(casesPath))
        {
            Console.Error.WriteLine($"case file not found: {casesPath}");
            return 2;
        }

        List<Case> cases = Cases.Load(casesPath);
        Application.EnableVisualStyles();
        using var form = new Form { Left = -4000, Top = -4000, Width = 400, Height = 300 };

        var results = new List<CaseResult>(cases.Count);
        foreach (Case c in cases)
        {
            results.Add(Run(form, c));
        }

        var header = new List<KeyValue>
        {
            new("probe", "winforms"),
            new("culture", CultureInfo.CurrentCulture.Name),
            new("decimalSeparator", CultureInfo.CurrentCulture.NumberFormat.NumberDecimalSeparator),
            new("groupSeparator", CultureInfo.CurrentCulture.NumberFormat.NumberGroupSeparator),
        };

        // No BOM, and pure ASCII (non-ASCII is escaped as \uXXXX), so the file
        // reads the same from Rust, from PowerShell and from a text editor.
        File.WriteAllText(outPath, Json.Render(header, results), new UTF8Encoding(false));
        Console.WriteLine($"{cases.Count} cases -> {outPath}");
        return 0;
    }

    /// Walks up from the executable until the shared case file turns up, so the
    /// probe runs from anywhere (`dotnet run`, the build output, the comparer).
    private static string FindParityDir()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "range-cases.txt")))
            {
                return dir.FullName;
            }
            dir = dir.Parent;
        }
        return AppContext.BaseDirectory;
    }

    private static CaseResult Run(Form form, Case c)
    {
        Control control = c.Kind switch
        {
            "hscrollbar" => new HScrollBar(),
            "vscrollbar" => new VScrollBar(),
            "trackbar" => new TrackBar(),
            "numericupdown" => new NumericUpDown(),
            "domainupdown" => new DomainUpDown(),
            _ => throw new InvalidOperationException($"unknown kind '{c.Kind}' in case {c.Id}"),
        };
        form.Controls.Add(control);
        _ = control.Handle;   // force the native control into existence

        var steps = new List<StepResult>(c.Steps.Count);
        foreach (string step in c.Steps)
        {
            try
            {
                Apply(control, step);
                steps.Add(new StepResult(step, "ok", ""));
            }
            catch (Exception e)
            {
                // WinForms throws here; the port returns `Err`. The comparer
                // treats the two as the same outcome and only the DETAIL differs.
                steps.Add(new StepResult(step, "err", e.GetType().Name));
            }
        }

        var result = new CaseResult(c.Id, c.Kind, steps, State(control), Extra(control));
        form.Controls.Remove(control);
        control.Dispose();
        return result;
    }

    private static void Apply(Control control, string step)
    {
        int eq = step.IndexOf('=');
        string op = eq < 0 ? step : step[..eq];
        string arg = eq < 0 ? null : step[(eq + 1)..];

        switch (control)
        {
            case ScrollBar sb:
                switch (op)
                {
                    case "min": sb.Minimum = Int(arg); return;
                    case "max": sb.Maximum = Int(arg); return;
                    case "value": sb.Value = Int(arg); return;
                    case "small": sb.SmallChange = Int(arg); return;
                    case "large": sb.LargeChange = Int(arg); return;
                    default:
                        if (ScrollCodes.TryGetValue(op, out int code))
                        {
                            int msg = (sb is HScrollBar ? WM_HSCROLL : WM_VSCROLL) + WM_REFLECT;
                            SendMessage(sb.Handle, msg, code, sb.Handle);
                            return;
                        }
                        break;
                }
                break;

            case TrackBar tb:
                switch (op)
                {
                    case "min": tb.Minimum = Int(arg); return;
                    case "max": tb.Maximum = Int(arg); return;
                    case "value": tb.Value = Int(arg); return;
                    case "small": tb.SmallChange = Int(arg); return;
                    case "large": tb.LargeChange = Int(arg); return;
                    case "freq": tb.TickFrequency = Int(arg); return;
                }
                break;

            case NumericUpDown nud:
                switch (op)
                {
                    case "min": nud.Minimum = Dec(arg); return;
                    case "max": nud.Maximum = Dec(arg); return;
                    case "value": nud.Value = Dec(arg); return;
                    case "inc": nud.Increment = Dec(arg); return;
                    case "dp": nud.DecimalPlaces = Int(arg); return;
                    case "hex": nud.Hexadecimal = Flag(arg); return;
                    case "sep": nud.ThousandsSeparator = Flag(arg); return;
                    case "up": nud.UpButton(); return;
                    case "down": nud.DownButton(); return;
                }
                break;

            case DomainUpDown dud:
                switch (op)
                {
                    case "items":
                        foreach (string item in arg.Split(','))
                        {
                            dud.Items.Add(item);
                        }
                        return;
                    case "sel": dud.SelectedIndex = Int(arg); return;
                    case "sorted": dud.Sorted = Flag(arg); return;
                    case "wrap": dud.Wrap = Flag(arg); return;
                    case "up": dud.UpButton(); return;
                    case "down": dud.DownButton(); return;
                }
                break;
        }

        throw new InvalidOperationException($"step '{step}' is not defined for {control.GetType().Name}");
    }

    /// The compared surface. Key order matches the Rust probe's.
    private static List<KeyValue> State(Control control) => control switch
    {
        ScrollBar sb =>
        [
            new("minimum", Num(sb.Minimum)),
            new("maximum", Num(sb.Maximum)),
            new("value", Num(sb.Value)),
            new("smallChange", Num(sb.SmallChange)),
            new("largeChange", Num(sb.LargeChange)),
        ],
        TrackBar tb =>
        [
            new("minimum", Num(tb.Minimum)),
            new("maximum", Num(tb.Maximum)),
            new("value", Num(tb.Value)),
            new("smallChange", Num(tb.SmallChange)),
            new("largeChange", Num(tb.LargeChange)),
            new("tickFrequency", Num(tb.TickFrequency)),
        ],
        NumericUpDown nud =>
        [
            new("minimum", Num(nud.Minimum)),
            new("maximum", Num(nud.Maximum)),
            new("value", Num(nud.Value)),
            new("increment", Num(nud.Increment)),
            new("decimalPlaces", Num(nud.DecimalPlaces)),
            new("hexadecimal", Flag(nud.Hexadecimal)),
            new("thousandsSeparator", Flag(nud.ThousandsSeparator)),
            new("text", nud.Text),
        ],
        DomainUpDown dud =>
        [
            new("items", string.Join("|", dud.Items.Cast<object>().Select(o => o.ToString()))),
            new("selectedIndex", Num(dud.SelectedIndex)),
            new("sorted", Flag(dud.Sorted)),
            new("wrap", Flag(dud.Wrap)),
            new("text", dud.Text),
        ],
        _ => [],
    };

    /// Toolkit-only observations. NOT compared — the port exposes no equivalent —
    /// but printed by the comparer, because they explain why a number is what it
    /// is.
    private static List<KeyValue> Extra(Control control)
    {
        var extra = new List<KeyValue>();
        switch (control)
        {
            case ScrollBar sb:
            {
                // What the NATIVE bar was configured with. `nMax - nPage + 1` is
                // the Win32 definition of the highest reachable position, so this
                // is the trap measured at its source rather than restated.
                var si = new SCROLLINFO { cbSize = (uint)Marshal.SizeOf<SCROLLINFO>(), fMask = SIF_ALL };
                if (GetScrollInfo(sb.Handle, SB_CTL, ref si))
                {
                    extra.Add(new KeyValue("nMin", Num(si.nMin)));
                    extra.Add(new KeyValue("nMax", Num(si.nMax)));
                    extra.Add(new KeyValue("nPage", Num((int)si.nPage)));
                    extra.Add(new KeyValue("nPos", Num(si.nPos)));
                }
                break;
            }
            case TrackBar tb:
                // TBM_GETNUMTICS: how many tick marks the native slider drew for
                // the current TickFrequency. The toolkit exposes no managed
                // property for it, which is why it lives here and not in `state`.
                extra.Add(new KeyValue("numTics", Num((int)SendMessage(tb.Handle, TBM_GETNUMTICS, IntPtr.Zero, IntPtr.Zero))));
                break;
        }
        return extra;
    }

    private static int Int(string s) => int.Parse(s, CultureInfo.InvariantCulture);

    private static decimal Dec(string s) => decimal.Parse(s, NumberStyles.Float, CultureInfo.InvariantCulture);

    private static bool Flag(string s) => s == "true";

    private static string Flag(bool b) => b ? "true" : "false";

    private static string Num(int v) => v.ToString(CultureInfo.InvariantCulture);

    /// Canonical decimal rendering, the twin of the Rust probe's `num`: the scale
    /// is normalised away (a `decimal` remembers that `3,50` has two places, an
    /// `f64` cannot) so `3.5` never differs from `3.50` for a spelling reason.
    /// Everything else — including the digits an f64 cannot represent — is left
    /// exactly as the type produced it.
    private static string Num(decimal v) => (v / 1.000000000000000000000000000000000m).ToString(CultureInfo.InvariantCulture);
}

internal sealed record Case(string Id, string Kind, List<string> Steps);

internal sealed record StepResult(string Op, string Status, string Detail);

internal sealed record KeyValue(string Key, string Value);

internal sealed record CaseResult(
    string Id,
    string Kind,
    List<StepResult> Steps,
    List<KeyValue> State,
    List<KeyValue> Extra);

internal static class Cases
{
    /// Parses the shared case file. The grammar is intentionally tiny — `id |
    /// kind | step; step` — so the Rust probe's parser can be its twin.
    public static List<Case> Load(string path)
    {
        var cases = new List<Case>();
        foreach (string raw in File.ReadAllLines(path))
        {
            string line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#'))
            {
                continue;
            }
            string[] parts = line.Split('|');
            if (parts.Length < 2)
            {
                throw new InvalidOperationException($"malformed case line: {raw}");
            }
            var steps = new List<string>();
            if (parts.Length > 2)
            {
                foreach (string step in parts[2].Split(';'))
                {
                    string s = step.Trim();
                    if (s.Length > 0)
                    {
                        steps.Add(s);
                    }
                }
            }
            cases.Add(new Case(parts[0].Trim(), parts[1].Trim(), steps));
        }
        return cases;
    }
}

/// A minimal JSON renderer, hand-rolled rather than `System.Text.Json` so the
/// byte layout matches the Rust probe's writer exactly — two files that differ
/// only in whitespace are much harder to read side by side.
internal static class Json
{
    public static string Render(List<KeyValue> header, List<CaseResult> cases)
    {
        var b = new StringBuilder();
        b.Append("{\n");
        foreach (KeyValue kv in header)
        {
            b.Append("  ").Append(Quote(kv.Key)).Append(": ").Append(Quote(kv.Value)).Append(",\n");
        }
        b.Append("  \"cases\": [\n");
        for (int i = 0; i < cases.Count; i++)
        {
            CaseResult c = cases[i];
            b.Append("    {\n");
            b.Append("      \"id\": ").Append(Quote(c.Id)).Append(",\n");
            b.Append("      \"kind\": ").Append(Quote(c.Kind)).Append(",\n");
            b.Append("      \"steps\": [");
            for (int s = 0; s < c.Steps.Count; s++)
            {
                StepResult st = c.Steps[s];
                b.Append(s == 0 ? "\n" : ",\n");
                b.Append("        {\"op\": ").Append(Quote(st.Op))
                 .Append(", \"status\": ").Append(Quote(st.Status))
                 .Append(", \"detail\": ").Append(Quote(st.Detail)).Append('}');
            }
            b.Append(c.Steps.Count == 0 ? "],\n" : "\n      ],\n");
            b.Append("      \"state\": ").Append(Map(c.State, 6)).Append(",\n");
            b.Append("      \"extra\": ").Append(Map(c.Extra, 6)).Append('\n');
            b.Append("    }").Append(i == cases.Count - 1 ? "\n" : ",\n");
        }
        b.Append("  ]\n}\n");
        return b.ToString();
    }

    private static string Map(List<KeyValue> pairs, int indent)
    {
        if (pairs.Count == 0)
        {
            return "{}";
        }
        var b = new StringBuilder("{\n");
        for (int i = 0; i < pairs.Count; i++)
        {
            b.Append(new string(' ', indent + 2)).Append(Quote(pairs[i].Key)).Append(": ").Append(Quote(pairs[i].Value));
            b.Append(i == pairs.Count - 1 ? "\n" : ",\n");
        }
        return b.Append(new string(' ', indent)).Append('}').ToString();
    }

    /// Escapes to pure ASCII: fr-FR's group separator is U+202F, and a literal
    /// narrow no-break space in a file read by three different tools is a bug
    /// waiting to happen.
    private static string Quote(string s)
    {
        var b = new StringBuilder("\"");
        foreach (char ch in s ?? "")
        {
            switch (ch)
            {
                case '"': b.Append("\\\""); break;
                case '\\': b.Append("\\\\"); break;
                case '\n': b.Append("\\n"); break;
                case '\r': b.Append("\\r"); break;
                case '\t': b.Append("\\t"); break;
                default:
                    if (ch < 0x20 || ch > 0x7E)
                    {
                        b.Append("\\u").Append(((int)ch).ToString("x4", CultureInfo.InvariantCulture));
                    }
                    else
                    {
                        b.Append(ch);
                    }
                    break;
            }
        }
        return b.Append('"').ToString();
    }
}
