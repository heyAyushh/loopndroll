use super::output::OutputFormat;

pub(crate) const OFF_VALUE: &str = "off";
pub(crate) const WAIT_FOR_REPLY_FLAG: &str = "--wait-for-reply";
pub(crate) const WAIT_FOR_UPDATES_FLAG: &str = "--wait";

const TABLE_FORMAT_FLAG: &str = "--format";
const TABLE_FORMAT_VALUE: &str = "table";
const JSON_FORMAT_FLAG: &str = "--json";
const TABLE_SHORTCUT_FLAG: &str = "--table";

pub(crate) fn parse_global_args(args: Vec<String>) -> (OutputFormat, Vec<String>) {
    let mut format = OutputFormat::Json;
    let mut filtered = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == JSON_FORMAT_FLAG {
            format = OutputFormat::Json;
            continue;
        }
        if arg == TABLE_SHORTCUT_FLAG {
            format = OutputFormat::Table;
            continue;
        }
        if arg == TABLE_FORMAT_FLAG {
            if matches!(iter.next().as_deref(), Some(TABLE_FORMAT_VALUE)) {
                format = OutputFormat::Table;
            }
            continue;
        }
        filtered.push(arg);
    }
    (format, filtered)
}

pub(crate) fn take_arg(args: &mut Vec<String>) -> Option<String> {
    if args.is_empty() {
        None
    } else {
        Some(args.remove(0))
    }
}

pub(crate) fn joined_args(args: &[String]) -> String {
    args.join(" ")
}

pub(crate) fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            (!item.is_empty()).then(|| item.to_owned())
        })
        .collect()
}

pub(crate) fn nullable_id(value: &str) -> Option<&str> {
    if value == OFF_VALUE {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_table_format_is_removed_from_args() {
        let (format, args) = parse_global_args(vec![
            "--format".to_owned(),
            "table".to_owned(),
            "connections".to_owned(),
            "list".to_owned(),
        ]);
        assert_eq!(format, OutputFormat::Table);
        assert_eq!(args, vec!["connections", "list"]);
    }

    #[test]
    fn global_json_format_is_removed_from_args() {
        let (format, args) = parse_global_args(vec![
            "--json".to_owned(),
            "pairing".to_owned(),
            "code".to_owned(),
        ]);
        assert_eq!(format, OutputFormat::Json);
        assert_eq!(args, vec!["pairing", "code"]);
    }

    #[test]
    fn csv_ids_skip_empty_values() {
        assert_eq!(
            split_csv("route-1, ,route-2"),
            vec!["route-1".to_owned(), "route-2".to_owned()]
        );
    }

    #[test]
    fn off_value_maps_to_null() {
        assert_eq!(nullable_id("off"), None);
        assert_eq!(nullable_id("max-turns-1"), Some("max-turns-1"));
    }
}
