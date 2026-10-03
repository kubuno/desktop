// The WinForms reference gallery: real controls, rendered by the real toolkit,
// grouped by family, each family screenshotted to a PNG.
//
// This is the visual ground truth the Kubuno control library is compared
// against — the same controls, the same states (normal / hover-ish / disabled /
// checked / read-only), at a known DPI.
//
//   gallery.exe <outDir> [family...]      // default: every family
//
// Each family is laid out on its own form, captured with the form's own
// DrawToBitmap so the result is exactly what the toolkit painted, then the form
// closes. No window management, no screen scraping.

using System.Drawing;
using System.Windows.Forms;

namespace Gallery;

static class Program
{
    [STAThread]
    static void Main(string[] args)
    {
        ApplicationConfiguration.Initialize();

        var outDir = args.Length > 0 ? args[0] : ".";
        Directory.CreateDirectory(outDir);
        var wanted = args.Skip(1).ToHashSet(StringComparer.OrdinalIgnoreCase);

        foreach (var (name, build) in Families.All)
        {
            if (wanted.Count > 0 && !wanted.Contains(name)) continue;
            // One family failing must not cost the others their reference shot.
            try { Capture(name, build, outDir); }
            catch (Exception e) { Console.WriteLine($"FAILED {name}: {e.Message}"); }
        }
    }

    /// Depth-first re-layout: children first, so an auto-sizing parent measures
    /// against sizes its children have already settled on.
    static void Relayout(Control c)
    {
        foreach (Control k in c.Controls) Relayout(k);
        c.PerformLayout();
    }

    static void Capture(string name, Action<Form> build, string outDir)
    {
        using var f = new Form
        {
            Text = $"WinForms reference — {name}",
            ClientSize = new Size(1500, 1000),
            StartPosition = FormStartPosition.Manual,
            Location = new Point(-3000, -3000),   // off-screen: never steals focus
            // `AutoScaleMode.Dpi` scales against `AutoScaleDimensions`, and an
            // unset (0,0) baseline means it scales by nothing. The sheet then
            // came out INTERNALLY INCONSISTENT: fonts and system metrics at the
            // real 175 %, but every explicit `Width`/`Height` still at 96 dpi —
            // so a 240-wide combo carried 175 % text. Every comparison against
            // such a sheet reads as a port defect when it is a harness one.
            // Naming the design DPI makes the whole sheet scale together.
            AutoScaleDimensions = new SizeF(96F, 96F),
            AutoScaleMode = AutoScaleMode.Dpi,
            BackColor = SystemColors.Control,
        };
        build(f);
        f.Show();
        Application.DoEvents();
        // The first layout runs at 96 dpi; DPI scaling then resizes the controls
        // but not the positions the flow already assigned, so children overlap.
        // Re-running the layout once the handle (and its real DPI) exists puts
        // every auto-sized panel back in agreement with its children.
        Relayout(f);
        Application.DoEvents();
        // Two passes: some controls (ListView, TreeView) paint their content on
        // the second layout once handles exist.
        Relayout(f);
        Application.DoEvents();

        using var bmp = new Bitmap(f.ClientSize.Width, f.ClientSize.Height);
        f.DrawToBitmap(bmp, new Rectangle(0, 0, bmp.Width, bmp.Height));
        var path = Path.Combine(outDir, $"{name}.png");
        bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png);
        Console.WriteLine($"saved {path}");
        f.Close();
    }
}
