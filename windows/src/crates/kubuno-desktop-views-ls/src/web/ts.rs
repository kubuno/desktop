//! The TypeScript code-behind of a web view, read with oxc's parser (`vskubuno/docs/WEB-VIEWS.md` §5, WV-7).
//!
//! Only what the language server needs, as byte spans into the file's own text: the imports (to add a type
//! import without rewriting the line), the classes (the one extending the generated `ViewBase` is the
//! code-behind), their members (methods = handlers, `@bind accessor` fields, getters and fields = binding
//! paths), the exported names (a project control's `defineControl` call) and the interfaces (`x:Props`).
//! Nothing here ever prints TypeScript back: every edit made from this model is an insertion at an offset
//! or the replacement of one identifier.

use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPattern, Class, ClassElement, Declaration, ExportDefaultDeclarationKind, Expression, Function, ImportDeclarationSpecifier,
    ImportOrExportKind, MethodDefinitionKind, ModuleExportName, PropertyKey, Statement, TSAccessibility,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};

/// A byte range `[start, end)` of the file's text.
pub type Range = (usize, usize);

fn r(span: Span) -> Range {
    (span.start as usize, span.end as usize)
}

/// One name of an import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportName {
    /// The exported name (`Button` in `import { Button as B }`); `default` / `*` for those forms.
    pub imported: String,
    /// The local binding (`B`).
    pub local: String,
    /// `import { type X }`.
    pub type_only: bool,
    pub span: Range,
}

/// One `import … from '…'` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub span: Range,
    /// The module specifier as written (`@kubuno/views`, `./model`).
    pub source: String,
    pub source_span: Range,
    /// `import type { … }`.
    pub type_only: bool,
    pub names: Vec<ImportName>,
    /// The `{` and `}` of the named imports, when the declaration has them.
    pub braces: Option<(usize, usize)>,
}

/// What a class member is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Constructor,
    Method,
    Getter,
    Setter,
    /// `accessor x = …` (bindable when decorated with `@bind`).
    Accessor,
    Field,
}

/// One parameter of a method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    /// The annotation's type text (`IconButton`, `MouseEventArgs`), without the `:`.
    pub type_text: Option<String>,
    pub optional: bool,
}

/// One member of a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub name_span: Range,
    pub kind: MemberKind,
    pub is_static: bool,
    /// `private` or `#name`.
    pub is_private: bool,
    pub is_async: bool,
    /// Decorated with `@bind`.
    pub bind: bool,
    pub params: Vec<Param>,
    /// A `...rest` parameter.
    pub rest: bool,
    /// A field's / accessor's declared type, a getter's or method's return type.
    pub type_text: Option<String>,
    /// The whole member, decorators included.
    pub span: Range,
    /// The method body's `{` … `}`.
    pub body: Option<Range>,
}

impl Member {
    /// Whether a view may bind to the member (`{Binding name}`): a field, an accessor or a getter.
    pub fn is_bindable(&self) -> bool {
        !self.is_static && !self.is_private && matches!(self.kind, MemberKind::Accessor | MemberKind::Field | MemberKind::Getter)
    }

    /// Whether the member can be an event handler: an instance method, not private.
    pub fn is_method(&self) -> bool {
        !self.is_static && !self.is_private && self.kind == MemberKind::Method
    }

    /// `name(a: A, b?: B): R` — the signature as shown in hovers and completions.
    pub fn signature(&self) -> String {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| format!("{}{}{}", p.name, if p.optional { "?" } else { "" }, p.type_text.as_ref().map(|t| format!(": {t}")).unwrap_or_default()))
            .collect();
        let ret = self.type_text.as_ref().map(|t| format!(": {t}")).unwrap_or_default();
        match self.kind {
            MemberKind::Getter => format!("get {}(){ret}", self.name),
            MemberKind::Setter => format!("set {}({})", self.name, params.join(", ")),
            MemberKind::Accessor => format!("{}accessor {}{ret}", if self.bind { "@bind " } else { "" }, self.name),
            MemberKind::Field => format!("{}{ret}", self.name),
            MemberKind::Constructor | MemberKind::Method => {
                format!("{}{}({}){ret}", if self.is_async { "async " } else { "" }, self.name, params.join(", "))
            }
        }
    }
}

