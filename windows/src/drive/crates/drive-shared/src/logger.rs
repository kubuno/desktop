//! Port of `Files.Shared/Utils/Logger/FileLogger.cs`.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Log severity, mirroring `Microsoft.Extensions.Logging.LogLevel` names,
/// which appear verbatim in the log file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Information,
    Warning,
    Error,
    Critical,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            LogLevel::Trace => "Trace",
            LogLevel::Debug => "Debug",
            LogLevel::Information => "Information",
            LogLevel::Warning => "Warning",
            LogLevel::Error => "Error",
            LogLevel::Critical => "Critical",
        };
        f.write_str(name)
    }
}

/// Thread-safe append-only file logger.
/// Line format matches the C# logger: `yyyy-MM-dd HH:mm:ss.ffff|Level|message`.
pub struct FileLogger {
    file_path: PathBuf,
    sync_root: Mutex<()>,
}

impl FileLogger {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
            sync_root: Mutex::new(()),
        }
    }

    pub fn log(&self, level: LogLevel, message: &str) {
        use chrono::Timelike;
        let now = chrono::Local::now();
        // C# format `ffff` = 4 fractional digits (100µs units); chrono has no %.4f.
        let line = format!(
            "{}.{:04}|{}|{}\r\n",
            now.format("%Y-%m-%d %H:%M:%S"),
            now.nanosecond() % 1_000_000_000 / 100_000,
            level,
            message
        );

        let _guard = self.sync_root.lock().unwrap();
        let result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .and_then(|mut f| f.write_all(line.as_bytes()));
        if let Err(e) = result {
            tracing::debug!("Writing to log file failed with the following exception:\n{e}");
        }
    }

    /// Truncates the log file, keeping only the last `number_of_lines_kept` lines.
    pub fn purge_logs(&self, number_of_lines_kept: usize) {
        let _guard = self.sync_root.lock().unwrap();
        if !self.file_path.exists() {
            return;
        }

        let result = std::fs::read_to_string(&self.file_path).and_then(|content| {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() <= number_of_lines_kept {
                return Ok(());
            }
            let kept = &lines[lines.len() - number_of_lines_kept..];
            std::fs::write(&self.file_path, kept.join("\r\n") + "\r\n")
        });
        if let Err(e) = result {
            tracing::debug!("Purging the log file failed with the following exception:\n{e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_and_purge() {
        let dir = std::env::temp_dir().join("drive-shared-logger-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("debug.log");
        let _ = std::fs::remove_file(&path);

        let logger = FileLogger::new(&path);
        for i in 0..10 {
            logger.log(LogLevel::Information, &format!("message {i}"));
        }

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 10);
        assert!(content.lines().next().unwrap().contains("|Information|message 0"));

        logger.purge_logs(3);
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("message 7"));

        std::fs::remove_file(&path).unwrap();
    }
}
