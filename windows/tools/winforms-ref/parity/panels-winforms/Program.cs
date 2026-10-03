// Numeric parity probe — the WinForms side.
//
// Builds every case declared in `panels-cases.txt` with REAL controls, realises
// the form so the native tab control has a handle, forces the layout, and writes
// the resolved geometry to `panels-winforms.json`.
//
// What is deliberately NOT done here: nothing is rounded, adjusted or
// "corrected" to look like the port. The file is a measurement.
//
//   dotnet run --project panels-winforms

using System.Globalization;
using System.Text;
using System.Windows.Forms;
using System.Drawing;

namespace PanelsWinForms;

// ── The shared case model (mirrors panels-cases.txt one field per token) ─────

sealed class ChildSpec
{
    public float W, H, ML, MT, MR, MB;
    public bool Break;
    // Table-only.
    public int Col = -1, Row = -1, CSpan = 1, RSpan = 1;
    public bool Fill = true;
}

sealed class TrackSpec
{
    public string Type = "auto";
    public float Value;
}

sealed class Case
{
    public string Id = "", Kind = "";
    public float BoxW = 100, BoxH = 100, PadL, PadT, PadR, PadB;
    public string Border = "none";

    // flow
    public string FlowDir = "ltr";
    public bool Wrap = true;

    // table
    public int Cols, Rows;
    public string CellBorder = "none", Grow = "addrows";
    public readonly List<TrackSpec> ColStyles = new();
    public readonly List<TrackSpec> RowStyles = new();

    // split
    public string Orient = "vertical", Fixed = "none";
    public float Dist = 50, SplitW = 4, Min1 = 25, Min2 = 25;
    public bool Collapse1, Collapse2;
    public bool HasResize;
    public float ResizeW, ResizeH;

    // tab
    public string Align = "top", SizeMode = "normal";
    public bool Multiline;
    public float ItemW, ItemH, PadX = 6, PadY = 3;
    public readonly List<string> Pages = new();

    public readonly List<ChildSpec> Children = new();
}

static class Program
{
    static readonly CultureInfo Inv = CultureInfo.InvariantCulture;

    [STAThread]
    static int Main(string[] args)
    {
        // Visual styles change the native tab control's own metrics, so the probe
        // runs the way a real application does rather than in classic mode.
        Application.EnableVisualStyles();
        // No modal "unhandled exception" dialog may ever appear: this probe runs
        // unattended in a build, and such a dialog would hang it forever.
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException);

        string casesPath = args.Length > 0 ? args[0] : FindCases();
        if (casesPath is null || !File.Exists(casesPath))
        {
            Console.Error.WriteLine("panels-cases.txt not found (pass its path as the first argument)");
            return 2;
        }
        string outPath = args.Length > 1
            ? args[1]
            : Path.Combine(Path.GetDirectoryName(Path.GetFullPath(casesPath))!, "panels-winforms.json");

