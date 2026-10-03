// The WinForms side of the layout parity harness.
//
// It builds every case from the SHARED case list (`../cases.json`) out of real
// `System.Windows.Forms` controls, forces a layout, and writes the resulting
// geometry to `layout-winforms.json`. The Rust probe reads the same case file
// and writes the same shape, so `compare-layout.ps1` can diff numbers rather
// than pixels.
//
// Why numbers: the port paints in the Kubuno design system, so a pixel diff of
// APPEARANCE would only re-measure a difference we already know about. What has
// to match exactly is the GEOMETRY — given the same container and the same
// children, every child rectangle must come out with the same numbers.
//
// Three details are load-bearing:
//
//  1. The process is DpiUnaware (see the .csproj). Both probes then work in the
//     same unscaled space.
//  2. The container is hosted on a REALISED form. .NET 8 introduced a second
//     anchor implementation that defers computing anchor offsets until the
//     handle exists; without a handle the anchors of a resized container are
//     simply never applied, and the harness would report a phantom match. The
//     switch's state is recorded in the file's `meta` so a future reader knows
//     which implementation produced the numbers.
//  3. Properties are set in a fixed order — MinimumSize/MaximumSize, then
//     Bounds, then Dock/Anchor. WinForms' MinimumSize setter clamps the bounds
//     on the spot, so the reverse order would feed the two probes different
//     inputs. The Rust probe uses the same order.

using System.Drawing;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Windows.Forms;

