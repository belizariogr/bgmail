//! Desktop notifications for new mail.
//!
//! One portable entry point ([`notify_new_mail`]); `notify-rust` hides the
//! platform backends (D-Bus on Linux/BSD, `NSUserNotification` on macOS, WinRT
//! toasts on Windows). Showing a notification can block on IPC, so it always
//! runs on a short-lived background thread, never on the UI thread.

use mail_sync::NewMailNotice;

use crate::locale::Language;

/// More messages than this are summarized in one notification.
const MAX_INDIVIDUAL: usize = 3;

/// Title and body of one notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
}

/// Turns newly arrived messages into notifications: one per message for a
/// few, a single summary for a burst (so a first sync after a vacation does
/// not flood the desktop).
pub fn notices_for(account: &str, messages: &[NewMailNotice], language: Language) -> Vec<Notice> {
    if messages.is_empty() {
        return Vec::new();
    }
    if messages.len() > MAX_INDIVIDUAL {
        let summary = match language {
            Language::English => format!("{} new messages", messages.len()),
            Language::Portuguese => format!("{} novas mensagens", messages.len()),
        };
        return vec![Notice {
            summary,
            body: account.to_string(),
        }];
    }
    messages
        .iter()
        .map(|message| Notice {
            summary: if message.sender.trim().is_empty() {
                account.to_string()
            } else {
                message.sender.clone()
            },
            body: if message.subject.trim().is_empty() {
                match language {
                    Language::English => "(no subject)".to_string(),
                    Language::Portuguese => "(sem assunto)".to_string(),
                }
            } else {
                message.subject.clone()
            },
        })
        .collect()
}

/// Shows desktop notifications for `messages` (best effort, off-thread).
pub fn notify_new_mail(account: &str, messages: &[NewMailNotice], language: Language) {
    let notices = notices_for(account, messages, language);
    if notices.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("bgmail-notify".into())
        .spawn(move || {
            for notice in notices {
                show(&notice);
            }
        });
    if let Err(error) = spawned {
        eprintln!("BGMail: could not show a notification: {error}");
    }
}

/// Windows AppUserModelID of the installed app: the NSIS installer stamps it
/// on the Start Menu shortcut (`identifier` in `[package.metadata.packager]`).
/// Toasts carrying it show as BGMail (name + icon).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WINDOWS_APP_ID: &str = "io.github.belizariogr.bgmail";

/// Where the NSIS installer may have put the Start Menu shortcut, given the
/// per-user and all-users `Start Menu\Programs` folders (the installer's
/// start-menu folder may be empty or the product name).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn shortcut_candidates(programs_dirs: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    programs_dirs
        .iter()
        .flat_map(|dir| {
            [
                dir.join("BGMail.lnk"),
                dir.join("BGMail").join("BGMail.lnk"),
            ]
        })
        .collect()
}

/// The toast app id: ours once the installer registered it (a shortcut
/// exists), otherwise `None` so `notify-rust` falls back to PowerShell's id —
/// Windows silently drops toasts from an unregistered id, so an unpackaged
/// `cargo run` would never show any.
#[cfg(target_os = "windows")]
fn windows_app_id() -> Option<&'static str> {
    let programs: Vec<std::path::PathBuf> = ["APPDATA", "PROGRAMDATA"]
        .iter()
        .filter_map(|var| std::env::var_os(var))
        .map(|base| {
            std::path::PathBuf::from(base)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        })
        .collect();
    shortcut_candidates(&programs)
        .iter()
        .any(|path| path.exists())
        .then_some(WINDOWS_APP_ID)
}

fn show(notice: &Notice) {
    let mut notification = notify_rust::Notification::new();
    notification
        .appname("BGMail")
        .summary(&notice.summary)
        .body(&notice.body);
    #[cfg(target_os = "windows")]
    if let Some(app_id) = windows_app_id() {
        notification.app_id(app_id);
    }
    if let Err(error) = notification.show() {
        eprintln!("BGMail: notification failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(sender: &str, subject: &str) -> NewMailNotice {
        NewMailNotice {
            message_id: 1,
            sender: sender.into(),
            subject: subject.into(),
        }
    }

    #[test]
    fn no_messages_no_notices() {
        assert!(notices_for("Work", &[], Language::English).is_empty());
    }

    #[test]
    fn a_few_messages_get_one_notice_each() {
        let notices = notices_for(
            "Work",
            &[notice("Ana", "Lunch?"), notice("", "")],
            Language::Portuguese,
        );
        assert_eq!(
            notices,
            vec![
                Notice {
                    summary: "Ana".into(),
                    body: "Lunch?".into()
                },
                Notice {
                    summary: "Work".into(),
                    body: "(sem assunto)".into()
                },
            ]
        );
    }

    #[test]
    fn shortcut_candidates_cover_both_start_menu_layouts() {
        let programs = std::path::PathBuf::from("Programs");
        assert_eq!(
            shortcut_candidates(std::slice::from_ref(&programs)),
            vec![
                programs.join("BGMail.lnk"),
                programs.join("BGMail").join("BGMail.lnk")
            ]
        );
        assert_eq!(WINDOWS_APP_ID, "io.github.belizariogr.bgmail");
    }

    #[test]
    fn bursts_are_summarized() {
        let messages: Vec<_> = (0..5).map(|_| notice("A", "B")).collect();
        assert_eq!(
            notices_for("Work", &messages, Language::English),
            vec![Notice {
                summary: "5 new messages".into(),
                body: "Work".into()
            }]
        );
    }
}
