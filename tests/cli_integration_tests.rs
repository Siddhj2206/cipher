use std::process::Command;

fn cipher_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cipher"))
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

#[test]
fn init_creates_book_structure() {
    let dir = temp_dir();
    let book_path = dir.path().join("test-book");

    let output = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("Failed to run cipher init");

    assert!(
        output.status.success(),
        "init failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        book_path.join("cipher.toml").exists(),
        "cipher.toml missing"
    );
    assert!(book_path.join("raw").exists(), "raw/ missing");
    assert!(book_path.join("tl").exists(), "tl/ missing");
    assert!(
        book_path.join("glossary.json").exists(),
        "glossary.json missing"
    );
    assert!(book_path.join("style.md").exists(), "style.md missing");
}

#[test]
fn init_on_existing_dir_does_not_fail() {
    let dir = temp_dir();
    let book_path = dir.path().join("another-book");

    let first = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("First init failed");
    assert!(first.status.success());

    let second = cipher_binary()
        .arg("init")
        .arg(&book_path)
        .output()
        .expect("Second init failed");
    assert!(
        second.status.success(),
        "re-init should succeed: {:?}",
        String::from_utf8_lossy(&second.stderr)
    );
}

#[test]
fn doctor_without_book_succeeds() {
    let output = cipher_binary()
        .arg("doctor")
        .output()
        .expect("Failed to run cipher doctor");

    assert!(
        output.status.success(),
        "doctor failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn status_on_nonexistent_dir_does_not_crash() {
    let dir = temp_dir();
    let book_path = dir.path().join("nonexistent-book");

    let output = cipher_binary()
        .arg("status")
        .arg(&book_path)
        .output()
        .expect("Failed to run cipher status");

    assert!(
        output.status.success(),
        "status should not crash on nonexistent dir"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No translation runs recorded yet"),
        "status should indicate no runs, got: {stdout}"
    );
}
