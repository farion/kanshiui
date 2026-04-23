use std::process::Command;

pub fn notify_profile(profile_name: &str) {
    let _ = Command::new("notify-send").arg(profile_name).status();
}
