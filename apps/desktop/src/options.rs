//! Command-line options.

use std::path::PathBuf;

use rustlite_paint::Size;

pub const USAGE: &str = "\
Usage: rustlite [OPTIONS] [URL_OR_FILE]

Opens URL_OR_FILE (an http(s) URL or a local HTML file) if given, otherwise
about:blank with the address bar focused.

Options:
  --size <WxH>              Initial window size in physical pixels (default 1024x768)
  --headless <PNG>          Load URL_OR_FILE, render one frame without a window, write PNG
  --exit-after-frames <N>   Close the window after presenting N frames (smoke testing)
  --dump-dom <HTML_FILE>    Parse a local HTML file and print its DOM tree (html5lib format)
  --dump-style <HTML_FILE>  Print computed styles of a local HTML file (viewport = --size)
  --dump-layout <HTML_FILE> Print element border boxes after layout (viewport = --size)
  --debug-overlay           Start with the paint debug overlay enabled
  --disable-javascript      Do not run page scripts
  -h, --help                Print this help
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub size: Size,
    pub headless_output: Option<PathBuf>,
    pub dump_dom: Option<PathBuf>,
    pub dump_style: Option<PathBuf>,
    pub dump_layout: Option<PathBuf>,
    pub exit_after_frames: Option<u64>,
    pub debug_overlay: bool,
    /// Run page scripts (`--disable-javascript` clears it).
    pub javascript: bool,
    pub help: bool,
    /// Local HTML file to open (named explicitly by the user).
    pub open: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            size: Size::new(1024, 768),
            headless_output: None,
            dump_dom: None,
            dump_style: None,
            dump_layout: None,
            exit_after_frames: None,
            debug_overlay: false,
            javascript: true,
            help: false,
            open: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OptionsError {
    #[error("unknown option `{0}`")]
    Unknown(String),
    #[error("option `{0}` requires a value")]
    MissingValue(&'static str),
    #[error("invalid value `{value}` for `{option}`: {reason}")]
    Invalid {
        option: &'static str,
        value: String,
        reason: &'static str,
    },
}

/// Maximum accepted dimension; keeps a headless framebuffer under ~256 MiB.
const MAX_DIMENSION: u32 = 8192;

impl Options {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, OptionsError> {
        let mut opts = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => opts.help = true,
                "--debug-overlay" => opts.debug_overlay = true,
                "--disable-javascript" => opts.javascript = false,
                "--size" => {
                    let value = args.next().ok_or(OptionsError::MissingValue("--size"))?;
                    opts.size = parse_size(&value)?;
                }
                "--headless" => {
                    let value = args
                        .next()
                        .ok_or(OptionsError::MissingValue("--headless"))?;
                    opts.headless_output = Some(PathBuf::from(value));
                }
                "--dump-dom" => {
                    let value = args
                        .next()
                        .ok_or(OptionsError::MissingValue("--dump-dom"))?;
                    opts.dump_dom = Some(PathBuf::from(value));
                }
                "--dump-style" => {
                    let value = args
                        .next()
                        .ok_or(OptionsError::MissingValue("--dump-style"))?;
                    opts.dump_style = Some(PathBuf::from(value));
                }
                "--dump-layout" => {
                    let value = args
                        .next()
                        .ok_or(OptionsError::MissingValue("--dump-layout"))?;
                    opts.dump_layout = Some(PathBuf::from(value));
                }
                "--exit-after-frames" => {
                    let value = args
                        .next()
                        .ok_or(OptionsError::MissingValue("--exit-after-frames"))?;
                    let n = value
                        .parse::<u64>()
                        .ok()
                        .filter(|&n| n > 0)
                        .ok_or_else(|| OptionsError::Invalid {
                            option: "--exit-after-frames",
                            value: value.clone(),
                            reason: "expected a positive integer",
                        })?;
                    opts.exit_after_frames = Some(n);
                }
                _ if !arg.starts_with('-') && opts.open.is_none() => {
                    opts.open = Some(PathBuf::from(arg));
                }
                _ => return Err(OptionsError::Unknown(arg)),
            }
        }
        Ok(opts)
    }
}

