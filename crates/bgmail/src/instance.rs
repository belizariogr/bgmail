//! One running BGMail receives what later launches are asked to open: a second
//! process started for a `mailto:` link hands it over and exits, so two
//! processes never fight over the mail database.
//!
//! Loopback TCP keeps this one portable code path (std has no Windows local
//! sockets). A private file holds the port and a random token; connections
//! without the token are ignored. All a message can do is prefill the compose
//! window, which still waits for the user to press Send.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Longest message accepted; browsers cap their URLs well below this.
const MAX_MESSAGE_BYTES: u64 = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// The first `mailto:` link among the process arguments.
pub fn mailto_argument(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().find(|arg| {
        arg.get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
    })
}

/// File through which later launches find the running instance.
fn instance_file() -> Option<PathBuf> {
    Some(crate::config::config_path()?.with_file_name("instance"))
}

/// Hands `message` to the running instance. `false`: there is none (no file, or
/// a stale one), so this process has to handle it.
pub fn forward(message: &str) -> bool {
    instance_file().is_some_and(|file| forward_through(&file, message).is_some())
}

fn forward_through(file: &Path, message: &str) -> Option<()> {
    let contents = std::fs::read_to_string(file).ok()?;
    let (port, token) = contents.trim().split_once(' ')?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port.parse().ok()?));
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).ok()?;
    stream
        .write_all(format!("{token}\n{message}").as_bytes())
        .ok()?;
    stream.shutdown(std::net::Shutdown::Write).ok()?;
    // The listener closes without answering a wrong token, and acknowledges a
    // right one: a stale port now owned by another program doesn't count.
    let mut ack = [0u8; 1];
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
    (stream.read(&mut ack).ok()? == 1).then_some(())
}

/// Starts listening off the startup path; `on_message` runs on a background
/// thread for every message. Failing only means later launches open their own
/// window, as they would without this.
pub fn listen(on_message: impl Fn(String) + Send + 'static) {
    let Some(file) = instance_file() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("bgmail-instance".into())
        .spawn(move || {
            if let Err(error) = serve(&file, on_message) {
                eprintln!("BGMail: not listening for other launches: {error}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("BGMail: not listening for other launches: {error}");
    }
}

fn serve(file: &Path, on_message: impl Fn(String)) -> std::io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let token = new_token();
    write_private(file, &format!("{} {token}", listener.local_addr()?.port()))?;
    for stream in listener.incoming().flatten() {
        if let Some(message) = read_message(stream, &token) {
            on_message(message);
        }
    }
    Ok(())
}

/// The message of a connection that knows the token, acknowledged.
fn read_message(mut stream: TcpStream, token: &str) -> Option<String> {
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
    let mut received = String::new();
    (&mut stream)
        .take(MAX_MESSAGE_BYTES)
        .read_to_string(&mut received)
        .ok()?;
    let (sent_token, message) = received.split_once('\n')?;
    if sent_token != token {
        return None;
    }
    stream.write_all(b"1").ok()?;
    Some(message.to_string())
}

/// 128 random bits, from the OS-seeded keys of the standard library's hasher.
fn new_token() -> String {
    let random = || RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", random(), random())
}

/// Writes `contents` readable by this user only, replacing the file at once.
fn write_private(file: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = file.with_extension(format!("tmp{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&temporary)?.write_all(contents.as_bytes())?;
    std::fs::rename(&temporary, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn finds_the_mailto_argument() {
        let args = |list: &[&str]| list.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        assert_eq!(
            mailto_argument(args(&["bgmail", "MAILTO:a@b.c"])).as_deref(),
            Some("MAILTO:a@b.c")
        );
        assert_eq!(mailto_argument(args(&["bgmail", "--x", "http://a"])), None);
        assert_eq!(mailto_argument(args(&["ç", "mail"])), None);
    }

    /// Serves on a private file and returns it with the received messages.
    fn serving() -> (
        tempfile::TempDir,
        PathBuf,
        std::sync::mpsc::Receiver<String>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("instance");
        let (tx, rx) = channel();
        let served = file.clone();
        std::thread::spawn(move || {
            serve(&served, move |message| tx.send(message).unwrap()).unwrap();
        });
        for _ in 0..200 {
            if file.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        (dir, file, rx)
    }

    #[test]
    fn a_later_launch_reaches_the_running_instance() {
        let (_dir, file, rx) = serving();
        assert!(forward_through(&file, "mailto:a@b.c?subject=1%0A2").is_some());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "mailto:a@b.c?subject=1%0A2"
        );
    }

    #[test]
    fn a_wrong_token_is_ignored() {
        let (_dir, file, rx) = serving();
        let contents = std::fs::read_to_string(&file).unwrap();
        let (port, _) = contents.split_once(' ').unwrap();
        std::fs::write(&file, format!("{port} wrong")).unwrap();
        assert!(forward_through(&file, "mailto:a@b.c").is_none());
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn no_instance_means_handling_it_here() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("instance");
        assert!(forward_through(&file, "x").is_none());
        // A port nobody listens on anymore.
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(&file, format!("{port} token")).unwrap();
        assert!(forward_through(&file, "x").is_none());
        std::fs::write(&file, "garbage").unwrap();
        assert!(forward_through(&file, "x").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_token_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, file, _rx) = serving();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(new_token(), new_token());
        assert_eq!(new_token().len(), 32);
    }
}