/// One class declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub name: String,
    pub name_span: Range,
    /// The `extends` expression's text (`ViewBase`).
    pub extends: Option<String>,
    pub exported: bool,
    /// The class's own `class` keyword offset (its line's indentation is the class's).
    pub start: usize,
    /// The body's `{` and `}`.
    pub body_open: usize,
    pub body_close: usize,
    pub members: Vec<Member>,
}

impl ClassInfo {
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name)
    }

    pub fn methods(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.is_method())
    }
}

/// A top-level exported name (`export const NoteCard = defineControl(…)`, `export function`, `export class`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedName {
    pub name: String,
    pub span: Range,
    /// Initialised by a `defineControl(…)` call.
    pub define_control: bool,
}

/// An `interface X` or `type X = …` (the `x:Props` type of a view).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    pub name: String,
    pub span: Range,
    /// The interface's property names (empty for a type alias).
    pub members: Vec<(String, Range)>,
}

/// A parsed TypeScript file.
#[derive(Debug, Clone, Default)]
pub struct TsFile {
    pub path: PathBuf,
    pub text: String,
    pub imports: Vec<Import>,
    pub classes: Vec<ClassInfo>,
    pub exports: Vec<ExportedName>,
    pub types: Vec<TypeDecl>,
    /// The parser reported syntax errors (the model is still what could be read).
    pub has_errors: bool,
}

impl TsFile {
    /// Parses `text` (the path decides TS vs TSX).
    pub fn parse(path: &Path, text: &str) -> Self {
        let allocator = Allocator::default();
        let tsx = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsx"));
        let source_type = if tsx { SourceType::tsx() } else { SourceType::ts() };
        let ret = Parser::new(&allocator, text, source_type).parse();
        let mut file = TsFile { path: path.to_path_buf(), text: text.to_string(), has_errors: !ret.diagnostics.is_empty(), ..Default::default() };
        for statement in &ret.program.body {
            file.statement(statement, text);
        }
        file
    }

    /// Reads and parses `path` (through the client's open buffers, [`crate::sources::read`]).
    pub fn load(path: &Path) -> Option<Self> {
        let text = crate::sources::read(path)?;
        Some(Self::parse(path, &text))
    }

