//! The wall clock a line of the log is told apart by.
//!
//! `bateri.log` is the application's standard error, and most of its lines carry no time: a line
//! about something that went wrong while the person was away says *what* and not *when*, and two
//! lines of the same kind cannot be matched with what the person remembers. The lines that
//! matter for that begin with the local time as `11:03:27` ([`stamp`]); the others are left as
//! they were.

/// The local time as `11:03:27`, a line's first word. Where the system cannot say what the local
/// time is, the seconds since the epoch (a number is still something to sort lines by).
pub fn stamp() -> String {
    // SAFETY: `time` with a null pointer only returns the time.
    let seconds = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY: `tm` is plain data; all zeroes is a valid value of it.
    let mut broken: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `localtime_r` reads `seconds` and writes only into `broken`, both this function's.
    if unsafe { libc::localtime_r(&raw const seconds, &raw mut broken) }.is_null() {
        return seconds.to_string();
    }
    clock(broken.tm_hour, broken.tm_min, broken.tm_sec)
}

/// `11:03:27`.
fn clock(hour: i32, minute: i32, second: i32) -> String {
    format!("{hour:02}:{minute:02}:{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_two_digits_of_each() {
        assert_eq!(clock(11, 3, 27), "11:03:27");
        assert_eq!(clock(0, 0, 0), "00:00:00");
        assert_eq!(clock(23, 59, 59), "23:59:59");
    }

    #[test]
    fn the_stamp_is_the_time_of_day() {
        let stamp = stamp();
        let parts: Vec<&str> = stamp.split(':').collect();
        assert_eq!(parts.len(), 3, "{stamp}");
        assert!(
            parts
                .iter()
                .all(|part| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit())),
            "{stamp}"
        );
        assert!(
            parts[0] < "24" && parts[1] < "60" && parts[2] < "62",
            "{stamp}"
        );
    }
}
