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

fn show(notice: &Notice) {
    let result = notify_rust::Notification::new()
        .appname("BGMail")
        .summary(&notice.summary)
        .body(&notice.body)
        .show();
    if let Err(error) = result {
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
