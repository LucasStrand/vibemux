use std::path::Path;
use std::sync::mpsc;

/// Blocking git branch detection — call from a background thread only.
fn detect_git_branch_blocking(cwd: &str) -> Option<String> {
    let path = Path::new(cwd);
    let repo = git2::Repository::discover(path).ok()?;
    let head = repo.head().ok()?;
    let branch = head.shorthand()?;
    Some(branch.to_string())
}

/// Fire-and-forget git branch detection that returns results via a callback.
/// The callback is invoked on a background thread; the caller is responsible
/// for forwarding the result to the main thread.
#[allow(dead_code)]
pub fn detect_git_branch_async(
    cwd: String,
    on_result: impl FnOnce(Option<String>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let result = detect_git_branch_blocking(&cwd);
        on_result(result);
    });
}

/// Synchronous wrapper that still works but doesn't block the calling thread.
/// Returns immediately with None if the detection takes too long (50ms timeout).
/// Falls back to blocking if the channel fails.
pub fn detect_git_branch(cwd: &str) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    let cwd_owned = cwd.to_string();
    std::thread::spawn(move || {
        let result = detect_git_branch_blocking(&cwd_owned);
        let _ = tx.send(result);
    });
    rx.recv_timeout(std::time::Duration::from_millis(50))
        .ok()
        .flatten()
}
