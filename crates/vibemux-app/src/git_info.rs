use std::path::Path;

pub fn detect_git_branch(cwd: &str) -> Option<String> {
    let path = Path::new(cwd);
    let repo = git2::Repository::discover(path).ok()?;
    let head = repo.head().ok()?;
    let branch = head.shorthand()?;
    Some(branch.to_string())
}
