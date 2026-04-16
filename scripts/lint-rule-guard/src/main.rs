use std::collections::{BTreeSet, HashSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

const DEFAULT_EXCLUDES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "dist",
    "build",
    "coverage",
    "target",
];

const DIRECTIVES: &[&str] = &[
    "eslint-disable-next-line",
    "eslint-disable-line",
    "eslint-disable",
    "oxlint-disable-next-line",
    "oxlint-disable-line",
    "oxlint-disable",
];

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => ExitCode::from(exit_code),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let config = match ParsedArgs::parse(env::args().skip(1))? {
        ParsedArgs::Help => return Ok(0),
        ParsedArgs::Config(config) => config,
    };
    let scan_result = scan_paths(&config)?;
    print_report(&config, &scan_result);

    if scan_result.findings.is_empty() {
        Ok(0)
    } else {
        Ok(1)
    }
}

enum ParsedArgs {
    Help,
    Config(Config),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy)]
enum Color {
    Red,
    Yellow,
    Green,
    Cyan,
    Gray,
    Bold,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Mode {
    AnyRule,
    Rules(BTreeSet<String>),
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    roots: Vec<PathBuf>,
    includes: Vec<String>,
    excludes: Vec<String>,
    use_default_excludes: bool,
    format: OutputFormat,
}

impl ParsedArgs {
    fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut any_rule = false;
        let mut rules = BTreeSet::new();
        let mut roots = Vec::new();
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        let mut use_default_excludes = true;
        let mut format = OutputFormat::Text;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    print_usage();
                    return Ok(Self::Help);
                }
                "--any-rule" => any_rule = true,
                "--no-default-excludes" => use_default_excludes = false,
                "--include" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--include requires a value".to_string())?;
                    includes.push(normalize_cli_path(&value));
                }
                "--rule" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--rule requires a value".to_string())?;
                    add_rules(&mut rules, &value);
                }
                "--exclude" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--exclude requires a value".to_string())?;
                    excludes.push(normalize_cli_path(&value));
                }
                "--format" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--format requires a value".to_string())?;
                    format = match value.as_str() {
                        "text" => OutputFormat::Text,
                        "json" => OutputFormat::Json,
                        _ => return Err(format!("unsupported format: {value}")),
                    };
                }
                _ if arg.starts_with("--rule=") => {
                    add_rules(&mut rules, &arg["--rule=".len()..]);
                }
                _ if arg.starts_with("--include=") => {
                    includes.push(normalize_cli_path(&arg["--include=".len()..]));
                }
                _ if arg.starts_with("--exclude=") => {
                    excludes.push(normalize_cli_path(&arg["--exclude=".len()..]));
                }
                _ if arg.starts_with("--format=") => {
                    let value = &arg["--format=".len()..];
                    format = match value {
                        "text" => OutputFormat::Text,
                        "json" => OutputFormat::Json,
                        _ => return Err(format!("unsupported format: {value}")),
                    };
                }
                _ if arg.starts_with('-') => {
                    return Err(format!("unknown flag: {arg}"));
                }
                _ => roots.push(PathBuf::from(arg)),
            }
        }

        if any_rule && !rules.is_empty() {
            return Err("use either --any-rule or one or more --rule flags".to_string());
        }

        let mode = if any_rule {
            Mode::AnyRule
        } else if rules.is_empty() {
            return Err("expected --any-rule or at least one --rule <name>".to_string());
        } else {
            Mode::Rules(rules)
        };

        if roots.is_empty() {
            roots.push(PathBuf::from("."));
        }

        Ok(Self::Config(Config {
            mode,
            roots,
            includes,
            excludes,
            use_default_excludes,
            format,
        }))
    }
}

#[derive(Debug)]
struct ScanResult {
    scanned_files: usize,
    matched_files: usize,
    findings: Vec<Finding>,
}

#[derive(Debug)]
struct Finding {
    path: String,
    line: usize,
    directive: String,
    disabled_rules: Vec<String>,
    matched_rules: Vec<String>,
}

#[derive(Debug)]
struct ParsedDirective {
    directive: String,
    disabled_rules: Vec<String>,
}

fn add_rules(rules: &mut BTreeSet<String>, value: &str) {
    for candidate in value.split(',') {
        let rule = candidate.trim().to_ascii_lowercase();
        if !rule.is_empty() {
            rules.insert(rule);
        }
    }
}

