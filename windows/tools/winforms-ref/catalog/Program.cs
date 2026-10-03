// Extracts the authoritative WinForms control catalog by reflection:
// every public Control subclass, its inheritance chain, and every designer-
// visible property with its type, default value and category.
//
// This is ground truth for the Kubuno desktop control library — far more
// reliable than prose documentation, because it is the shipping surface itself.

using System.Collections;
using System.ComponentModel;
using System.Reflection;
using System.Text.Json;
using System.Windows.Forms;

var asm = typeof(Control).Assembly;

// Every public, non-abstract-or-abstract type deriving from Control, plus the
// abstract bases (ButtonBase, TextBoxBase, ListControl…) that carry the shared
// property surface — those bases are exactly what must NOT be re-implemented
// per leaf control.
var types = asm.GetTypes()
    .Where(t => t.IsPublic && !t.IsGenericTypeDefinition && typeof(Control).IsAssignableFrom(t))
    .OrderBy(t => t.FullName, StringComparer.Ordinal)
    .ToList();

static string TypeName(Type t)
{
    if (t == null) return null;
    if (Nullable.GetUnderlyingType(t) is Type u) return TypeName(u) + "?";
    if (t.IsGenericType)
    {
        var name = t.Name.Split('`')[0];
        var args = string.Join(", ", t.GetGenericArguments().Select(TypeName));
        return $"{name}<{args}>";
    }
    return t.FullName?.StartsWith("System.Windows.Forms.") == true
        ? t.FullName.Substring("System.Windows.Forms.".Length)
        : t.Name;
}

static object DefaultOf(PropertyInfo p)
{
    var d = p.GetCustomAttribute<DefaultValueAttribute>();
    if (d == null) return null;
    var v = d.Value;
    if (v == null) return null;
    if (v is Enum e) return e.ToString();
    if (v is bool or string or int or long or float or double or decimal) return v;
    return v.ToString();
}

var records = new List<object>();

foreach (var t in types)
{
    // The inheritance chain up to Control — the reuse map.
    var chain = new List<string>();
    for (var b = t.BaseType; b != null && typeof(Control).IsAssignableFrom(b); b = b.BaseType)
        chain.Add(TypeName(b));

    // Properties DECLARED on this type (not inherited): what a faithful port
    // must add on top of its base. Instance, public, readable.
    var declared = t.GetProperties(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly)
        .Where(p => p.GetIndexParameters().Length == 0 && p.CanRead)
        .Where(p => p.GetCustomAttribute<BrowsableAttribute>()?.Browsable != false
                    || p.GetCustomAttribute<CategoryAttribute>() != null)
        .OrderBy(p => p.Name, StringComparer.Ordinal)
        .Select(p => (object)new
        {
            name = p.Name,
            type = TypeName(p.PropertyType),
            settable = p.CanWrite && (p.SetMethod?.IsPublic ?? false),
            category = p.GetCustomAttribute<CategoryAttribute>()?.Category,
            @default = DefaultOf(p),
            description = p.GetCustomAttribute<DescriptionAttribute>()?.Description,
            localizable = p.GetCustomAttribute<LocalizableAttribute>()?.IsLocalizable ?? false,
        })
        .ToList();

    // Events declared here — the behaviour surface.
    var events = t.GetEvents(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly)
        .Select(e => e.Name).OrderBy(n => n, StringComparer.Ordinal).ToList();

    records.Add(new
    {
        name = TypeName(t),
        full = t.FullName,
        chain,
        isAbstract = t.IsAbstract,
        declaredProperties = declared,
        declaredEvents = events,
        declaredPropertyCount = declared.Count,
    });
}

var opts = new JsonSerializerOptions { WriteIndented = true };
var outDir = args.Length > 0 ? args[0] : ".";
Directory.CreateDirectory(outDir);
File.WriteAllText(Path.Combine(outDir, "winforms-catalog.json"), JsonSerializer.Serialize(records, opts));

// A compact hierarchy map, easy to read at a glance.
using (var w = new StreamWriter(Path.Combine(outDir, "winforms-hierarchy.txt")))
{
    var byBase = types.ToLookup(t => t.BaseType);
    void Dump(Type t, int depth)
    {
        var flag = t.IsAbstract ? " (abstract)" : "";
        var count = t.GetProperties(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly).Length;
        w.WriteLine($"{new string(' ', depth * 2)}{TypeName(t)}{flag}  [+{count} props]");
        foreach (var c in byBase[t].OrderBy(x => x.Name, StringComparer.Ordinal)) Dump(c, depth + 1);
    }
    Dump(typeof(Control), 0);
}

Console.WriteLine($"{records.Count} control types written to {Path.GetFullPath(outDir)}");