    fn statement(&mut self, statement: &Statement<'_>, text: &str) {
        match statement {
            Statement::ImportDeclaration(decl) => {
                let mut names = Vec::new();
                for spec in decl.specifiers.iter().flatten() {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => names.push(ImportName {
                            imported: export_name(&s.imported),
                            local: s.local.name.as_str().to_string(),
                            type_only: s.import_kind == ImportOrExportKind::Type,
                            span: r(s.span),
                        }),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            names.push(ImportName { imported: "default".into(), local: s.local.name.as_str().to_string(), type_only: false, span: r(s.span) })
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            names.push(ImportName { imported: "*".into(), local: s.local.name.as_str().to_string(), type_only: false, span: r(s.span) })
                        }
                    }
                }
                let span = r(decl.span);
                let source_span = r(decl.source.span);
                let braces = text.get(span.0..source_span.0).and_then(|head| {
                    let open = head.find('{')?;
                    let close = head.rfind('}')?;
                    (close > open).then_some((span.0 + open, span.0 + close))
                });
                self.imports.push(Import {
                    span,
                    source: decl.source.value.as_str().to_string(),
                    source_span,
                    type_only: decl.import_kind == ImportOrExportKind::Type,
                    names,
                    braces,
                });
            }
            Statement::ClassDeclaration(class) => self.class(class, false, text),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::ClassDeclaration(class) => {
                    self.class(class, true, text);
                    if let Some(id) = &class.id {
                        self.exports.push(ExportedName { name: id.name.as_str().to_string(), span: r(id.span), define_control: false });
                    }
                }
                Declaration::FunctionDeclaration(f) => {
                    if let Some(id) = &f.id {
                        self.exports.push(ExportedName { name: id.name.as_str().to_string(), span: r(id.span), define_control: false });
                    }
                }
                Declaration::VariableDeclaration(v) => {
                    for d in &v.declarations {
                        if let BindingPattern::BindingIdentifier(id) = &d.id {
                            let define_control = matches!(&d.init, Some(Expression::CallExpression(call)) if call.callee.span().source_text(text) == "defineControl");
                            self.exports.push(ExportedName { name: id.name.as_str().to_string(), span: r(id.span), define_control });
                        }
                    }
                }
                Declaration::TSInterfaceDeclaration(i) => self.interface(i, text),
                Declaration::TSTypeAliasDeclaration(t) => {
                    self.types.push(TypeDecl { name: t.id.name.as_str().to_string(), span: r(t.id.span), members: Vec::new() })
                }
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &export.declaration {
                    self.class(class, true, text);
                }
            }
            Statement::TSInterfaceDeclaration(i) => self.interface(i, text),
            Statement::TSTypeAliasDeclaration(t) => {
                self.types.push(TypeDecl { name: t.id.name.as_str().to_string(), span: r(t.id.span), members: Vec::new() })
            }
            _ => {}
        }
    }

    fn interface(&mut self, i: &oxc_ast::ast::TSInterfaceDeclaration<'_>, text: &str) {
        let members = i
            .body
            .body
            .iter()
            .filter_map(|m| match m {
                oxc_ast::ast::TSSignature::TSPropertySignature(p) => {
                    let name = p.key.span().source_text(text).trim_matches(|c| c == '\'' || c == '"').to_string();
                    Some((name, r(p.key.span())))
                }
                _ => None,
            })
            .collect();
        self.types.push(TypeDecl { name: i.id.name.as_str().to_string(), span: r(i.id.span), members });
    }

    fn class(&mut self, class: &Class<'_>, exported: bool, text: &str) {
        let Some(id) = &class.id else { return };
        let body = r(class.body.span);
        let mut members = Vec::new();
        for element in &class.body.body {
            if let Some(m) = member(element, text) {
                members.push(m);
            }
        }
        self.classes.push(ClassInfo {
            name: id.name.as_str().to_string(),
            name_span: r(id.span),
            extends: class.heritage.as_ref().map(|h| h.expression.span().source_text(text).to_string()),
            exported,
            start: class.span.start as usize,
            body_open: body.0,
            body_close: body.1.saturating_sub(1),
            members,
        });
    }

    /// The code-behind class of a view: the class extending `ViewBase` (the generated base), else the one
    /// named like the view's file stem.
    pub fn view_class(&self, stem: &str) -> Option<&ClassInfo> {
        self.classes.iter().find(|c| c.extends.as_deref() == Some("ViewBase")).or_else(|| self.classes.iter().find(|c| c.name == stem))
    }

    /// The local name under which `imported` is imported from `source` (type or value).
    pub fn imported_as(&self, source: &str, imported: &str) -> Option<&ImportName> {
        self.imports.iter().filter(|i| i.source == source).flat_map(|i| i.names.iter()).find(|n| n.imported == imported)
    }

    /// Whether `local` is a name bound by some import of the file.
    pub fn binds(&self, local: &str) -> bool {
        self.imports.iter().flat_map(|i| i.names.iter()).any(|n| n.local == local)
    }

    /// `\r\n` when the file uses it, else `\n`.
    pub fn newline(&self) -> &'static str {
        if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// The quote of the file's import specifiers (`'` unless they use `"`).
    pub fn quote(&self) -> char {
        self.imports.first().and_then(|i| self.text[i.source_span.0..].chars().next()).filter(|c| *c == '"' || *c == '\'').unwrap_or('\'')
    }

    /// Whether the file ends its import declarations with `;`.
    pub fn semicolons(&self) -> bool {
        self.imports.first().is_some_and(|i| self.text[..i.span.1].trim_end().ends_with(';') || self.text[i.span.1..].trim_start_matches([' ', '\t']).starts_with(';'))
    }
}

fn export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(n) => n.name.as_str().to_string(),
        ModuleExportName::IdentifierReference(n) => n.name.as_str().to_string(),
        ModuleExportName::StringLiteral(s) => s.value.as_str().to_string(),
    }
}

