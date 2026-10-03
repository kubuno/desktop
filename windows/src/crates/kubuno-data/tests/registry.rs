//! The data components are registered as non-visual components (the designer's component tray)
//! when an application links the crate, and views using them validate and compile.

extern crate kubuno_data as _;

use kubuno_views::registry::{self, ClassKind, Origin};

#[test]
fn data_components_are_non_visual_classes_of_the_registry() {
    for name in kubuno_data::context::ELEMENTS {
        let info = registry::project_info(name).unwrap_or_else(|| panic!("{name} is registered"));
        assert_eq!(info.origin, Origin::Linked);
        assert_eq!(info.kind, ClassKind::Component, "{name}");
        assert_eq!(info.crate_name, Some("kubuno_data"));
        assert!(registry::is_non_visual(name), "{name} goes to the component tray");
    }
    let bs = registry::lookup("BindingSource").expect("meta");
    assert_eq!(bs.default_event(), Some("OnCurrentChanged"));
    assert!(bs.property("Filter").is_some() && bs.property("DataSource").is_some());
    assert!(bs.event("OnRowValidating").is_some_and(|e| e.cancelable));
    let conn = registry::lookup("DbConnection").expect("meta");
    assert!(matches!(conn.property("Provider").map(|p| p.kind), Some(registry::PropKind::Enum(v)) if v.contains(&"Sqlite")));
    assert_eq!(conn.event("OnStateChange").map(|e| e.args_type), Some("StateChangeEventArgs"));
}

