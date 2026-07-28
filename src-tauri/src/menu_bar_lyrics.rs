use tauri::AppHandle;

#[cfg(target_os = "macos")]
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Manager,
};
#[cfg(any(target_os = "macos", test))]
use unicode_segmentation::UnicodeSegmentation;
#[cfg(any(target_os = "macos", test))]
use unicode_width::UnicodeWidthStr;

#[cfg(target_os = "macos")]
const MENU_BAR_LYRICS_ID: &str = "menu-bar-lyrics";
#[cfg(any(target_os = "macos", test))]
const ELLIPSIS: &str = "…";
#[cfg(any(target_os = "macos", test))]
const MIN_MENU_BAR_WIDTH: u16 = 24;
#[cfg(any(target_os = "macos", test))]
const MAX_MENU_BAR_WIDTH: u16 = 56;

#[cfg(target_os = "macos")]
pub fn update(
    app: &AppHandle,
    enabled: bool,
    line: &str,
    title: &str,
    subtitle: &str,
    max_width: u16,
) -> tauri::Result<()> {
    let line = normalize_line(line);
    if !enabled {
        if let Some(tray) = app.tray_by_id(MENU_BAR_LYRICS_ID) {
            tray.set_visible(false)?;
        }
        return Ok(());
    }

    let max_width = usize::from(max_width.clamp(MIN_MENU_BAR_WIDTH, MAX_MENU_BAR_WIDTH));
    let display_line = display_line(&line, max_width);
    let tooltip = tooltip(title, subtitle, &line);
    let tray = match app.tray_by_id(MENU_BAR_LYRICS_ID) {
        Some(tray) => tray,
        None => build_tray(app, &display_line, &tooltip)?,
    };
    tray.set_title(Some(display_line))?;
    tray.set_tooltip(Some(tooltip))?;
    // tray-icon removes the NSStatusItem when hidden. Keeping it alive while the
    // preference is enabled preserves its position in menu bar managers.
    tray.set_visible(true)
}

#[cfg(not(target_os = "macos"))]
pub fn update(
    _app: &AppHandle,
    _enabled: bool,
    _line: &str,
    _title: &str,
    _subtitle: &str,
    _max_width: u16,
) -> tauri::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn build_tray(app: &AppHandle, title: &str, tooltip: &str) -> tauri::Result<TrayIcon> {
    TrayIconBuilder::with_id(MENU_BAR_LYRICS_ID)
        .title(title)
        .tooltip(tooltip)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
}

#[cfg(target_os = "macos")]
fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

#[cfg(any(target_os = "macos", test))]
fn tooltip(title: &str, subtitle: &str, line: &str) -> String {
    let track = [normalize_line(title), normalize_line(subtitle)]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" - ");
    match (track.is_empty(), line.is_empty()) {
        (true, _) => line.to_owned(),
        (false, true) => track,
        (false, false) => format!("{track}\n{line}"),
    }
}

#[cfg(any(target_os = "macos", test))]
fn display_line(line: &str, max_width: usize) -> String {
    if line.is_empty() {
        ELLIPSIS.to_owned()
    } else {
        truncate_line(line, max_width)
    }
}

#[cfg(any(target_os = "macos", test))]
fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(any(target_os = "macos", test))]
fn truncate_line(line: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(line) <= max_width {
        return line.to_owned();
    }

    let content_width = max_width.saturating_sub(UnicodeWidthStr::width(ELLIPSIS));
    let mut words = String::new();
    for word in line.split_whitespace() {
        let separator = if words.is_empty() { "" } else { " " };
        let candidate_width = UnicodeWidthStr::width(words.as_str())
            + UnicodeWidthStr::width(separator)
            + UnicodeWidthStr::width(word);
        if candidate_width > content_width {
            break;
        }
        words.push_str(separator);
        words.push_str(word);
    }
    if !words.is_empty() {
        words.push_str(ELLIPSIS);
        return words;
    }

    let mut graphemes = String::new();
    for grapheme in line.graphemes(true) {
        if UnicodeWidthStr::width(graphemes.as_str()) + UnicodeWidthStr::width(grapheme)
            > content_width
        {
            break;
        }
        graphemes.push_str(grapheme);
    }
    graphemes.push_str(ELLIPSIS);
    graphemes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_line_should_collapse_line_breaks_and_spacing() {
        assert_eq!(normalize_line("  first\n  second  "), "first second");
    }

    #[test]
    fn truncate_line_should_prefer_complete_words() {
        assert_eq!(
            truncate_line("Coffee cools while the melody stays", 20),
            "Coffee cools while…"
        );
    }

    #[test]
    fn truncate_line_should_preserve_unicode_graphemes() {
        assert_eq!(truncate_line("咖啡凉了旋律还在继续播放", 11), "咖啡凉了旋…");
        assert_eq!(truncate_line("👩‍🎤👩‍🎤👩‍🎤", 5), "👩‍🎤👩‍🎤…");
    }

    #[test]
    fn native_width_limits_should_match_the_settings_contract() {
        assert_eq!(MIN_MENU_BAR_WIDTH, 24);
        assert_eq!(MAX_MENU_BAR_WIDTH, 56);
    }

    #[test]
    fn tooltip_should_include_track_context() {
        assert_eq!(
            tooltip("Song", "Artist", "A lyric"),
            "Song - Artist\nA lyric"
        );
    }

    #[test]
    fn tooltip_should_omit_empty_lyric_line() {
        assert_eq!(tooltip("Song", "Artist", ""), "Song - Artist");
    }

    #[test]
    fn display_line_should_keep_a_placeholder_for_empty_lyrics() {
        assert_eq!(display_line("", 40), ELLIPSIS);
    }
}