fn key_name(key: &PropertyKey<'_>, text: &str) -> Option<(String, Range, bool)> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some((id.name.as_str().to_string(), r(id.span), false)),
        PropertyKey::PrivateIdentifier(id) => Some((format!("#{}", id.name.as_str()), r(id.span), true)),
        PropertyKey::StringLiteral(s) => Some((s.value.as_str().to_string(), r(s.span), false)),
        other => {
            let _ = text;
            let _ = other;
            None
        }
    }
}

fn is_bind_decorator(expression: &Expression<'_>, text: &str) -> bool {
    let t = expression.span().source_text(text).trim();
    t == "bind" || t.starts_with("bind(") || t.starts_with("bind ")
}

fn params(f: &Function<'_>, text: &str) -> (Vec<Param>, bool) {
    let list = f
        .params
        .items
        .iter()
        .map(|p| Param {
            name: match &p.pattern {
                BindingPattern::BindingIdentifier(id) => id.name.as_str().to_string(),
                other => other.span().source_text(text).to_string(),
            },
            type_text: p.type_annotation.as_ref().map(|t| t.type_annotation.span().source_text(text).to_string()),
            optional: p.optional || p.initializer.is_some(),
        })
        .collect();
    (list, f.params.rest.is_some())
}

fn member(element: &ClassElement<'_>, text: &str) -> Option<Member> {
    match element {
        ClassElement::MethodDefinition(m) => {
            let (name, name_span, hash) = key_name(&m.key, text)?;
            let (params, rest) = params(&m.value, text);
            let kind = match m.kind {
                MethodDefinitionKind::Constructor => MemberKind::Constructor,
                MethodDefinitionKind::Method => MemberKind::Method,
                MethodDefinitionKind::Get => MemberKind::Getter,
                MethodDefinitionKind::Set => MemberKind::Setter,
            };
            Some(Member {
                name,
                name_span,
                kind,
                is_static: m.r#static,
                is_private: hash || m.accessibility == Some(TSAccessibility::Private),
                is_async: m.value.r#async,
                bind: m.decorators.iter().any(|d| is_bind_decorator(&d.expression, text)),
                params,
                rest,
                type_text: m.value.return_type.as_ref().map(|t| t.type_annotation.span().source_text(text).to_string()),
                span: r(m.span),
                body: m.value.body.as_ref().map(|b| r(b.span)),
            })
        }
        ClassElement::PropertyDefinition(p) => {
            let (name, name_span, hash) = key_name(&p.key, text)?;
            Some(Member {
                name,
                name_span,
                kind: MemberKind::Field,
                is_static: p.r#static,
                is_private: hash || p.accessibility == Some(TSAccessibility::Private),
                is_async: false,
                bind: p.decorators.iter().any(|d| is_bind_decorator(&d.expression, text)),
                params: Vec::new(),
                rest: false,
                type_text: p.type_annotation.as_ref().map(|t| t.type_annotation.span().source_text(text).to_string()),
                span: r(p.span),
                body: None,
            })
        }
        ClassElement::AccessorProperty(a) => {
            let (name, name_span, hash) = key_name(&a.key, text)?;
            Some(Member {
                name,
                name_span,
                kind: MemberKind::Accessor,
                is_static: a.r#static,
                is_private: hash || a.accessibility == Some(TSAccessibility::Private),
                is_async: false,
                bind: a.decorators.iter().any(|d| is_bind_decorator(&d.expression, text)),
                params: Vec::new(),
                rest: false,
                type_text: a.type_annotation.as_ref().map(|t| t.type_annotation.span().source_text(text).to_string()),
                span: r(a.span),
                body: None,
            })
        }
        _ => None,
    }
}

/// The start of the line holding byte `offset`.
pub fn line_start(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
}

