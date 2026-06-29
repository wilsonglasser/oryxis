//! SFTP view helpers: breadcrumb. Split out of views/sftp/mod.rs.

use super::*;
use iced::widget::row;
/// Build a clickable breadcrumb for a remote POSIX path. The root is
/// the only `/` rendered, subsequent segments are added with separators
/// in between, never *after* the root crumb itself, which avoids the
/// `/ / home` doubling that crept in when separators were emitted at the
/// start of every iteration.
pub(crate) fn remote_breadcrumb<'a>(side: SftpPaneSide, path: &str) -> Element<'a, Message> {
    let mut row = iced::widget::Row::new().align_y(iced::Alignment::Center).spacing(2);
    row = row.push(crumb_remote(side, "/", "/"));
    let mut accumulated = String::new();
    let mut first_segment = true;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        accumulated.push('/');
        accumulated.push_str(segment);
        if !first_segment {
            row = row.push(text("/").size(11).color(OryxisColors::t().text_muted));
        }
        first_segment = false;
        row = row.push(crumb_remote(side, segment, &accumulated));
    }
    row.into()
}

/// Build a clickable breadcrumb for a local filesystem path. On Windows
/// the first crumb is the drive letter and clicking it opens the drive
/// picker dropdown. The Unix root chip swallows the next separator so
/// the visual reads `/ home / user` instead of `/ / home / user`. On
/// Windows the implicit `RootDir` component after the drive prefix is
/// skipped (its job is taken by the drive chip itself).
pub(crate) fn local_breadcrumb<'a>(side: SftpPaneSide, path: &std::path::Path) -> Element<'a, Message> {
    // Pick the separator from the path's flavor: real Windows drives
    // (`C:\`, `D:\`) get `\`; everything else (Unix paths, WSL UNC like
    // `\\wsl$\Ubuntu\…`, bare network shares) keeps the Unix `/` since
    // either the user is on Linux or they're navigating into a Linux
    // filesystem from Windows.
    let separator = if is_windows_disk_path(path) { "\\" } else { "/" };
    let mut row = iced::widget::Row::new().align_y(iced::Alignment::Center).spacing(2);
    let mut accumulated = std::path::PathBuf::new();
    let mut first = true;
    let mut last_was_root_or_drive = false;
    let mut had_drive = false;
    for component in path.components() {
        let (label, is_drive, is_root) = match component {
            std::path::Component::Prefix(p) => {
                had_drive = true;
                (p.as_os_str().to_string_lossy().into_owned(), true, false)
            }
            std::path::Component::RootDir => {
                // Skip the implicit root component on Windows, the drive
                // chip already represents the volume root.
                if had_drive {
                    accumulated.push(component.as_os_str());
                    last_was_root_or_drive = true;
                    continue;
                }
                ("/".to_string(), false, true)
            }
            std::path::Component::Normal(s) => (s.to_string_lossy().into_owned(), false, false),
            std::path::Component::CurDir | std::path::Component::ParentDir => continue,
        };
        accumulated.push(component.as_os_str());
        if !first && !last_was_root_or_drive {
            row = row.push(text(separator).size(11).color(OryxisColors::t().text_muted));
        }
        first = false;
        last_was_root_or_drive = is_root || is_drive;
        if is_drive {
            // Drive-letter chip toggles the drives dropdown so the user
            // can jump to another mount without typing.
            row = row.push(
                button(
                    row![
                        iced_fonts::lucide::hard_drive()
                            .size(11)
                            .color(OryxisColors::t().accent),
                        Space::new().width(4),
                        text(label).size(11).color(OryxisColors::t().text_secondary),
                        Space::new().width(2),
                        iced_fonts::lucide::chevron_down()
                            .size(9)
                            .color(OryxisColors::t().text_muted),
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::SftpToggleDrives(side))
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .style(|_, status| {
                    let bg = match status {
                        BtnStatus::Hovered => OryxisColors::t().bg_hover,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border { radius: Radius::from(4.0), ..Default::default() },
                        ..Default::default()
                    }
                }),
            );
        } else {
            row = row.push(local_crumb(side, label, accumulated.clone()));
        }
    }
    row.into()
}

pub(crate) fn crumb_remote<'a>(side: SftpPaneSide, label: &str, full: &str) -> Element<'a, Message> {
    let label = label.to_string();
    let full = full.to_string();
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::SftpNavigateRemote(side, full))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into()
}

pub(crate) fn local_crumb<'a>(side: SftpPaneSide, label: String, full: std::path::PathBuf) -> Element<'a, Message> {
    button(text(label).size(11).color(OryxisColors::t().text_secondary))
        .on_press(Message::SftpNavigateLocal(side, full))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(|_, status| {
            let bg = match status {
                BtnStatus::Hovered => OryxisColors::t().bg_hover,
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border { radius: Radius::from(4.0), ..Default::default() },
                ..Default::default()
            }
        })
        .into()
}