#[test]
fn views_with_data_components_validate_and_compile() {
    let view = r#"
        <Panel DesignWidth="600" DesignHeight="400">
          <DbConnection x:Name="db" Provider="Sqlite" ConnectionStringName="Customers" OnStateChange="db_state"/>
          <TableAdapter x:Name="customersAdapter" Connection="db" SelectCommand="SELECT * FROM customers" UpdateTable="customers"/>
          <BindingSource x:Name="customers" DataSource="customersAdapter" Sort="name" OnCurrentChanged="current_changed"/>
          <ErrorProvider x:Name="errors" DataSource="customers"/>
          <DataTable ItemsSource="{Binding Source=customers}" SelectedIndex="{Binding Source=customers, Path=Position, Mode=TwoWay}" X="8" Y="8" Width="400" Height="200">
            <Column Header="Name" Binding="{Binding name}"/>
          </DataTable>
          <TextField Text="{Binding Source=customers, Path=name, Mode=TwoWay}" Invalid="{Binding Source=errors, Path=name.HasError}" X="8" Y="220" Width="200" Height="36"/>
        </Panel>"#;
    let diagnostics = kubuno_views::validate::validate_with_default_registry(&kubuno_views::syntax::parse(view));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(kubuno_views::compile::compile(view).is_ok());
    let bad = kubuno_views::validate::validate_with_default_registry(&kubuno_views::syntax::parse(r#"<DbConnection Provider="Oracle"/>"#));
    assert!(!bad.is_empty(), "an unknown provider is reported");
}

#[test]
fn the_navigator_is_a_data_control_and_the_new_properties_are_declared() {
    let info = registry::project_info("BindingNavigator").expect("registered");
    assert_eq!((info.origin, info.kind, info.toolbox_category), (Origin::Linked, ClassKind::Control, Some("Data")));
    assert!(!registry::is_non_visual("BindingNavigator"));
    let nav = registry::lookup("BindingNavigator").expect("meta");
    assert_eq!(nav.default_event(), Some("OnItemClicked"));
    assert!(nav.property("BindingSource").is_some() && nav.property("AutoSave").is_some() && nav.property("Width").is_some(), "a control's properties are inherited");
    let ep = registry::lookup("ErrorProvider").expect("meta");
    assert!(matches!(ep.property("BlinkStyle").map(|p| p.kind), Some(registry::PropKind::Enum(v)) if v.contains(&"NeverBlink")));
    assert!(matches!(ep.property("IconAlignment").map(|p| p.kind), Some(registry::PropKind::Enum(v)) if v.contains(&"MiddleRight")));
    let conn = registry::lookup("DbConnection").expect("meta");
    assert!(matches!(conn.property("Provider").map(|p| p.kind), Some(registry::PropKind::Enum(v)) if v.contains(&"MySql") && v.contains(&"SqlServer")));
    let adapter = registry::lookup("TableAdapter").expect("meta");
    for p in ["PageSize", "PagingMode", "ConflictOption", "RowVersionColumn", "InsertCommand", "UpdateCommand", "DeleteCommand"] {
        assert!(adapter.property(p).is_some(), "TableAdapter.{p}");
    }
    let bs = registry::lookup("BindingSource").expect("meta");
    assert!(bs.property("DataMember").is_some() && bs.property("TableAdapter").is_some());
    assert!(registry::lookup("DbCommand").and_then(|c| c.property("CommandType")).is_some());
}

/// DATA-2: the view runtime creates the data components of a view, owns them, finds them by name,
/// and keeps them across a hot reload.
#[test]
fn the_view_runtime_owns_the_data_components() {
    let view = r#"
        <Panel DesignWidth="600" DesignHeight="400">
          <DbConnection x:Name="db" Provider="Sqlite" ConnectionString="sqlite::memory:"/>
          <TableAdapter x:Name="customersAdapter" Connection="db" SelectCommand="SELECT id, name FROM customers" UpdateTable="customers"/>
          <BindingSource x:Name="customers" DataSource="customersAdapter" Filter="name LIKE 'A%'"/>
          <BindingSource DataSource="customersAdapter"/>
          <ErrorProvider x:Name="errors" DataSource="customers" BlinkStyle="NeverBlink"/>
          <BindingNavigator x:Name="nav" BindingSource="customers" X="8" Y="8" Width="330" Height="32"/>
          <TextField Text="{Binding Source=customers, Path=name, FormatString=N2, NullValue='-', Mode=TwoWay}" X="8" Y="60" Width="200" Height="36"/>
        </Panel>"#;
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    assert_eq!(scope.names(), ["db", "customersAdapter", "customers", "bindingSource2", "errors", "nav"], "the non-visual components and the named controls of the libraries");
    assert_eq!(rt.with_component::<kubuno_data::BindingSource, _>("customers", |bs| bs.data_source.clone()).as_deref(), Some("customersAdapter"), "configured from the XML before the first paint");
    assert_eq!(scope.class_of("customers"), Some("BindingSource"));
    let mut t = kubuno_data::Table::new("customers", vec![kubuno_data::DataColumn::new("name", kubuno_data::DbType::new(kubuno_data::DbKind::Text, "TEXT"))]);
    t.load_row(vec!["Ada".into()]);
    rt.with_component::<kubuno_data::BindingSource, _>("customers", |bs| bs.load(t)).expect("the runtime's instance");
    // The XML's properties were applied when the elements painted (not yet): the instance exists
    // before, and a hot reload keeps it with its rows.
    assert!(rt.reload_from_text(&view.replace("NeverBlink", "AlwaysBlink")));
    assert_eq!(rt.with_component::<kubuno_data::BindingSource, _>("customers", |bs| bs.count()), Some(1), "kept across the reload");
    assert!(rt.reload_from_text(&view.replace("x:Name=\"customers\"", "x:Name=\"people\"").replace("Source=customers", "Source=people").replace("DataSource=\"customers\"", "DataSource=\"people\"").replace("BindingSource=\"customers\"", "BindingSource=\"people\"")));
    assert_eq!(rt.with_component::<kubuno_data::BindingSource, _>("people", |bs| bs.count()), Some(0), "a renamed element is a new component");
    assert!(rt.with_component::<kubuno_data::BindingSource, _>("customers", |bs| bs.count()).is_none());
}

/// `#[kubuno::view]` accepts `x:Name`d data components (the designer's drag and drop from Data
/// Sources names them) through `kubuno_views_meta::kbview::LIBRARY_ELEMENTS`, whose `DATA_ELEMENTS`
/// are exactly what this crate registers.
#[test]
fn the_view_macro_knows_every_class_this_crate_registers() {
    let mut ours: Vec<&str> = registry::all().iter().map(|c| c.name).filter(|n| registry::project_info(n).is_some_and(|i| i.crate_name == Some("kubuno_data"))).collect();
    ours.sort_unstable();
    let mut table = kubuno_views_meta::kbview::DATA_ELEMENTS.to_vec();
    table.sort_unstable();
    assert_eq!(table, ours);
}