/// The indentation (leading spaces and tabs) of the line holding byte `offset`.
pub fn indentation_at(text: &str, offset: usize) -> &str {
    let start = line_start(text, offset);
    let line = &text[start..];
    let len = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = r#"/** Doc. */
import i18n from 'i18next'
import { bind, type ElementHandle, type IconButton } from '@kubuno/views'

import { ViewBase } from './AccountMenu.kbcontrol'
import type { AccountUser } from './model'

export interface AccountMenuProps {
  user?: AccountUser
  showAdmin?: boolean
}

export class AccountMenu extends ViewBase {
  @bind accessor expanded = true
  private readonly cache = new Map<string, string>()
  count: number = 0

  get greeting(): string {
    return i18n.t('x')
  }

  close_button_click(_sender: IconButton): void {
    this.props.onCloseRequested?.()
  }

  async load(sender: ElementHandle, e?: unknown) {}

  private rowId(e: unknown): string | null { return null }
  static create() {}
  #secret() {}
}

export const NoteCard = defineControl(NoteCardImpl, {})
export default AccountMenu.component()
"#;

    #[test]
    fn reads_imports_classes_members_and_exports() {
        let f = TsFile::parse(Path::new("AccountMenu.ts"), CODE);
        assert!(!f.has_errors);
        assert_eq!(f.imports.len(), 4);
        let views = &f.imports[1];
        assert_eq!(views.source, "@kubuno/views");
        assert!(!views.type_only);
        assert_eq!(views.names.iter().map(|n| (n.imported.as_str(), n.type_only)).collect::<Vec<_>>(), vec![("bind", false), ("ElementHandle", true), ("IconButton", true)]);
        let (open, close) = views.braces.expect("braces");
        assert_eq!(&CODE[open..=close], "{ bind, type ElementHandle, type IconButton }");
        assert!(f.imports[3].type_only);

        let class = f.view_class("AccountMenu").expect("the view class");
        assert_eq!(class.name, "AccountMenu");
        assert_eq!(class.extends.as_deref(), Some("ViewBase"));
        assert_eq!(&CODE[class.body_close..=class.body_close], "}");
        assert!(CODE[class.body_open..].starts_with('{'));
        let expanded = class.member("expanded").expect("expanded");
        assert!(expanded.bind && expanded.kind == MemberKind::Accessor && expanded.is_bindable());
        assert!(!class.member("cache").expect("cache").is_bindable(), "private");
        assert!(class.member("count").expect("count").is_bindable());
        let greeting = class.member("greeting").expect("greeting");
        assert_eq!(greeting.kind, MemberKind::Getter);
        assert_eq!(greeting.type_text.as_deref(), Some("string"));
        let close = class.member("close_button_click").expect("handler");
        assert!(close.is_method());
        assert_eq!(close.params, vec![Param { name: "_sender".into(), type_text: Some("IconButton".into()), optional: false }]);
        assert_eq!(close.signature(), "close_button_click(_sender: IconButton): void");
        assert_eq!(&CODE[close.name_span.0..close.name_span.1], "close_button_click");
        let load = class.member("load").expect("load");
        assert!(load.is_async && load.params[1].optional);
        let methods: Vec<&str> = class.methods().map(|m| m.name.as_str()).collect();
        assert_eq!(methods, vec!["close_button_click", "load"], "private, static and #private are not handlers");

        assert!(f.exports.iter().any(|e| e.name == "NoteCard" && e.define_control));
        let props = f.types.iter().find(|t| t.name == "AccountMenuProps").expect("props");
        assert_eq!(props.members.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), vec!["user", "showAdmin"]);
        assert_eq!(f.quote(), '\'');
        assert!(!f.semicolons());
        assert_eq!(f.newline(), "\n");
        assert_eq!(indentation_at(CODE, close.name_span.0), "  ");
    }

    #[test]
    fn a_broken_file_is_flagged_and_semicolons_are_detected() {
        let f = TsFile::parse(Path::new("X.ts"), "import { a } from 'b';\nexport class X extends ViewBase {\n  ok() {}\n  broken( {\n");
        assert!(f.has_errors);
        let f = TsFile::parse(Path::new("X.ts"), "import { a } from \"b\";\r\nexport class X extends ViewBase {}\r\n");
        assert!(!f.has_errors);
        assert!(f.semicolons());
        assert_eq!(f.quote(), '"');
        assert_eq!(f.newline(), "\r\n");
    }
}
