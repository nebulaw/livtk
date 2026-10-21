//! How livtk says things to people, the same in the CLI and the app.

/// A length of time as people say it: `3d 4h`, `5h 20m`, `12m`, `40s`.
pub fn duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{secs}s"),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

/// A Claude Code session id as people read it: its first eight characters.
pub fn session(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// A claim of earlier events for a session, as one phrase.
pub fn claim(data: &crate::event::SessionData) -> String {
    format!(
        "{} earlier event(s) credited to session {}",
        data.events.len(),
        session(&data.session)
    )
}

#[cfg(test)]
mod tests {
    use super::duration;

    #[test]
    fn durations_read_as_people_say_them() {
        assert_eq!(duration(40), "40s");
        assert_eq!(duration(12 * 60 + 5), "12m");
        assert_eq!(duration(5 * 3600 + 20 * 60), "5h 20m");
        assert_eq!(duration(3 * 86_400 + 4 * 3600 + 59), "3d 4h");
        assert_eq!(duration(-5), "0s", "never negative");
    }
}
