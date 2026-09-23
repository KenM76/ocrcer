//! Saying which *build* produced a number, not only which model file it read.
//!
//! # Why this exists
//!
//! A benchmark line names the model file and the corpus, and a reader
//! reasonably takes those two to identify the run. They do not. The third
//! input is the engine binary, and it is the one that goes stale silently:
//! the bench binaries sit behind `required-features`, so a `cargo build -p`
//! that omits them finishes cleanly, prints nothing, and leaves a reader on
//! disk that is older than the model file it is about to load.
//!
//! That is not a hypothetical. A model file carrying a newly added parameter
//! was read by a binary that predated it; the row was dropped, half a
//! two-parameter change applied, and the resulting configuration — which
//! nobody had ever authored — measured as a clean 12% relative CER
//! regression. The numbers were self-consistent and entirely false.
//!
//! # Contract
//!
//! [`line`] returns a one-line provenance string naming the binary's build
//! time and the model file's, and **says so loudly when the binary is the
//! older of the two**. It never fails: a filesystem that will not report an
//! mtime yields `unknown`, which is still more than the previous silence.
//!
//! The comparison is a heuristic and is deliberately stated as one. A binary
//! newer than the model file can still be stale with respect to a source edit
//! that was never compiled, and a binary older than the model file is fine
//! when the model was merely rewritten from unchanged inputs. It catches the
//! case that actually occurred, and it puts the two timestamps in front of a
//! reader who can judge the rest.

use std::path::Path;
use std::time::SystemTime;

/// The provenance line for a run that loaded `model`.
///
/// Print it beside the `model` line. `stale` in the output means the engine
/// binary is older than the model file, which is the shape of the failure
/// described above; it is a warning to check, not a proof of error.
pub fn line(model: &Path) -> String {
    let exe = std::env::current_exe().ok();
    let exe_t = exe.as_deref().and_then(mtime);
    let model_t = mtime(model);
    let flag = match (exe_t, model_t) {
        (Some(e), Some(m)) if e < m => {
            "  ** STALE: this binary is older than the model file it just read. \
             A parameter the file carries and this build does not know is dropped \
             silently, so any figure below may be measuring a configuration nobody \
             authored. Rebuild with --features vs-ocrs,pages and re-run. **"
        }
        _ => "",
    };
    format!(
        "built   engine binary {}, model file {}{}",
        stamp(exe_t),
        stamp(model_t),
        flag
    )
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

/// Seconds since the Unix epoch, or `unknown`.
///
/// Deliberately not a calendar date: formatting one needs either a
/// dependency or a civil-calendar routine, and neither is worth it for a
/// field whose only job is to be compared against the field beside it.
fn stamp(t: Option<SystemTime>) -> String {
    match t.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()) {
        Some(d) => format!("t={}", d.as_secs()),
        None => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the module: the older-binary case must be visible
    /// in the output, not merely representable.
    #[test]
    fn a_binary_older_than_its_model_is_called_out() {
        let dir = std::env::temp_dir().join("ocrcer-provenance-test");
        let _ = std::fs::create_dir_all(&dir);
        let model = dir.join("newer.ocrw");
        std::fs::write(&model, b"x").expect("the scratch file writes");
        // `current_exe` is the test harness, built before this file was
        // written a moment ago, so the stale branch is the one taken.
        let s = line(&model);
        assert!(s.contains("STALE"), "{s}");
        let _ = std::fs::remove_file(&model);
    }

    #[test]
    fn an_unreadable_path_still_produces_a_line() {
        let s = line(Path::new("no/such/model.ocrw"));
        assert!(s.starts_with("built   "), "{s}");
        assert!(s.contains("unknown"), "{s}");
    }
}