fn parse_size(value: &str) -> Result<Size, OptionsError> {
    let invalid = |reason| OptionsError::Invalid {
        option: "--size",
        value: value.to_owned(),
        reason,
    };
    let (w, h) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| invalid("expected WIDTHxHEIGHT"))?;
    let parse = |s: &str| {
        s.parse::<u32>()
            .ok()
            .filter(|&n| (1..=MAX_DIMENSION).contains(&n))
            .ok_or_else(|| invalid("dimensions must be between 1 and 8192"))
    };
    Ok(Size::new(parse(w)?, parse(h)?))
}

/// The navigation input for a command-line argument: an existing local
/// file (relative paths included) becomes its absolute path, so
/// `rustlite tests/pages/sample.html` opens the file rather than the host
/// `tests`. Anything else (URLs, host names) is passed through as typed.
/// Only the command line does this; the address bar never probes the disk.
pub fn command_line_input(arg: &std::path::Path) -> String {
    if arg.is_file()
        && let Ok(absolute) = std::fs::canonicalize(arg)
    {
        let text = absolute.to_string_lossy().into_owned();
        // Windows canonical paths carry a verbatim prefix (`\\?\C:\...`).
        return text
            .strip_prefix(r"\\?\")
            .map_or(text.clone(), str::to_owned);
    }
    arg.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, OptionsError> {
        Options::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        assert_eq!(parse(&[]).unwrap(), Options::default());
    }

    #[test]
    fn all_options() {
        let o = parse(&[
            "--size",
            "640x480",
            "--headless",
            "out.png",
            "--exit-after-frames",
            "3",
            "--debug-overlay",
            "--disable-javascript",
        ])
        .unwrap();
        assert!(!o.javascript);
        assert!(Options::default().javascript);
        assert_eq!(o.size, Size::new(640, 480));
        assert_eq!(o.headless_output, Some(PathBuf::from("out.png")));
        assert_eq!(o.exit_after_frames, Some(3));
        assert!(o.debug_overlay);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(matches!(
            parse(&["--size", "0x10"]),
            Err(OptionsError::Invalid { .. })
        ));
        assert!(matches!(
            parse(&["--size", "99999x10"]),
            Err(OptionsError::Invalid { .. })
        ));
        assert!(matches!(
            parse(&["--size", "abc"]),
            Err(OptionsError::Invalid { .. })
        ));
        assert!(matches!(
            parse(&["--exit-after-frames", "0"]),
            Err(OptionsError::Invalid { .. })
        ));
        assert_eq!(
            parse(&["--size"]),
            Err(OptionsError::MissingValue("--size"))
        );
        assert_eq!(
            parse(&["--bogus"]),
            Err(OptionsError::Unknown("--bogus".into()))
        );
    }

    #[test]
    fn command_line_files_become_absolute_paths() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let file = manifest.join("Cargo.toml");
        let relative = pathdiff_from_cwd(&file);
        let input = command_line_input(&relative);
        assert!(std::path::Path::new(&input).is_absolute(), "{input}");
        assert!(!input.starts_with(r"\\?\"));
        assert_eq!(
            command_line_input(std::path::Path::new("example.com")),
            "example.com"
        );
        assert_eq!(
            command_line_input(std::path::Path::new("https://a.test/x")),
            "https://a.test/x"
        );
    }

    /// `path` relative to the current directory when possible.
    fn pathdiff_from_cwd(path: &std::path::Path) -> std::path::PathBuf {
        let cwd = std::env::current_dir().unwrap();
        path.strip_prefix(&cwd)
            .map_or_else(|_| path.to_path_buf(), std::path::Path::to_path_buf)
    }
}
