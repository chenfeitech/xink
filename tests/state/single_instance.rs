use super::*;

#[test]
fn current_process_is_alive() {
    assert!(process_alive(std::process::id() as i32));
}

#[test]
fn nonexistent_process_is_not_alive() {
    // 取一个几乎不可能存在的 pid
    assert!(!process_alive(999_999));
}

#[test]
fn invalid_pid_is_not_alive() {
    assert!(!process_alive(0));
    assert!(!process_alive(-1));
}

#[test]
fn pid_file_name_is_recognized() {
    assert!(is_pid_file(Path::new("/tmp/xink.pid")));
    assert!(!is_pid_file(Path::new("/tmp/other.pid")));
}