fn print_usage() {
    println!(
        "lint-rule-guard\n\
         \n\
         Usage:\n\
           lint-rule-guard --any-rule [options] [paths...]\n\
           lint-rule-guard --rule <name> [--rule <name> ...] [options] [paths...]\n\
         \n\
         Options:\n\
           --rule <name>          Rule name to fail on when disabled. Repeatable.\n\
           --any-rule             Fail on any disable directive.\n\
           --include <path>       Force-include a path or folder prefix. Repeatable.\n\
           --exclude <path>       Skip a path or folder prefix. Repeatable.\n\
           --no-default-excludes  Do not apply built-in excludes.\n\
           --format <text|json>   Report output format.\n\
           -h, --help             Show this help.\n\
         \n\
         Exit codes:\n\
           0 no matches found\n\
           1 matching disabled-lint directives found\n\
           2 invalid arguments or runtime error"
    );
}

fn use_color() -> bool {
    env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}

fn paint(text: &str, color: Color, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }

    let code = match color {
        Color::Red => "31",
        Color::Yellow => "33",
        Color::Green => "32",
        Color::Cyan => "36",
        Color::Gray => "90",
        Color::Bold => "1",
    };

    format!("\x1b[{code}m{text}\x1b[0m")
}

fn scan_paths(config: &Config) -> Result<ScanResult, String> {
    let mut files = Vec::new();
    for root in &config.roots {
        collect_files(root, config, &mut files)?;
    }

    files.sort();
    files.dedup();

    let mut findings = Vec::new();
    for path in &files {
        let Some(file_findings) = scan_file(path, config)? else {
            continue;
        };
        findings.extend(file_findings);
    }

    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.directive.cmp(&right.directive))
    });

    let matched_files = findings
        .iter()
        .map(|finding| finding.path.as_str())
        .collect::<HashSet<_>>()
        .len();

    Ok(ScanResult {
        scanned_files: files.len(),
        matched_files,
        findings,
    })
}

fn collect_files(path: &Path, config: &Config, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if should_exclude(path, config) {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to read metadata for {}: {error}", path.display()))?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }

    if metadata.is_dir() {
        let entries = fs::read_dir(path)
            .map_err(|error| format!("failed to read directory {}: {error}", path.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("failed to read a directory entry in {}: {error}", path.display())
            })?;
            collect_files(&entry.path(), config, files)?;
        }
    }

    Ok(())
}

fn should_exclude(path: &Path, config: &Config) -> bool {
    let normalized = normalize_path_for_matching(path);
    let components = normalized_path_components(path);

    if should_include(&normalized, &config.includes) {
        return false;
    }

    if config.use_default_excludes
        && components
            .iter()
            .any(|component| DEFAULT_EXCLUDES.iter().any(|value| value == component))
    {
        return true;
    }

    config.excludes.iter().any(|exclude| {
        normalized == *exclude
            || normalized.starts_with(&format!("{exclude}/"))
            || components.iter().any(|component| component == exclude)
    })
}

fn should_include(normalized_path: &str, includes: &[String]) -> bool {
    includes.iter().any(|include| {
        normalized_path == include
            || normalized_path.starts_with(&format!("{include}/"))
            || include.starts_with(&format!("{normalized_path}/"))
    })
}

fn normalize_path_for_matching(path: &Path) -> String {
    let display_path = if let Ok(current_dir) = env::current_dir() {
        path.strip_prefix(&current_dir).unwrap_or(path)
    } else {
        path
    };

    normalize_cli_path(&display_path.to_string_lossy())
}

fn normalize_cli_path(value: &str) -> String {
    let normalized = value
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string();

    if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized
    }
}

fn normalized_path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

fn scan_file(path: &Path, config: &Config) -> Result<Option<Vec<Finding>>, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

    if bytes.contains(&0) {
        return Ok(None);
    }

    let contents = match String::from_utf8(bytes) {
        Ok(contents) => contents,
        Err(_) => return Ok(None),
    };

    let normalized_path = normalize_path_for_matching(path);
    let mut findings = Vec::new();

    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        for directive in parse_line(line) {
            let matched_rules = match &config.mode {
                Mode::AnyRule => {
                    if directive.disabled_rules.is_empty() {
                        vec!["<all-rules>".to_string()]
                    } else {
                        directive.disabled_rules.clone()
                    }
                }
                Mode::Rules(requested_rules) => {
                    if directive.disabled_rules.is_empty() {
                        requested_rules.iter().cloned().collect()
                    } else {
                        directive
                            .disabled_rules
                            .iter()
                            .filter(|rule| requested_rules.contains(*rule))
                            .cloned()
                            .collect()
                    }
                }
            };

            if matched_rules.is_empty() {
                continue;
            }

            findings.push(Finding {
                path: normalized_path.clone(),
                line: line_number,
                directive: directive.directive,
                disabled_rules: directive.disabled_rules,
                matched_rules,
            });
        }
    }

    if findings.is_empty() {
        Ok(None)
    } else {
        Ok(Some(findings))
    }
}

