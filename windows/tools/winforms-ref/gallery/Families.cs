// One builder per control family, mirroring the reflection hierarchy: each
// family gathers the controls that SHARE a base, because that base is what the
// port implements once (ButtonBase, TextBoxBase, ListControl, ScrollableControl).
//
// Every control is shown in the states that change its painting — default,
// disabled, checked/selected, read-only, and the flag combinations named on the
// control itself (FlatStyle, Appearance, CheckAlign, BorderStyle…). A state that
// the toolkit paints differently is a state the port must reproduce.

using System.Drawing;
using System.Windows.Forms;

namespace Gallery;

static class Families
{
    public static readonly (string Name, Action<Form> Build)[] All =
    {
        ("01-buttonbase",  Buttons),
        ("02-textboxbase", TextBoxes),
        ("03-listcontrol", Lists),
        ("04-containers",  Containers),
        ("05-labels",      Labels),
        ("06-range",       Range),
        ("07-datetime",    DateTime),
        ("08-views",       Views),
        ("09-toolstrip",   ToolStrips),
        ("10-grid",        Grid),
    };

    // A left-to-right, wrapping flow so a family reads as a sheet. Everything
    // auto-sizes: the reference must stay correct at any DPI, and the port is
    // compared against what the toolkit actually laid out, not against a
    // hand-guessed pixel grid.
    static FlowLayoutPanel Sheet(Form f)
    {
        var flow = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoScroll = true,
            Padding = new Padding(12),
            FlowDirection = FlowDirection.LeftToRight,
            WrapContents = true,
        };
        f.Controls.Add(flow);
        return flow;
    }

    /// A labelled group: the caption states exactly which properties are set, so
    /// a screenshot is self-documenting when an agent compares against it. The
    /// group grows to fit its children — no clipped captions at high DPI.
    /// `w`/`h` are kept as MINIMUM sizes only.
    static GroupBox Box(string caption, int w, int h, params Control[] kids)
    {
        var inner = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            FlowDirection = FlowDirection.TopDown,
            WrapContents = false,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            Padding = new Padding(6, 4, 6, 6),
        };
        foreach (var k in kids)
        {
            k.Margin = new Padding(4);
            // Let each control size itself to the DPI-scaled font. Without this
            // the toolkit scales the FONT but keeps the design-time bounds, and
            // the text overflows a box that is too short — which would make the
            // reference sheet lie about the control's real metrics. Controls
            // that ignore AutoSize (ListBox, DataGridView…) keep the explicit
            // size they were given.
            //
            // ScrollBar is the exception that must NOT be auto-sized: its
            // metrics come from the system, and asking it to fit its (absent)
            // content collapses it to a sliver — which would misreport exactly
            // the geometry the port has to reproduce.
            if (k is not ScrollBar) k.AutoSize = true;
            inner.Controls.Add(k);
        }
        var g = new GroupBox
        {
            Text = caption,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            MinimumSize = new Size(w, 0),
            Margin = new Padding(8),
            Padding = new Padding(4, 6, 4, 4),
        };
        g.Controls.Add(inner);
        return g;
    }

    // ── ButtonBase: Button, CheckBox, RadioButton ────────────────────────────
    static void Buttons(Form f)
    {
        var flow = Sheet(f);

        flow.Controls.Add(Box("Button — FlatStyle", 300, 190,
            new Button { Text = "Standard", Width = 120, FlatStyle = FlatStyle.Standard },
            new Button { Text = "Flat", Width = 120, FlatStyle = FlatStyle.Flat },
            new Button { Text = "Popup", Width = 120, FlatStyle = FlatStyle.Popup },
            new Button { Text = "System", Width = 120, FlatStyle = FlatStyle.System }));

        flow.Controls.Add(Box("Button — states", 300, 160,
            new Button { Text = "Disabled", Width = 120, Enabled = false },
            new Button { Text = "Default (accept)", Width = 140 },
            new Button { Text = "AutoSize", AutoSize = true }));

        flow.Controls.Add(Box("CheckBox — Appearance / CheckState", 300, 190,
            new CheckBox { Text = "Unchecked", Width = 160 },
            new CheckBox { Text = "Checked", Checked = true, Width = 160 },
            new CheckBox { Text = "Indeterminate", ThreeState = true, CheckState = CheckState.Indeterminate, Width = 160 },
            new CheckBox { Text = "Appearance=Button", Appearance = Appearance.Button, Width = 160 }));

        flow.Controls.Add(Box("RadioButton", 300, 130,
            new RadioButton { Text = "Option A", Checked = true, Width = 160 },
            new RadioButton { Text = "Option B", Width = 160 },
            new RadioButton { Text = "Disabled", Enabled = false, Width = 160 }));

        flow.Controls.Add(Box("ButtonBase — TextAlign / Image", 300, 130,
            new Button { Text = "TopLeft", TextAlign = ContentAlignment.TopLeft, Width = 140, Height = 40 },
            new Button { Text = "BottomRight", TextAlign = ContentAlignment.BottomRight, Width = 140, Height = 40 }));
    }

    // ── TextBoxBase: TextBox, MaskedTextBox, RichTextBox ─────────────────────
    static void TextBoxes(Form f)
    {
        var flow = Sheet(f);

        flow.Controls.Add(Box("TextBox — BorderStyle", 320, 160,
            new TextBox { Text = "Fixed3D (default)", Width = 260, BorderStyle = BorderStyle.Fixed3D },
            new TextBox { Text = "FixedSingle", Width = 260, BorderStyle = BorderStyle.FixedSingle },
            new TextBox { Text = "None", Width = 260, BorderStyle = BorderStyle.None }));

        flow.Controls.Add(Box("TextBox — states", 320, 190,
            new TextBox { Text = "ReadOnly", Width = 260, ReadOnly = true },
            new TextBox { Text = "Disabled", Width = 260, Enabled = false },
            new TextBox { Text = "Password", Width = 260, UseSystemPasswordChar = true },
            new TextBox { PlaceholderText = "PlaceholderText", Width = 260 }));

        flow.Controls.Add(Box("TextBox — Multiline / ScrollBars", 320, 130,
            new TextBox
            {
                Multiline = true, Width = 260, Height = 80, ScrollBars = ScrollBars.Vertical,
                Text = "Multiline with a vertical scrollbar.\r\nSecond line.\r\nThird line.\r\nFourth line.",
            }));

        flow.Controls.Add(Box("MaskedTextBox", 320, 100,
            new MaskedTextBox { Mask = "00/00/0000", Width = 260 },
            new MaskedTextBox { Mask = "(999) 000-0000", Width = 260 }));

        var rtf = new RichTextBox { Width = 260, Height = 80 };
        rtf.Rtf = @"{\rtf1\ansi {\b Bold} then {\i italic} then {\ul underline}.\par Second paragraph.}";
        flow.Controls.Add(Box("RichTextBox", 320, 120, rtf));
    }

    // ── ListControl: ComboBox, ListBox, CheckedListBox ───────────────────────
    static void Lists(Form f)
    {
        var flow = Sheet(f);
        string[] items = { "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta" };

        var cbDrop = new ComboBox { Width = 240, DropDownStyle = ComboBoxStyle.DropDown };
        cbDrop.Items.AddRange(items); cbDrop.SelectedIndex = 0;
        var cbList = new ComboBox { Width = 240, DropDownStyle = ComboBoxStyle.DropDownList };
        cbList.Items.AddRange(items); cbList.SelectedIndex = 1;
        var cbSimple = new ComboBox { Width = 240, Height = 80, DropDownStyle = ComboBoxStyle.Simple };
        cbSimple.Items.AddRange(items); cbSimple.SelectedIndex = 2;
        flow.Controls.Add(Box("ComboBox — DropDownStyle", 300, 220, cbDrop, cbList, cbSimple));

        var lb = new ListBox { Width = 240, Height = 110 };
        lb.Items.AddRange(items); lb.SelectedIndex = 1;
        var lbMulti = new ListBox { Width = 240, Height = 110, SelectionMode = SelectionMode.MultiExtended };
        lbMulti.Items.AddRange(items);
        lbMulti.SelectedIndices.Add(0); lbMulti.SelectedIndices.Add(2); lbMulti.SelectedIndices.Add(3);
        flow.Controls.Add(Box("ListBox — SelectionMode", 300, 270, lb, lbMulti));

        var clb = new CheckedListBox { Width = 240, Height = 120, CheckOnClick = true };
        clb.Items.AddRange(items);
        clb.SetItemChecked(0, true); clb.SetItemChecked(2, true);
        clb.SetItemCheckState(3, CheckState.Indeterminate);
        flow.Controls.Add(Box("CheckedListBox", 300, 160, clb));
    }

    // ── ScrollableControl / ContainerControl: the container family ───────────
    static void Containers(Form f)
    {
        var flow = Sheet(f);

        var panel = new Panel { Width = 260, Height = 90, BorderStyle = BorderStyle.FixedSingle, AutoScroll = true };
        panel.Controls.Add(new Button { Text = "inside panel", Location = new Point(8, 8), Width = 120 });
        panel.Controls.Add(new Label { Text = "AutoScroll content →", Location = new Point(8, 44), Width = 300 });
        flow.Controls.Add(Box("Panel — BorderStyle / AutoScroll", 300, 140, panel));

        var gb = new GroupBox { Text = "GroupBox caption", Width = 260, Height = 90 };
        gb.Controls.Add(new RadioButton { Text = "grouped A", Location = new Point(10, 24), Checked = true });
        gb.Controls.Add(new RadioButton { Text = "grouped B", Location = new Point(10, 50) });
        flow.Controls.Add(Box("GroupBox", 300, 140, gb));

        var flp = new FlowLayoutPanel { Width = 260, Height = 90, BorderStyle = BorderStyle.FixedSingle, FlowDirection = FlowDirection.LeftToRight, WrapContents = true };
        for (var i = 1; i <= 6; i++) flp.Controls.Add(new Button { Text = $"b{i}", Width = 70 });
        flow.Controls.Add(Box("FlowLayoutPanel — wrap", 300, 140, flp));

        var tlp = new TableLayoutPanel { Width = 260, Height = 90, ColumnCount = 3, RowCount = 2, CellBorderStyle = TableLayoutPanelCellBorderStyle.Single };
        for (var i = 0; i < 6; i++) tlp.Controls.Add(new Label { Text = $"cell {i}", AutoSize = true });
        flow.Controls.Add(Box("TableLayoutPanel — cell borders", 300, 140, tlp));

        var tabs = new TabControl { Width = 260, Height = 100 };
        tabs.TabPages.Add(new TabPage("First") { BackColor = SystemColors.Window });
        tabs.TabPages.Add(new TabPage("Second"));
        tabs.TabPages.Add(new TabPage("Third"));
        tabs.TabPages[0].Controls.Add(new Label { Text = "page content", Location = new Point(10, 10), AutoSize = true });
        flow.Controls.Add(Box("TabControl / TabPage", 300, 150, tabs));

        var split = new SplitContainer { Width = 260, Height = 100, SplitterDistance = 100, BorderStyle = BorderStyle.FixedSingle };
        split.Panel1.Controls.Add(new Label { Text = "Panel1", Location = new Point(6, 6), AutoSize = true });
        split.Panel2.Controls.Add(new Label { Text = "Panel2", Location = new Point(6, 6), AutoSize = true });
        flow.Controls.Add(Box("SplitContainer", 300, 150, split));
    }

    // ── Label, LinkLabel, PictureBox, ProgressBar ────────────────────────────
    static void Labels(Form f)
    {
        var flow = Sheet(f);

        flow.Controls.Add(Box("Label — BorderStyle / align", 300, 170,
            new Label { Text = "AutoSize label", AutoSize = true },
            new Label { Text = "Fixed3D border", BorderStyle = BorderStyle.Fixed3D, Width = 200 },
            new Label { Text = "MiddleCenter", TextAlign = ContentAlignment.MiddleCenter, BorderStyle = BorderStyle.FixedSingle, Width = 200, Height = 40 },
            new Label { Text = "Disabled", Enabled = false, Width = 200 }));

        var ll = new LinkLabel { Text = "Visit the documentation page", Width = 240 };
        ll.LinkVisited = true;
        flow.Controls.Add(Box("LinkLabel — LinkBehavior", 300, 140,
            new LinkLabel { Text = "AlwaysUnderline", Width = 240, LinkBehavior = LinkBehavior.AlwaysUnderline },
            new LinkLabel { Text = "HoverUnderline", Width = 240, LinkBehavior = LinkBehavior.HoverUnderline },
            ll));

        var pb = new PictureBox { Width = 120, Height = 80, BorderStyle = BorderStyle.FixedSingle, SizeMode = PictureBoxSizeMode.Zoom };
        var img = new Bitmap(64, 48);
        using (var g = Graphics.FromImage(img))
        {
            g.Clear(Color.FromArgb(26, 115, 232));
            g.FillEllipse(Brushes.White, 12, 8, 40, 32);
        }
        pb.Image = img;
        flow.Controls.Add(Box("PictureBox — SizeMode=Zoom", 300, 130, pb));

        flow.Controls.Add(Box("ProgressBar — Style", 300, 160,
            new ProgressBar { Width = 240, Value = 45 },
            new ProgressBar { Width = 240, Value = 70, Style = ProgressBarStyle.Continuous },
            new ProgressBar { Width = 240, Style = ProgressBarStyle.Marquee }));
    }

    // ── ScrollBar, TrackBar, UpDownBase ──────────────────────────────────────
    static void Range(Form f)
    {
        var flow = Sheet(f);

        // Explicit sizes: a scrollbar's thickness is a system metric, and the
        // long axis is whatever the host gives it. `LargeChange` is shown too
        // because it decides the thumb's proportion — and because the highest
        // reachable Value is `Maximum - LargeChange + 1`, not `Maximum`.
        // The thickness is a SYSTEM metric, already DPI-correct — the port must
        // read it the same way rather than hard-code 17 px.
        var hThick = SystemInformation.HorizontalScrollBarHeight;
        var vThick = SystemInformation.VerticalScrollBarWidth;
        flow.Controls.Add(Box($"ScrollBar — thumb ∝ LargeChange (thickness {hThick}/{vThick} px)", 300, 130,
            new HScrollBar { Width = 240, Height = hThick, Minimum = 0, Maximum = 100, LargeChange = 20, Value = 30 },
            new HScrollBar { Width = 240, Height = hThick, Minimum = 0, Maximum = 100, LargeChange = 50, Value = 0 },
            new VScrollBar { Width = vThick, Height = 120, Minimum = 0, Maximum = 100, LargeChange = 10, Value = 40 }));

        flow.Controls.Add(Box("TrackBar — TickStyle", 300, 200,
            new TrackBar { Width = 240, Value = 4, TickStyle = TickStyle.BottomRight },
            new TrackBar { Width = 240, Value = 6, TickStyle = TickStyle.Both },
            new TrackBar { Width = 240, Value = 2, TickStyle = TickStyle.None }));

        flow.Controls.Add(Box("NumericUpDown", 300, 170,
            new NumericUpDown { Width = 200, Value = 42 },
            new NumericUpDown { Width = 200, Value = 3.5m, DecimalPlaces = 2, Increment = 0.25m },
            // Maximum BEFORE Value: the setter clamps against the current range
            // and throws outside it — an ordering the port must reproduce.
            new NumericUpDown { Width = 200, Maximum = 4095, Value = 255, Hexadecimal = true },
            new NumericUpDown { Width = 200, Maximum = 100000, Value = 10000, ThousandsSeparator = true }));

        var dud = new DomainUpDown { Width = 200 };
        dud.Items.AddRange(new[] { "Lundi", "Mardi", "Mercredi" });
        dud.SelectedIndex = 1;
        flow.Controls.Add(Box("DomainUpDown", 300, 90, dud));
    }

    // ── DateTimePicker, MonthCalendar ────────────────────────────────────────
    static void DateTime(Form f)
    {
        var flow = Sheet(f);
        var d = new System.DateTime(2026, 6, 15, 14, 30, 0);

        flow.Controls.Add(Box("DateTimePicker — Format", 320, 200,
            new DateTimePicker { Width = 260, Value = d, Format = DateTimePickerFormat.Long },
            new DateTimePicker { Width = 260, Value = d, Format = DateTimePickerFormat.Short },
            new DateTimePicker { Width = 260, Value = d, Format = DateTimePickerFormat.Time, ShowUpDown = true },
            new DateTimePicker { Width = 260, Value = d, Format = DateTimePickerFormat.Custom, CustomFormat = "yyyy-MM-dd HH:mm" }));

        flow.Controls.Add(Box("DateTimePicker — ShowCheckBox", 320, 90,
            new DateTimePicker { Width = 260, Value = d, ShowCheckBox = true, Checked = false }));

        flow.Controls.Add(Box("MonthCalendar", 320, 230,
            new MonthCalendar { SelectionStart = d, ShowToday = true, ShowWeekNumbers = true }));
    }

    // ── TreeView, ListView ───────────────────────────────────────────────────
    static void Views(Form f)
    {
        var flow = Sheet(f);

        var tv = new TreeView { Width = 260, Height = 160, CheckBoxes = false, ShowLines = true, ShowRootLines = true };
        var root = tv.Nodes.Add("Instance");
        var a = root.Nodes.Add("Kubuno");
        a.Nodes.Add("Support N1");
        a.Nodes.Add("Équipe support");
        root.Nodes.Add("Invités");
        tv.ExpandAll();
        tv.SelectedNode = a;
        flow.Controls.Add(Box("TreeView — lines / selection", 300, 200, tv));

        var lv = new ListView { Width = 380, Height = 160, View = View.Details, FullRowSelect = true, GridLines = true };
        lv.Columns.Add("Nom", 140);
        lv.Columns.Add("Rôle", 100);
        lv.Columns.Add("Quota", 100);
        lv.Items.Add(new ListViewItem(new[] { "Admin", "admin", "10 Go" }) { Selected = true });
        lv.Items.Add(new ListViewItem(new[] { "Alice", "user", "5 Go" }));
        lv.Items.Add(new ListViewItem(new[] { "Bob", "user", "5 Go" }));
        flow.Controls.Add(Box("ListView — View=Details", 420, 200, lv));

        var lvIcons = new ListView { Width = 380, Height = 110, View = View.List };
        for (var i = 1; i <= 8; i++) lvIcons.Items.Add($"élément {i}");
        flow.Controls.Add(Box("ListView — View=List", 420, 150, lvIcons));
    }

    // ── ToolStrip family ─────────────────────────────────────────────────────
    static void ToolStrips(Form f)
    {
        var flow = Sheet(f);

        var menu = new MenuStrip { Width = 420, Dock = DockStyle.None };
        var file = new ToolStripMenuItem("Fichier");
        file.DropDownItems.Add(new ToolStripMenuItem("Nouveau"));
        file.DropDownItems.Add(new ToolStripMenuItem("Ouvrir") { ShortcutKeys = Keys.Control | Keys.O });
        file.DropDownItems.Add(new ToolStripSeparator());
        file.DropDownItems.Add(new ToolStripMenuItem("Quitter"));
        menu.Items.Add(file);
        menu.Items.Add(new ToolStripMenuItem("Édition"));
        menu.Items.Add(new ToolStripMenuItem("Aide"));
        flow.Controls.Add(Box("MenuStrip", 460, 80, menu));

        var ts = new ToolStrip { Width = 420, Dock = DockStyle.None, GripStyle = ToolStripGripStyle.Visible };
        ts.Items.Add(new ToolStripButton("Enregistrer"));
        ts.Items.Add(new ToolStripSeparator());
        ts.Items.Add(new ToolStripLabel("Étiquette"));
        var combo = new ToolStripComboBox { Width = 120 };
        combo.Items.AddRange(new[] { "100 %", "125 %", "150 %" });
        combo.SelectedIndex = 0;
        ts.Items.Add(combo);
        ts.Items.Add(new ToolStripButton("Activé") { Checked = true, CheckOnClick = true });
        flow.Controls.Add(Box("ToolStrip — button / separator / combo", 460, 80, ts));

        var status = new StatusStrip { Width = 420, Dock = DockStyle.None, SizingGrip = true };
        status.Items.Add(new ToolStripStatusLabel("Prêt") { Spring = true, TextAlign = ContentAlignment.MiddleLeft });
        status.Items.Add(new ToolStripStatusLabel("26 comptes"));
        status.Items.Add(new ToolStripProgressBar { Value = 40 });
        flow.Controls.Add(Box("StatusStrip", 460, 80, status));
    }

    // ── DataGridView ─────────────────────────────────────────────────────────
    static void Grid(Form f)
    {
        var flow = Sheet(f);

        var dgv = new DataGridView
        {
            Width = 560, Height = 200, AllowUserToAddRows = false,
            RowHeadersVisible = true, SelectionMode = DataGridViewSelectionMode.FullRowSelect,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        };
        dgv.Columns.Add("name", "Nom");
        dgv.Columns.Add("mail", "Adresse");
        var chk = new DataGridViewCheckBoxColumn { HeaderText = "Actif", Name = "actif" };
        dgv.Columns.Add(chk);
        var cmb = new DataGridViewComboBoxColumn { HeaderText = "Rôle", Name = "role" };
        cmb.Items.AddRange("admin", "user", "guest");
        dgv.Columns.Add(cmb);
        dgv.Rows.Add("Admin", "admin@kubuno.local", true, "admin");
        dgv.Rows.Add("Alice", "alice@kubuno.local", true, "user");
        dgv.Rows.Add("Bob", "bob@kubuno.local", false, "guest");
        dgv.Rows[1].Selected = true;
        flow.Controls.Add(Box("DataGridView — text / checkbox / combo columns", 600, 240, dgv));

        var pg = new PropertyGrid { Width = 300, Height = 200, SelectedObject = new Button { Text = "sample" } };
        flow.Controls.Add(Box("PropertyGrid", 340, 240, pg));
    }
}