namespace LayoutWinForms;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        Application.EnableVisualStyles();
        // An exception raised inside a control's `WndProc` does not surface at
        // the call that provoked it: WinForms routes it to
        // `Application.OnThreadException`, which shows a modal dialog and waits
        // — which hangs an unattended probe. `ThrowException` makes it
        // propagate so the caller can record it. Must precede the first window.
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException);

        string caseFile = args.Length > 0 ? args[0] : DefaultCaseFile();
        string outFile = args.Length > 1 ? args[1] : DefaultOutFile();

        if (!File.Exists(caseFile))
        {
            Console.Error.WriteLine($"case list not found: {caseFile}");
            return 2;
        }

        JsonNode root = JsonNode.Parse(File.ReadAllText(caseFile))
                        ?? throw new InvalidDataException("empty case list");
        JsonArray cases = root["cases"].AsArray();

        // A real, realised, off-screen host. Nothing is ever painted; the window
        // exists only so the children have a handle-backed container.
        using var form = new Form
        {
            AutoScaleMode = AutoScaleMode.None,
            FormBorderStyle = FormBorderStyle.None,
            ShowInTaskbar = false,
            StartPosition = FormStartPosition.Manual,
            Location = new Point(-4000, -4000),
            ClientSize = new Size(16, 16),
        };
        form.Show();
        _ = form.Handle;   // force handle creation even if Show() were elided

        var results = new JsonArray();
        foreach (JsonNode c in cases)
        {
            results.Add(RunCase(form, c));
        }

        AppContext.TryGetSwitch("System.Windows.Forms.AnchorLayoutV2", out bool anchorV2);
        var doc = new JsonObject
        {
            ["meta"] = new JsonObject
            {
                ["probe"] = "winforms",
                ["framework"] = Environment.Version.ToString(),
                ["deviceDpi"] = form.DeviceDpi,
                ["anchorLayoutV2"] = anchorV2,
                ["caseFile"] = Path.GetFullPath(caseFile),
            },
            ["cases"] = results,
        };

        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(outFile))!);
        File.WriteAllText(outFile, doc.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        Console.WriteLine($"{results.Count} cases -> {Path.GetFullPath(outFile)}");
        form.Close();
        return 0;
    }

    /// <summary>Builds one case, lays it out, and returns its recorded geometry.</summary>
    private static JsonObject RunCase(Form host, JsonNode spec)
    {
        // The container is a Panel, not the Form: a Panel's DisplayRectangle is
        // its client rectangle deflated by Padding and nothing else, which is
        // exactly the input the port's `local_display_rect` produces. A Form
        // would add auto-scaling and chrome the port does not model, and the
        // harness would then be measuring the host, not the layout.
        var container = new Panel
        {
            Name = "container",
            BorderStyle = BorderStyle.None,
            AutoScroll = false,
            AutoSize = false,
            Margin = Padding.Empty,
            Padding = ReadPadding(spec["padding"], Padding.Empty),
            Location = Point.Empty,
        };

        // The container is sized BEFORE its children are added, and that is not
        // cosmetic. Classic DefaultLayout captures a child's anchor offsets when
        // the child joins the collection (`InitLayout`), relative to the
        // container's display rect at that instant. Adding first and sizing
        // afterwards would silently baseline every anchored child against
        // `Panel`'s 200x100 default instead of the case's declared start size,
        // and the harness would then report a construction artefact as a port
        // defect. The port's baseline is the display rect of its first layout
        // pass, which this order makes the same rectangle.
        container.Size = ReadSize(spec["size"]);
        host.Controls.Add(container);

        container.SuspendLayout();
        foreach (JsonNode childSpec in spec["children"].AsArray())
        {
            container.Controls.Add(BuildChild(childSpec));
        }
        container.ResumeLayout(performLayout: true);
        container.PerformLayout();

        JsonNode resize = spec["resize"];
        if (resize is not null)
        {
            container.Size = ReadSize(resize);
            container.PerformLayout();
        }

        var children = new JsonArray();
        Emit(container, "", children);

        var result = new JsonObject
        {
            ["id"] = (string)spec["id"],
            ["display"] = RectNode(container.DisplayRectangle),
            ["children"] = children,
        };

        host.Controls.Remove(container);
        container.Dispose();
        return result;
    }

    /// <summary>
    /// Builds one child (and, recursively, its own children). Every node is a
    /// Panel so that a leaf and a container differ only in whether they have
    /// children — no control-specific preferred size can leak into the numbers.
    /// </summary>
    private static Panel BuildChild(JsonNode spec)
    {
        var p = new Panel
        {
            Name = (string)spec["name"],
            BorderStyle = BorderStyle.None,
            AutoScroll = false,
            AutoSize = false,
        };

        p.Margin = ReadPadding(spec["margin"], new Padding(3));
        p.Padding = ReadPadding(spec["padding"], Padding.Empty);

        // Order matters: MinimumSize/MaximumSize clamp Bounds as they are set.
        p.MinimumSize = ReadSize(spec["min"], Size.Empty);
        p.MaximumSize = ReadSize(spec["max"], Size.Empty);

        int[] r = ReadInts(spec["rect"], 4);
        p.Bounds = new Rectangle(r[0], r[1], r[2], r[3]);

        p.Dock = ReadDock(spec["dock"]);
        p.Anchor = ReadAnchor(spec["anchor"]);

        JsonNode kids = spec["children"];
        if (kids is not null)
        {
            p.SuspendLayout();
            foreach (JsonNode k in kids.AsArray())
            {
                p.Controls.Add(BuildChild(k));
            }
            // `true`, not `false`: a nested container whose design size already
            // matches the size its parent's dock pass will give it never changes
            // size, so no later layout is ever triggered for it and its own
            // children would keep their raw design bounds. The port lays every
            // level out on its first pass; this makes WinForms do the same.
            p.ResumeLayout(performLayout: true);
        }

        // `Visible` last: hiding a control before its siblings exist changes
        // nothing, but hiding it after keeps the construction order uniform.
        JsonNode visible = spec["visible"];
        if (visible is not null && !(bool)visible)
        {
            p.Visible = false;
        }

        return p;
    }

    /// <summary>
    /// Walks the tree in collection order — which is add order, and therefore
    /// the same index a child has in the port's `Vec` — and records each
    /// child's Bounds plus, for a container, its DisplayRectangle.
    /// </summary>
    private static void Emit(Control parent, string prefix, JsonArray sink)
    {
        for (int i = 0; i < parent.Controls.Count; i++)
        {
            Control c = parent.Controls[i];
            string path = prefix.Length == 0 ? i.ToString() : $"{prefix}.{i}";
            var node = new JsonObject
            {
                ["path"] = path,
                ["name"] = c.Name,
                ["bounds"] = RectNode(c.Bounds),
            };
            if (c.Controls.Count > 0)
            {
                node["display"] = RectNode(c.DisplayRectangle);
            }
            sink.Add(node);
            Emit(c, path, sink);
        }
    }

    // ── Reading the shared case file ─────────────────────────────────────

    private static int[] ReadInts(JsonNode n, int count)
    {
        JsonArray a = n.AsArray();
        if (a.Count != count)
        {
            throw new InvalidDataException($"expected {count} numbers, got {a.Count}");
        }
        var v = new int[count];
        for (int i = 0; i < count; i++)
        {
            v[i] = (int)a[i];
        }
        return v;
    }

    private static Size ReadSize(JsonNode n, Size fallback = default)
    {
        if (n is null)
        {
            return fallback;
        }
        int[] v = ReadInts(n, 2);
        return new Size(v[0], v[1]);
    }

    private static Padding ReadPadding(JsonNode n, Padding fallback)
    {
        if (n is null)
        {
            return fallback;
        }
        int[] v = ReadInts(n, 4);
        return new Padding(v[0], v[1], v[2], v[3]);
    }

    private static DockStyle ReadDock(JsonNode n) =>
        n is null ? DockStyle.None : Enum.Parse<DockStyle>((string)n, ignoreCase: true);

    private static AnchorStyles ReadAnchor(JsonNode n)
    {
        if (n is null)
        {
            return AnchorStyles.Top | AnchorStyles.Left;   // Control's own default
        }
        AnchorStyles a = AnchorStyles.None;
        foreach (string part in ((string)n).Split(',', StringSplitOptions.RemoveEmptyEntries))
        {
            a |= Enum.Parse<AnchorStyles>(part.Trim(), ignoreCase: true);
        }
        return a;
    }

    // ── Writing ──────────────────────────────────────────────────────────

    private static JsonArray RectNode(Rectangle r) =>
        new() { r.Left, r.Top, r.Right, r.Bottom };

    // ── Default paths, resolved from the assembly location ───────────────

    /// <summary>The parity directory: this project's parent.</summary>
    private static string ParityDir()
    {
        // bin/<Config>/<TFM>/ → project → parity
        string dir = AppContext.BaseDirectory;
        for (int i = 0; i < 6 && dir is not null; i++)
        {
            if (File.Exists(Path.Combine(dir, "cases.json")))
            {
                return dir;
            }
            dir = Path.GetDirectoryName(dir.TrimEnd(Path.DirectorySeparatorChar));
        }
        return Directory.GetCurrentDirectory();
    }

    private static string DefaultCaseFile() => Path.Combine(ParityDir(), "cases.json");

    private static string DefaultOutFile() => Path.Combine(ParityDir(), "out", "layout-winforms.json");
}