fn parse_line(line: &str) -> Vec<ParsedDirective> {
    let mut results = Vec::new();
    let mut offset = 0;

    while offset < line.len() {
        let Some((position, directive)) = next_directive(&line[offset..]) else {
            break;
        };

        let absolute_position = offset + position;
        if !looks_like_comment_context(line, absolute_position) {
            offset = absolute_position + directive.len();
            continue;
        }

        let remainder = &line[absolute_position + directive.len()..];
        results.push(ParsedDirective {
            directive: directive.to_string(),
            disabled_rules: parse_rules(remainder),
        });
        offset = absolute_position + directive.len();
    }

    results
}

fn next_directive(line: &str) -> Option<(usize, &'static str)> {
    let mut best_match: Option<(usize, &'static str)> = None;

    for directive in DIRECTIVES {
        let Some(index) = line.find(directive) else {
            continue;
        };

        match best_match {
            None => best_match = Some((index, *directive)),
            Some((best_index, best_directive)) => {
                if index < best_index
                    || (index == best_index && directive.len() > best_directive.len())
                {
                    best_match = Some((index, *directive));
                }
            }
        }
    }

    best_match
}

fn looks_like_comment_context(line: &str, directive_position: usize) -> bool {
    let before = &line[..directive_position];

    if let Some(index) = before.rfind("//") {
        let spacer = &before[index + 2..];
        if spacer.trim().is_empty() && has_valid_comment_boundary(before, index) {
            return true;
        }
    }

    if let Some(index) = before.rfind("/*") {
        let spacer = &before[index + 2..];
        if spacer.trim().is_empty() && has_valid_comment_boundary(before, index) {
            return true;
        }
    }

    before.trim_start() == "*"
}

fn has_valid_comment_boundary(before: &str, marker_index: usize) -> bool {
    if marker_index == 0 {
        return true;
    }

    let previous = before[..marker_index].chars().next_back().unwrap_or_default();
    !previous.is_ascii_alphanumeric() && !matches!(previous, '"' | '\'' | '`')
}

fn parse_rules(remainder: &str) -> Vec<String> {
    let cleaned = remainder
        .split("--")
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches("*/")
        .trim();

    if cleaned.is_empty() {
        return Vec::new();
    }

    cleaned
        .split(',')
        .filter_map(|part| {
            let rule = part
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            if rule.is_empty() {
                None
            } else {
                Some(rule)
            }
        })
        .collect()
}

fn print_report(config: &Config, scan_result: &ScanResult) {
    match config.format {
        OutputFormat::Text => print_text_report(config, scan_result),
        OutputFormat::Json => print_json_report(config, scan_result),
    }
}

fn print_text_report(config: &Config, scan_result: &ScanResult) {
    let colored = use_color();

    if scan_result.findings.is_empty() {
        match &config.mode {
            Mode::AnyRule => {
                println!(
                    "{} {} {}",
                    paint("✓", Color::Green, colored),
                    paint("No disabled lint rules found.", Color::Bold, colored),
                    paint(
                        &format!("({} files scanned)", scan_result.scanned_files),
                        Color::Gray,
                        colored
                    )
                );
            }
            Mode::Rules(rules) => {
                println!(
                    "{} {} {} {}",
                    paint("✓", Color::Green, colored),
                    paint("No matching disabled lint rules found.", Color::Bold, colored),
                    paint("Requested:", Color::Gray, colored),
                    rules.iter().cloned().collect::<Vec<_>>().join(", ")
                );
                println!(
                    "{}",
                    paint(
                        &format!("{} files scanned", scan_result.scanned_files),
                        Color::Gray,
                        colored
                    )
                );
            }
        }
        return;
    }

    let widest_location = scan_result
        .findings
        .iter()
        .map(|finding| format!("{}:{}", finding.path, finding.line).len())
        .max()
        .unwrap_or(0);

    match &config.mode {
        Mode::AnyRule => {
            println!(
                "{} {}",
                paint("✖", Color::Red, colored),
                paint(
                    &format!(
                        "Found {} disabled lint directive(s) across {} file(s)",
                        scan_result.findings.len(),
                        scan_result.matched_files
                    ),
                    Color::Bold,
                    colored
                )
            );
        }
        Mode::Rules(rules) => {
            println!(
                "{} {}",
                paint("✖", Color::Red, colored),
                paint(
                    &format!(
                        "Found {} matching disabled lint directive(s) across {} file(s)",
                        scan_result.findings.len(),
                        scan_result.matched_files
                    ),
                    Color::Bold,
                    colored
                )
            );
            println!(
                "{} {}",
                paint("Requested rules:", Color::Gray, colored),
                rules.iter().cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }

    println!(
        "{}",
        paint(
            "It's not allowed to ignore linter rules in this repository.",
            Color::Red,
            colored
        )
    );
    println!();

    for finding in &scan_result.findings {
        let location = format!("{}:{}", finding.path, finding.line);
        let disabled = if finding.disabled_rules.is_empty() {
            "<all-rules>".to_string()
        } else {
            finding.disabled_rules.join(", ")
        };

        if matches!(config.mode, Mode::AnyRule) {
            println!(
                "  {}  {}  {}  {}",
                format!("{location:<width$}", width = widest_location),
                paint(&finding.directive, Color::Yellow, colored),
                paint("disabled", Color::Gray, colored),
                disabled
            );
        } else {
            println!(
                "  {}  {}  {}  {}  {}  {}",
                format!("{location:<width$}", width = widest_location),
                paint(&finding.directive, Color::Yellow, colored),
                paint("disabled", Color::Gray, colored),
                disabled,
                paint("matched", Color::Cyan, colored),
                finding.matched_rules.join(", ")
            );
        }
    }

    println!();
    println!(
        "{}",
        paint(
            &format!(
                "{} problem(s), {} file(s) affected, {} file(s) scanned",
                scan_result.findings.len(),
                scan_result.matched_files,
                scan_result.scanned_files
            ),
            Color::Red,
            colored
        )
    );
}

fn print_json_report(config: &Config, scan_result: &ScanResult) {
    let mode = match &config.mode {
        Mode::AnyRule => "\"any-rule\"".to_string(),
        Mode::Rules(_) => "\"rules\"".to_string(),
    };

    let requested_rules = match &config.mode {
        Mode::AnyRule => "[]".to_string(),
        Mode::Rules(rules) => {
            let entries = rules
                .iter()
                .map(|rule| format!("\"{}\"", escape_json(rule)))
                .collect::<Vec<_>>()
                .join(",");
            format!("[{entries}]")
        }
    };

    let roots = config
        .roots
        .iter()
        .map(|root| format!("\"{}\"", escape_json(&normalize_path_for_matching(root))))
        .collect::<Vec<_>>()
        .join(",");

    let excludes = effective_excludes(config)
        .iter()
        .map(|exclude| format!("\"{}\"", escape_json(exclude)))
        .collect::<Vec<_>>()
        .join(",");
    let includes = config
        .includes
        .iter()
        .map(|include| format!("\"{}\"", escape_json(include)))
        .collect::<Vec<_>>()
        .join(",");

    let findings = scan_result
        .findings
        .iter()
        .map(|finding| {
            let disabled_rules = if finding.disabled_rules.is_empty() {
                String::from("[]")
            } else {
                format!(
                    "[{}]",
                    finding
                        .disabled_rules
                        .iter()
                        .map(|rule| format!("\"{}\"", escape_json(rule)))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };

            let matched_rules = format!(
                "[{}]",
                finding
                    .matched_rules
                    .iter()
                    .map(|rule| format!("\"{}\"", escape_json(rule)))
                    .collect::<Vec<_>>()
                    .join(",")
            );

            format!(
                "{{\"path\":\"{}\",\"line\":{},\"directive\":\"{}\",\"disabled_rules\":{},\"matched_rules\":{}}}",
                escape_json(&finding.path),
                finding.line,
                escape_json(&finding.directive),
                disabled_rules,
                matched_rules
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    println!(
        "{{\"mode\":{},\"requested_rules\":{},\"roots\":[{}],\"includes\":[{}],\"excludes\":[{}],\"summary\":{{\"scanned_files\":{},\"matched_files\":{},\"findings\":{}}},\"findings\":[{}]}}",
        mode,
        requested_rules,
        roots,
        includes,
        excludes,
        scan_result.scanned_files,
        scan_result.matched_files,
        scan_result.findings.len(),
        findings
    );
}

fn effective_excludes(config: &Config) -> Vec<String> {
    let mut excludes = Vec::new();
    if config.use_default_excludes {
        excludes.extend(DEFAULT_EXCLUDES.iter().map(|value| value.to_string()));
    }
    excludes.extend(config.excludes.iter().cloned());
    excludes.sort();
    excludes.dedup();
    excludes
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped
}