        var cases = Parse(File.ReadAllLines(casesPath));
        var sb = new StringBuilder();
        sb.Append("{\n  \"probe\": \"winforms\",\n");
        sb.Append("  \"cases\": [\n");
        for (int i = 0; i < cases.Count; i++)
        {
            var values = Measure(cases[i]);
            sb.Append("    {\"id\": \"").Append(cases[i].Id).Append("\", \"kind\": \"").Append(cases[i].Kind)
              .Append("\", \"values\": {");
            bool first = true;
            foreach (var kv in values)
            {
                if (!first) sb.Append(", ");
                first = false;
                sb.Append('"').Append(kv.Key).Append("\": [");
                for (int k = 0; k < kv.Value.Length; k++)
                {
                    if (k > 0) sb.Append(", ");
                    sb.Append(kv.Value[k].ToString("0.####", Inv));
                }
                sb.Append(']');
            }
            sb.Append("}}");
            if (i < cases.Count - 1) sb.Append(',');
            sb.Append('\n');
        }
        sb.Append("  ]\n}\n");
        File.WriteAllText(outPath, sb.ToString());
        Console.WriteLine($"{cases.Count} cases -> {outPath}");
        return 0;
    }

    /// Walks up from the executable looking for the shared case file, so the
    /// probe runs from anywhere without a hard-coded path.
    static string FindCases()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null)
        {
            string here = Path.Combine(dir.FullName, "panels-cases.txt");
            if (File.Exists(here)) return here;
            string sub = Path.Combine(dir.FullName, "parity", "panels-cases.txt");
            if (File.Exists(sub)) return sub;
            dir = dir.Parent;
        }
        return null;
    }

    // ── Parsing ─────────────────────────────────────────────────────────────

    static float F(string s) => float.Parse(s, Inv);
    static bool B(string s) => s.Equals("true", StringComparison.OrdinalIgnoreCase);

    static List<Case> Parse(string[] lines)
    {
        var cases = new List<Case>();
        Case cur = null;
        foreach (var raw in lines)
        {
            string line = raw;
            int hash = line.IndexOf('#');
            if (hash >= 0) line = line.Substring(0, hash);
            var t = line.Split((char[])null, StringSplitOptions.RemoveEmptyEntries);
            if (t.Length == 0) continue;

            switch (t[0])
            {
                case "case":
                    cur = new Case { Id = t[1], Kind = t[2] };
                    cases.Add(cur);
                    break;
                case "box":
                    cur.BoxW = F(t[1]); cur.BoxH = F(t[2]);
                    cur.PadL = F(t[3]); cur.PadT = F(t[4]); cur.PadR = F(t[5]); cur.PadB = F(t[6]);
                    cur.Border = t[7];
                    break;
                case "flow":
                    cur.FlowDir = t[1]; cur.Wrap = B(t[2]);
                    break;
                case "child":
                    cur.Children.Add(new ChildSpec
                    {
                        W = F(t[1]), H = F(t[2]),
                        ML = F(t[3]), MT = F(t[4]), MR = F(t[5]), MB = F(t[6]),
                        Break = B(t[7]),
                    });
                    break;
                case "grid":
                    cur.Cols = (int)F(t[1]); cur.Rows = (int)F(t[2]);
                    cur.CellBorder = t[3]; cur.Grow = t[4];
                    break;
                case "col":
                    cur.ColStyles.Add(new TrackSpec { Type = t[1], Value = F(t[2]) });
                    break;
                case "row":
                    cur.RowStyles.Add(new TrackSpec { Type = t[1], Value = F(t[2]) });
                    break;
                case "cell":
                    cur.Children.Add(new ChildSpec
                    {
                        W = F(t[1]), H = F(t[2]),
                        ML = F(t[3]), MT = F(t[4]), MR = F(t[5]), MB = F(t[6]),
                        Col = (int)F(t[7]), Row = (int)F(t[8]),
                        CSpan = (int)F(t[9]), RSpan = (int)F(t[10]),
                        Fill = t[11] == "fill",
                    });
                    break;
                case "split":
                    cur.Orient = t[1]; cur.Fixed = t[2];
                    cur.Dist = F(t[3]); cur.SplitW = F(t[4]);
                    cur.Min1 = F(t[5]); cur.Min2 = F(t[6]);
                    cur.Collapse1 = B(t[7]); cur.Collapse2 = B(t[8]);
                    break;
                case "resize":
                    cur.HasResize = true; cur.ResizeW = F(t[1]); cur.ResizeH = F(t[2]);
                    break;
                case "tabs":
                    cur.Align = t[1]; cur.Multiline = B(t[2]); cur.SizeMode = t[3];
                    cur.ItemW = F(t[4]); cur.ItemH = F(t[5]); cur.PadX = F(t[6]); cur.PadY = F(t[7]);
                    break;
                case "page":
                    cur.Pages.Add(t[1]);
                    break;
                default:
                    throw new InvalidDataException($"unknown record '{t[0]}'");
            }
        }
        return cases;
    }

    // ── Measuring ───────────────────────────────────────────────────────────

    static BorderStyle BorderOf(string s) => s switch
    {
        "single" => BorderStyle.FixedSingle,
        "fixed3d" => BorderStyle.Fixed3D,
        _ => BorderStyle.None,
    };

    static double[] R(Rectangle r) => new double[] { r.X, r.Y, r.Width, r.Height };

    /// Realises `root` inside an off-screen form and runs `read` against the
    /// live control. A handle is required: TabControl.DisplayRectangle asks the
    /// native control (TCM_ADJUSTRECT), and returns a wrong answer without one.
    static void Realise(Control root, Action<Form> read)
    {
        using var form = new Form
        {
            AutoScaleMode = AutoScaleMode.None,   // no font/DPI rescaling of our numbers
            FormBorderStyle = FormBorderStyle.None,
            StartPosition = FormStartPosition.Manual,
            Location = new Point(-4000, -4000),
            ShowInTaskbar = false,
            ClientSize = new Size(1200, 800),
        };
        form.Controls.Add(root);
        form.Show();
        form.PerformLayout();
        Application.DoEvents();
        read(form);
        form.Hide();
    }

    static List<KeyValuePair<string, double[]>> Measure(Case c) => c.Kind switch
    {
        "flow" => MeasureFlow(c),
        "table" => MeasureTable(c),
        "split" => MeasureSplit(c),
        "tab" => MeasureTab(c),
        _ => throw new InvalidDataException($"unknown kind '{c.Kind}'"),
    };

    static Panel Leaf(ChildSpec s) => new()
    {
        AutoSize = false,
        Size = new Size((int)s.W, (int)s.H),
        Margin = new Padding((int)s.ML, (int)s.MT, (int)s.MR, (int)s.MB),
        MinimumSize = Size.Empty,
    };

    static List<KeyValuePair<string, double[]>> MeasureFlow(Case c)
    {
        var v = new List<KeyValuePair<string, double[]>>();
        var p = new FlowLayoutPanel
        {
            Location = new Point(0, 0),
            Size = new Size((int)c.BoxW, (int)c.BoxH),
            Padding = new Padding((int)c.PadL, (int)c.PadT, (int)c.PadR, (int)c.PadB),
            BorderStyle = BorderOf(c.Border),
            AutoSize = false,
            AutoScroll = false,
            WrapContents = c.Wrap,
            FlowDirection = c.FlowDir switch
            {
                "td" => FlowDirection.TopDown,
                "rtl" => FlowDirection.RightToLeft,
                "bu" => FlowDirection.BottomUp,
                _ => FlowDirection.LeftToRight,
            },
        };
        var kids = new List<Panel>();
        foreach (var s in c.Children)
        {
            var k = Leaf(s);
            p.Controls.Add(k);
            // SetFlowBreak is an extender property of the panel, so it is set
            // after the child joins the collection.
            p.SetFlowBreak(k, s.Break);
            kids.Add(k);
        }

        Realise(p, _ =>
        {
            // MEASURED, not assumed: a ScrollableControl's DisplayRectangle is
            // ALREADY deflated by Padding (the first run of this probe deflated
            // it a second time and reported a 10 DIP phantom offset). It is the
            // area children are laid out in, so it is emitted raw.
            v.Add(new("display", R(p.DisplayRectangle)));
            v.Add(new("clientSize", new double[] { p.ClientSize.Width, p.ClientSize.Height }));
            for (int i = 0; i < kids.Count; i++) v.Add(new($"child{i}", R(kids[i].Bounds)));
        });
        return v;
    }

    static List<KeyValuePair<string, double[]>> MeasureTable(Case c)
    {
        var v = new List<KeyValuePair<string, double[]>>();
        var p = new TableLayoutPanel
        {
            Location = new Point(0, 0),
            Size = new Size((int)c.BoxW, (int)c.BoxH),
            Padding = new Padding((int)c.PadL, (int)c.PadT, (int)c.PadR, (int)c.PadB),
            BorderStyle = BorderOf(c.Border),
            AutoSize = false,
            ColumnCount = c.Cols,
            RowCount = c.Rows,
            GrowStyle = c.Grow switch
            {
                "fixed" => TableLayoutPanelGrowStyle.FixedSize,
                "addcolumns" => TableLayoutPanelGrowStyle.AddColumns,
                _ => TableLayoutPanelGrowStyle.AddRows,
            },
            CellBorderStyle = c.CellBorder switch
            {
                "single" => TableLayoutPanelCellBorderStyle.Single,
                "inset" => TableLayoutPanelCellBorderStyle.Inset,
                "insetdouble" => TableLayoutPanelCellBorderStyle.InsetDouble,
                "outset" => TableLayoutPanelCellBorderStyle.Outset,
                "outsetdouble" => TableLayoutPanelCellBorderStyle.OutsetDouble,
                "outsetpartial" => TableLayoutPanelCellBorderStyle.OutsetPartial,
                _ => TableLayoutPanelCellBorderStyle.None,
            },
        };
        foreach (var s in c.ColStyles) p.ColumnStyles.Add(new ColumnStyle(TypeOf(s.Type), s.Value));
        foreach (var s in c.RowStyles) p.RowStyles.Add(new RowStyle(TypeOf(s.Type), s.Value));

        var kids = new List<Panel>();
        double overflowThrew = 0;
        foreach (var s in c.Children)
        {
            var k = Leaf(s);
            k.Dock = s.Fill ? DockStyle.Fill : DockStyle.None;
            try
            {
                p.Controls.Add(k);
            }
            catch (ArgumentException)
            {
                // MEASURED: a saturated GrowStyle = FixedSize table THROWS on the
                // child that no longer fits ("TableLayoutPanel is full"). It does
                // not silently drop it, and it does not park it in a phantom cell.
                // The half-added child is taken back out, or every later layout
                // pass would throw again.
                overflowThrew = 1;
                if (p.Controls.Contains(k)) p.Controls.Remove(k);
                k.Dispose();
                break;
            }
            if (s.Col >= 0 && s.Row >= 0)
            {
                p.SetCellPosition(k, new TableLayoutPanelCellPosition(s.Col, s.Row));
            }
            if (s.CSpan > 1) p.SetColumnSpan(k, s.CSpan);
            if (s.RSpan > 1) p.SetRowSpan(k, s.RSpan);
            kids.Add(k);
        }

        Realise(p, _ =>
        {
            v.Add(new("display", R(p.DisplayRectangle)));
            v.Add(new("overflowThrew", new double[] { overflowThrew }));
            v.Add(new("columnWidths", Array.ConvertAll(p.GetColumnWidths(), x => (double)x)));
            v.Add(new("rowHeights", Array.ConvertAll(p.GetRowHeights(), x => (double)x)));
            for (int i = 0; i < kids.Count; i++)
            {
                v.Add(new($"child{i}", R(kids[i].Bounds)));
                var pos = p.GetPositionFromControl(kids[i]);
                v.Add(new($"cell{i}", new double[] { pos.Column, pos.Row }));
            }
        });
        return v;
    }

    static SizeType TypeOf(string s) => s switch
    {
        "abs" => SizeType.Absolute,
        "pct" => SizeType.Percent,
        _ => SizeType.AutoSize,
    };

    static List<KeyValuePair<string, double[]>> MeasureSplit(Case c)
    {
        var v = new List<KeyValuePair<string, double[]>>();
        var sc = new SplitContainer
        {
            Location = new Point(0, 0),
            Size = new Size((int)c.BoxW, (int)c.BoxH),
            BorderStyle = BorderOf(c.Border),
        };
        // ORDER MATTERS. Orientation decides which extent the distance is
        // measured along, SplitterWidth and the minimums decide the legal range
        // the setter validates against — so all four precede SplitterDistance.
        sc.Orientation = c.Orient == "horizontal" ? Orientation.Horizontal : Orientation.Vertical;
        sc.SplitterWidth = (int)c.SplitW;
        sc.FixedPanel = c.Fixed switch
        {
            "panel1" => FixedPanel.Panel1,
            "panel2" => FixedPanel.Panel2,
            _ => FixedPanel.None,
        };
        int requested = (int)c.Dist;
        double threw = 0;
        try
        {
            sc.Panel1MinSize = (int)c.Min1;
            sc.Panel2MinSize = (int)c.Min2;
            sc.SplitterDistance = requested;
        }
        catch (Exception)
        {
            // The toolkit REFUSES an out-of-range distance where the port clamps
            // it. Recorded rather than swallowed: the refusal is the behaviour.
            threw = 1;
        }
        sc.Panel1Collapsed = c.Collapse1;
        sc.Panel2Collapsed = c.Collapse2;

        Realise(sc, _ =>
        {
            v.Add(new("panel1", R(sc.Panel1.Bounds)));
            v.Add(new("splitter", R(sc.SplitterRectangle)));
            v.Add(new("panel2", R(sc.Panel2.Bounds)));
            v.Add(new("distance", new double[] { sc.SplitterDistance }));
            // 1 when the request was not honoured verbatim — by clamping OR by
            // the setter refusing it outright.
            v.Add(new("notHonoured", new double[] { (threw != 0 || sc.SplitterDistance != requested) ? 1 : 0 }));
            v.Add(new("threw", new double[] { threw }));

            if (c.HasResize)
            {
                sc.Size = new Size((int)c.ResizeW, (int)c.ResizeH);
                sc.PerformLayout();
                Application.DoEvents();
                v.Add(new("panel1After", R(sc.Panel1.Bounds)));
                v.Add(new("splitterAfter", R(sc.SplitterRectangle)));
                v.Add(new("panel2After", R(sc.Panel2.Bounds)));
                v.Add(new("distanceAfter", new double[] { sc.SplitterDistance }));
            }
        });
        return v;
    }

    static List<KeyValuePair<string, double[]>> MeasureTab(Case c)
    {
        var v = new List<KeyValuePair<string, double[]>>();
        var tc = new TabControl
        {
            Location = new Point(0, 0),
            Size = new Size((int)c.BoxW, (int)c.BoxH),
        };
        // Alignment first: Left/Right FORCE Multiline on, so setting Multiline
        // before it would be silently overridden.
        tc.Alignment = c.Align switch
        {
            "bottom" => TabAlignment.Bottom,
            "left" => TabAlignment.Left,
            "right" => TabAlignment.Right,
            _ => TabAlignment.Top,
        };
        tc.Multiline = c.Multiline;
        tc.SizeMode = c.SizeMode switch
        {
            "filltoright" => TabSizeMode.FillToRight,
            "fixed" => TabSizeMode.Fixed,
            _ => TabSizeMode.Normal,
        };
        tc.Padding = new Point((int)c.PadX, (int)c.PadY);
        if (c.ItemW > 0 || c.ItemH > 0) tc.ItemSize = new Size((int)c.ItemW, (int)c.ItemH);
        foreach (var caption in c.Pages) tc.TabPages.Add(new TabPage(caption));

        Realise(tc, _ =>
        {
            var d = tc.DisplayRectangle;
            v.Add(new("display", R(d)));
            v.Add(new("itemSize", new double[] { tc.ItemSize.Width, tc.ItemSize.Height }));
            v.Add(new("rowCount", new double[] { tc.RowCount }));
            if (tc.TabPages.Count > 0) v.Add(new("page0", R(tc.TabPages[0].Bounds)));
            for (int i = 0; i < tc.TabCount; i++) v.Add(new($"tab{i}", R(tc.GetTabRect(i))));
        });
        return v;
    }
}
