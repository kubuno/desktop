//! Port of `Files.App/UserControls/Pane/InfoPane.xaml`: the Details/Preview
//! pane card — Détails/Aperçu selector (SelectionPill), big thumbnail or
//! content preview, name, property rows, tags stub, and Properties button.

use drive_app_controls::themes::shape;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING;

use crate::services::storage::IconCache;
use crate::view_models::shell_view_model::Location;
use crate::ui::{Hot, Layout, Painter, Rect, UiState};

/// "#RRGGBB" (tag color) → D2D color.
fn tag_color(hex: &str) -> D2D1_COLOR_F {
    let p = hex.trim_start_matches('#');
    let c = u32::from_str_radix(p, 16).unwrap_or(0x808080);
    D2D1_COLOR_F {
        r: ((c >> 16) & 0xFF) as f32 / 255.0,
        g: ((c >> 8) & 0xFF) as f32 / 255.0,
        b: (c & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

impl Painter<'_> {
    /// Details pane (port of InfoPane.xaml): tabs, thumbnail, name,
    /// property rows, tags, Properties button.
    pub(crate) fn draw_info_pane(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let Some(pane) = &layout.info_pane else { return };
        let t = self.theme;
        let f = &self.renderer.formats;
        let tab = state.active();

        // Selected entry, or the current folder itself (original behavior).
        let (name, path, is_dir): (String, String, bool) = match tab.selected_entry() {
            Some(entry) => (entry.name.clone(), entry.path.clone(), entry.is_dir),
            None => match &tab.location {
                Location::Dir(p) => (tab.title(), p.to_string_lossy().into_owned(), true),
                _ => {
                    let center = Rect::new(pane.left, pane.top + 60.0, pane.right, pane.top + 100.0);
                    self.text(drive_localization::tr("NoItemSelected"), &center, &f.body, &t.text_secondary, true);
                    return;
                }
            },
        };

        // A FLAT panel like the listing beside it — same white surface, same
        // rounding, no drop shadow (the web stacks panels on the page
        // background, it never floats them).
        self.fill_rounded(pane, shape::radius::XL, &t.layer_background);

        // Détails/Aperçu selector — `@ui/Tabs.tsx`, variant "underline".
        // The web draws NO box: a single `border-b border-border` hairline runs
        // under the whole strip (Tabs.tsx:172) and the active tab is marked by a
        // `::before` band, not by a filled pill (Tabs.tsx:117-118):
        // `bottom-0 mx-2 h-[3px] rounded-t-[3px] bg-primary`. The active LABEL
        // is the accent itself (`text-primary`, Tabs.tsx:122) at `font-medium`;
        // inactive labels are `text-text-secondary` at the normal weight. Hover
        // tints the tab with `surface-2` whether it is active or not
        // (Tabs.tsx:121). Confirmed live in drive at `MobileHome.tsx:127-138`.
        let selected_tab = crate::services::settings::get().info_pane_tab;
        let strip_bottom = layout.info_tabs[0].bottom;
        let rule = Rect::new(
            layout.info_tabs[0].left,
            strip_bottom - 1.0,
            layout.info_tabs[1].right,
            strip_bottom,
        );
        self.fill_rounded(&rule, 0.0, &t.divider);

        let labels = ["Details", "Preview"];
        for (i, rect) in layout.info_tabs.iter().enumerate() {
            let checked = (i == 1) == (selected_tab == crate::services::settings::InfoPaneTab::Preview);
            if state.hot == Some(Hot::InfoTab(i)) {
                // `hover:bg-surface-2`, square: an underline tab carries no
                // radius. It stops 1 DIP short of the bottom because in the web
                // the rule is the CONTAINER's border, so a tab's box ends above
                // it and never paints over it.
                let hover = Rect::new(rect.left, rect.top, rect.right, rect.bottom - 1.0);
                self.fill_rounded(&hover, 0.0, &t.card_preview_background);
            }
            let fg = if checked { &t.accent } else { &t.text_secondary };
            let format = if checked { &f.body_strong } else { &f.body };
            self.text(drive_localization::tr(labels[i]), rect, format, fg, true);
            if checked {
                // The indicator: 3 DIP tall, inset 8 DIP on each side (`mx-2`),
                // top corners rounded to 3, sitting ON the hairline.
                let pill = Rect::new(rect.left + 8.0, rect.bottom - 3.0, rect.right - 8.0, rect.bottom);
                self.fill_top_rounded(&pill, 3.0, &t.accent);
            }
        }

        let tabs_bottom = layout.info_tabs[0].bottom;

        // Aperçu tab: content preview (image thumbnail or text excerpt),
        // else the original "not available" message.
        if selected_tab == crate::services::settings::InfoPaneTab::Preview {
            let area = Rect::new(pane.left + 12.0, tabs_bottom + 12.0, pane.right - 12.0, layout.info_properties.top - 12.0);
            use drive_shared::helpers::file_extensions as fe;
            let n = Some(name.as_str());
            if is_dir || fe::is_image_file(n) || fe::is_video_file(n) || fe::is_pdf_file(n) {
                let size = (area.right - area.left).min(area.bottom - area.top).min(256.0);
                let size_px = (size * scale).round() as i32;
                match icons.get_thumbnail(&path, size_px) {
                    Some(bitmap) => self.image(bitmap, &area, size),
                    None => self.image(&self.renderer.images.folder, &area, 140.0),
                }
            } else if fe::is_text_file(n) || fe::is_markdown_file(n) || fe::is_html_file(n) {
                let excerpt = std::fs::read(&path)
                    .map(|bytes| String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned())
                    .unwrap_or_default();
                self.text_aligned(&excerpt, &area, &f.caption_wrap, &t.text_primary, DWRITE_TEXT_ALIGNMENT_LEADING);
            } else {
                self.text(drive_localization::tr("DetailsPanePreviewNotAvaliableText"), &area, &f.body, &t.text_secondary, true);
            }
            // Properties button is shared with the details tab (pinned here —
            // the Aperçu tab has no scrolling stack).
            let b = &layout.info_properties;
            state.info_properties_rect.set(Some((b.left, b.top, b.right, b.bottom)));
            self.draw_info_properties_button(b, state);
            return;
        }

        // Big thumbnail. A FOLDER gets the same flat monochrome mark as the
        // listing rather than the shell's folder preview: it keeps the pane
        // consistent, and that preview arrives as an opaque bitmap whose
        // backdrop reads as a black square on our surface.
        let thumb_rect = Rect::new(pane.left, tabs_bottom + 12.0, pane.right, tabs_bottom + 168.0);
        if is_dir {
            self.vector_icon("Folder", &thumb_rect, 64.0, &t.text_tertiary);
        } else {
            let size_px = (140.0 * scale).round() as i32;
            match icons.get_thumbnail(&path, size_px) {
                Some(bitmap) => self.image(bitmap, &thumb_rect, 140.0),
                None => self.vector_icon("File", &thumb_rect, 64.0, &t.text_tertiary),
            }
        }

        // Name: centered, bold, 20px (InfoPane.xaml DetailsListHeader). A
        // drive root shows "Libellé (X:)", like the DriveItem.
        let name = if is_dir && crate::main_window::is_drive_root(&path) {
            let letter = path.as_bytes()[0] as char;
            let root = windows::core::HSTRING::from(format!("{letter}:\\"));
            let mut label_buf = [0u16; 261];
            unsafe {
                windows::Win32::Storage::FileSystem::GetVolumeInformationW(
                    &root,
                    Some(&mut label_buf),
                    None,
                    None,
                    None,
                    None,
                )
            }
            .ok()
            .map(|_| String::from_utf16_lossy(&label_buf).trim_end_matches('\0').to_string())
            .filter(|s| !s.is_empty())
            .map(|label| format!("{label} ({letter}:)"))
            .unwrap_or(name)
        } else {
            name
        };
        // SCROLLING REGION (the original RootPropertiesScrollViewer): name,
        // rows, tags AND Properties button — nothing overlaps, the
        // wheel scrolls, the button is the last element of the stack.
        let region = Rect::new(pane.left, thumb_rect.bottom + 4.0, pane.right, pane.bottom);
        let viewport = region.bottom - region.top;
        let scroll = state
            .info_pane_scroll
            .min((state.info_pane_extent.get() - viewport).max(0.0))
            .max(0.0);
        unsafe {
            self.ctx.PushAxisAlignedClip(
                &region.d2d(),
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }
        let mut y = region.top + 4.0 - scroll;
        let name_rect = Rect::new(pane.left + 12.0, y, pane.right - 12.0, y + 28.0);
        // Title centered, truncated with "…" when the name overflows the pane.
        self.text_ellipsis_center(&name, &name_rect, &f.title, &t.text_primary);
        y = name_rect.bottom + 12.0;

        // DRIVE ROOT: InfoPane.xaml's `DriveStorageDetailsAvailable`
        // state replaces the property rows — file
        // system + type, used space (accent), StorageBar,
        // Available/Total — then the Properties button in the flow.
        if is_dir && crate::main_window::is_drive_root(&path) {
            let bottom = self.draw_drive_details(pane, &path, y);
            self.finish_scroll_region(layout, state, &region, bottom, scroll);
            return;
        }

        // Property rows (labels from the original resw). Dates follow
        // `ToLongLabel`: long date + time + "Il y a …" if recent, depending on
        // the format SETTING (Application/System/Universal).
        let date_format = crate::services::settings::get().date_time_format;
        let metadata = std::fs::metadata(&path).ok();
        let format_time = |time: Option<std::time::SystemTime>| -> String {
            time.map(|st| {
                crate::services::date_time_formatter::to_long_label(st.into(), date_format)
            })
            .unwrap_or_default()
        };
        // The rows depend on the item's TYPE, like the Preview
        // ViewModels: folder (count + dates + path), shortcut (target +
        // dates + path — the original skips the system properties), file
        // (the SHELL properties from PreviewPanePropertiesInformation.json —
        // image dimensions, media duration, music, document…).
        let is_shortcut = path.to_lowercase().ends_with(".lnk");
        let tr = drive_localization::tr;
        let mut rows: Vec<(String, String)> = Vec::new();
        if is_dir {
            // `Strings.Items` is an ICU plural: "1 élément", "19 éléments".
            let count = std::fs::read_dir(&path).map(|it| it.count()).unwrap_or(0);
            let plural = crate::user_controls::status_bar::icu_plural(tr("Items"), count);
            rows.push((tr("PropertyItemCount").to_string(), format!("{count} {plural}")));
            rows.push((tr("PropertyDateModified").to_string(), format_time(metadata.as_ref().and_then(|m| m.modified().ok()))));
            rows.push((tr("PropertyDateCreated").to_string(), format_time(metadata.as_ref().and_then(|m| m.created().ok()))));
            rows.push((tr("PropertyParsingPath").to_string(), path.clone()));
        } else if is_shortcut {
            // `ShortcutPreviewViewModel`: the shortcut's TARGET at the top
            // (the original skips the system properties for shortcuts).
            if let Some(target) = crate::utils::storage::resolve_shortcut(&path) {
                rows.push((tr("PropertyItemTargetPath").to_string(), target));
            }
            rows.push((tr("PropertyDateModified").to_string(), format_time(metadata.as_ref().and_then(|m| m.modified().ok()))));
            rows.push((tr("PropertyDateCreated").to_string(), format_time(metadata.as_ref().and_then(|m| m.created().ok()))));
            rows.push((tr("PropertyParsingPath").to_string(), path.clone()));
        } else {
            rows.extend(crate::utils::file_properties::preview_pane_properties(&path));
        }

        // Variable-height rows: the VALUES wrap
        // (TextWrapping="Wrap") instead of being truncated — the path especially.
        let inner_w = pane.right - pane.left - 24.0;
        for (key, value) in rows {
            // Header: caption SemiBold (Local.FileDetailsHeaderTextBlockStyle).
            self.text(&key, &Rect::new(pane.left + 12.0, y, pane.right - 12.0, y + 20.0), &f.caption_strong, &t.text_secondary, false);
            let vh = self.measure_height(&value, &f.body_wrap, inner_w).max(20.0);
            self.text(&value, &Rect::new(pane.left + 12.0, y + 20.0, pane.right - 12.0, y + 20.0 + vh + 2.0), &f.body_wrap, &t.text_primary, false);
            y += 20.0 + vh + 12.0;
        }

        // Tags: header, PILLS of the item's tags
        // (tinted FilledTag icon + name, wrapping flow), then the
        // "Modifier les étiquettes" button (TagEdit icon + text, clickable).
        self.text(drive_localization::tr("FileTags"), &Rect::new(pane.left + 12.0, y, pane.right - 12.0, y + 20.0), &f.caption_strong, &t.text_secondary, false);
        let defs = crate::services::settings::get().file_tags;
        let assigned = crate::utils::file_tags::read_file_tags(&path);
        let right = pane.right - 12.0;
        let mut x = pane.left + 12.0;
        let mut yy = y + 22.0;
        for uid in &assigned {
            let Some(def) = defs.iter().find(|t| &t.uid == uid) else { continue };
            let w = 16.0 + 8.0 + self.measure(&def.name, &f.body) + 8.0;
            if x + w > right && x > pane.left + 12.0 {
                x = pane.left + 12.0;
                yy += 30.0;
            }
            let icon = Rect::new(x, yy + 2.0, x + 16.0, yy + 18.0);
            self.vector_icon("FilledTag", &icon, 16.0, &tag_color(&def.color));
            self.text(
                &def.name,
                &Rect::new(x + 24.0, yy - 2.0, x + w + 4.0, yy + 20.0),
                &f.body,
                &t.text_primary,
                false,
            );
            x += w + 16.0;
        }
        let edit_w = 16.0 + 8.0 + self.measure(drive_localization::tr("EditTags"), &f.body) + 8.0;
        if x + edit_w > right && x > pane.left + 12.0 {
            x = pane.left + 12.0;
            yy += 30.0;
        }
        let tag_icon = Rect::new(x, yy + 2.0, x + 16.0, yy + 18.0);
        self.vector_icon("TagEdit", &tag_icon, 16.0, &t.text_primary);
        self.text(
            drive_localization::tr("EditTags"),
            &Rect::new(x + 24.0, yy - 2.0, x + edit_w + 8.0, yy + 20.0),
            &f.body,
            &t.text_primary,
            false,
        );
        // Clickable area of the tags button, captured by the drawing
        // (position depends on measured heights) for hit-testing.
        state
            .info_edit_tags_rect
            .set(Some((x - 4.0, yy - 4.0, x + edit_w + 8.0, yy + 22.0)));

        self.finish_scroll_region(layout, state, &region, yy + 30.0, scroll);
    }

    /// Closes the scrolling stack: the Properties button as the LAST element of
    /// the flow (Margin 12,0,8,8 of the original), captures the total extent for the
    /// wheel, and closes the clip.
    fn finish_scroll_region(
        &self,
        layout: &Layout,
        state: &UiState,
        region: &Rect,
        content_bottom: f32,
        scroll: f32,
    ) {
        let bw = layout.info_properties.right - layout.info_properties.left;
        let button = Rect::new(
            region.left + 12.0,
            content_bottom + 8.0,
            region.left + 12.0 + bw,
            content_bottom + 40.0,
        );
        state
            .info_properties_rect
            .set(Some((button.left, button.top, button.right, button.bottom)));
        self.draw_info_properties_button(&button, state);
        state.info_pane_extent.set(button.bottom + 8.0 + scroll - region.top);
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }
    }

    /// The pane's drive state (`DriveStorageDetailsAvailable`): file
    /// system + type, USED space in accent, StorageBar (4 track /
    /// 8 value), then Available / Total. Returns the BOTTOM of the content (for the
    /// Properties button that follows in the flow).
    fn draw_drive_details(&self, pane: &Rect, path: &str, top: f32) -> f32 {
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::{
            GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeInformationW,
        };
        let t = self.theme;
        let f = &self.renderer.formats;
        let tr = drive_localization::tr;
        let root = HSTRING::from(format!("{}\\", path.trim_end_matches('\\')));

        // File system ("NTFS") + localized type ("Disque local"),
        // the two centered Runs of DriveFormatAndTypeTextBlock.
        let mut fs_buf = [0u16; 64];
        let fs = unsafe {
            GetVolumeInformationW(&root, None, None, None, None, Some(&mut fs_buf))
        }
        .ok()
        .map(|_| String::from_utf16_lossy(&fs_buf).trim_end_matches('\0').to_string())
        .unwrap_or_default();
        // `DriveItem.TypeText`: the `DriveType{Type}` resource.
        let type_key = match unsafe { GetDriveTypeW(&root) } {
            2 => "DriveTypeRemovable",
            4 => "DriveTypeNetwork",
            5 => "DriveTypeCDRom",
            6 => "DriveTypeRam",
            _ => "DriveTypeFixed",
        };
        let type_text = drive_localization::tr_opt(type_key).unwrap_or("");
        let fs_w = self.measure(&fs, &f.body);
        let type_w = self.measure(type_text, &f.body);
        let cx = (pane.left + pane.right) / 2.0;
        let line_left = cx - (fs_w + 6.0 + type_w) / 2.0;
        let line = Rect::new(line_left, top + 4.0, line_left + fs_w + 6.0, top + 26.0);
        self.text(&fs, &line, &f.body, &t.text_primary, false);
        let type_rect = Rect::new(
            line_left + fs_w + 6.0,
            top + 4.0,
            line_left + fs_w + 6.0 + type_w + 6.0,
            top + 26.0,
        );
        self.text(type_text, &type_rect, &f.body, &t.text_secondary, false);

        // Spaces: used (accent, centered subtitle), bar, then the
        // Available / Total grid.
        let (mut free, mut total) = (0u64, 0u64);
        let _ = unsafe { GetDiskFreeSpaceExW(&root, None, Some(&mut total), Some(&mut free)) };
        let used = total.saturating_sub(free);
        let used_rect = Rect::new(pane.left + 12.0, top + 34.0, pane.right - 12.0, top + 62.0);
        self.text(
            &crate::data::items::format_bytes_fr(used),
            &used_rect,
            &f.title,
            &t.accent,
            true,
        );

        // Storage gauge — the drive web's bar, not WinUI's StorageBar (whose
        // value was FATTER than its track). `FilesTreeSidebar.tsx:146-147` and
        // `FilesStorageGaugeHeader.tsx:37-41`: one `h-1.5` (6 DIP) `bg-black/10`
        // track, `rounded-full`, with a `rounded-full` fill of the same height
        // colour-coded by the fill ratio (see `gauge_color`).
        const BAR_H: f32 = 6.0;
        let bar = Rect::new(pane.left + 20.0, top + 74.0, pane.right - 20.0, top + 74.0 + BAR_H);
        self.fill_rounded(&bar, shape::pill(BAR_H), &t.drive_bar_track);
        if total > 0 {
            let frac = (used as f64 / total as f64) as f32;
            let value = Rect::new(bar.left, bar.top, bar.left + (bar.right - bar.left) * frac, bar.bottom);
            let fill = crate::user_controls::widgets::gauge_color(t, frac);
            self.fill_rounded(&value, shape::pill(BAR_H), &fill);
        }

        // Available (left, strong) / Total (right, dimmed).
        let row1 = Rect::new(pane.left + 12.0, top + 94.0, pane.right - 12.0, top + 116.0);
        self.text(&crate::data::items::format_bytes_fr(free), &row1, &f.body_strong, &t.text_primary, false);
        self.text_aligned(
            &crate::data::items::format_bytes_fr(total),
            &row1,
            &f.body_strong,
            &t.text_secondary,
            windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING,
        );
        let row2 = Rect::new(pane.left + 12.0, top + 116.0, pane.right - 12.0, top + 138.0);
        self.text(tr("Available"), &row2, &f.body, &t.text_primary, false);
        self.text_aligned(
            tr("Total"),
            &row2,
            &f.body,
            &t.text_secondary,
            windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING,
        );
        top + 138.0
    }

    /// "Propriétés" — `@ui/Button.tsx` variant "secondary", size "sm".
    ///
    /// Button.tsx:27 gives the fills: `bg-white` at rest (the pane's own
    /// surface, NOT surface-1 as the WinUI chrome had it) over a
    /// `border border-border` outline, `hover:bg-surface-1`. The radius is
    /// fixed at `rounded-md` = 4 (Button.tsx:16-17) and the label is never bold
    /// (Button.tsx:23). The 32 DIP height and the 12 DIP left padding of `px-3`
    /// come from `SIZE.sm` (Button.tsx:41); the icon then sits `gap-1.5` (6 DIP)
    /// from the label.
    fn draw_info_properties_button(&self, button: &Rect, state: &UiState) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let fill = if state.hot == Some(Hot::InfoProperties) {
            &t.card_background // hover:bg-surface-1
        } else {
            &t.layer_background // bg-white == surface-0
        };
        self.fill_rounded(button, shape::radius::SM, fill);
        self.stroke_rounded(button, shape::radius::SM, &t.card_stroke);
        let cy = (button.top + button.bottom) / 2.0;
        let icon = Rect::new(button.left + 12.0, cy - 8.0, button.left + 28.0, cy + 8.0);
        self.vector_icon("Properties", &icon, 16.0, &t.text_primary);
        self.text(
            drive_localization::tr("Properties"),
            &Rect::new(button.left + 34.0, button.top, button.right - 12.0, button.bottom),
            &f.body,
            &t.text_primary,
            false,
        );
    }
}
