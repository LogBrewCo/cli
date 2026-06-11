//! Log-read shortcut argument normalization.

use super::{
    is_ambiguous_log_search_word, is_known_log_level, is_log_search_shortcut, is_read_verb,
    is_recency_read_verb, move_leading_json_to_tail,
};

/// Rewrites natural log shortcuts to explicit read filters.
pub(super) fn log_shortcut_args(args: &[String]) -> Vec<String> {
    let normalized = move_leading_json_to_tail(args);
    if let Some((search, rest)) = split_separator_log_search_tail(normalized.as_slice()) {
        return build_log_shortcut_args(None, Some(search), rest, normalized.len());
    }
    let explicit = log_explicit_filter_shortcut_args(normalized.as_slice());
    if explicit != normalized {
        return explicit;
    }
    if let Some((level, tail)) = normalized
        .split_first()
        .filter(|(level, tail)| is_log_level_shortcut_candidate(level, tail))
    {
        let (search, rest) = split_natural_log_search_tail(tail, true);
        return build_log_shortcut_args(Some(level.clone()), search, rest, normalized.len());
    }

    let (search, rest) = split_natural_log_search_tail(normalized.as_slice(), false);
    search.map_or_else(
        || args.to_vec(),
        |query| build_log_shortcut_args(None, Some(query), rest, normalized.len()),
    )
}

/// Returns the index of a literal log-search separator for help/JSON routing.
pub(super) fn literal_log_search_separator_index(command: &str, args: &[String]) -> Option<usize> {
    if is_log_search_shortcut(command) || matches!(command, "logs" | "log") {
        return args.iter().position(|arg| arg == "--");
    }
    if command == "read" {
        let (resource, rest) = args.split_first()?;
        if is_recency_read_verb(resource) {
            return log_read_separator_index(rest, true).map(|index| index + 1);
        }
        return log_read_separator_index(args, false);
    }
    if is_recency_read_verb(command) {
        return log_read_separator_index(args, true);
    }
    if is_read_verb(command) {
        return log_read_separator_index(args, false);
    }
    None
}

/// Returns the separator index when read args target logs.
fn log_read_separator_index(args: &[String], allow_count: bool) -> Option<usize> {
    let resource_index = usize::from(
        allow_count
            && args
                .first()
                .is_some_and(|arg| arg.chars().all(|char| char.is_ascii_digit())),
    );
    let resource = args.get(resource_index)?;
    if !matches!(resource.as_str(), "logs" | "log") {
        return None;
    }
    args.iter()
        .enumerate()
        .skip(resource_index + 1)
        .find_map(|(index, arg)| (arg == "--").then_some(index))
}

/// Rewrites natural search text after explicit log filters.
fn log_explicit_filter_shortcut_args(args: &[String]) -> Vec<String> {
    let mut rewritten = Vec::with_capacity(args.len() + 2);
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--search" {
            let consumed = push_explicit_search_filter_value(&args[index + 1..], &mut rewritten);
            index += consumed + 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--search=")
            && !value.is_empty()
        {
            let consumed = push_inline_search_value(value, &args[index + 1..], &mut rewritten);
            index += consumed + 1;
            continue;
        }
        if matches!(arg.as_str(), "--level" | "--severity") {
            rewritten.push(arg.clone());
            let consumed = push_level_value_and_tail_search(&args[index + 1..], &mut rewritten);
            index += consumed + 1;
            continue;
        }
        if let Some(value) = inline_level_value(arg)
            && is_known_log_level(value)
        {
            rewritten.push(arg.clone());
            let consumed = push_optional_tail_search(&args[index + 1..], &mut rewritten);
            index += consumed + 1;
            continue;
        }
        if is_log_filter_with_tail_search(arg) {
            rewritten.push(arg.clone());
            let consumed = push_filter_tail_search(&args[index + 1..], &mut rewritten);
            index += consumed + 1;
            continue;
        }
        if let Some((flag, value)) = arg.split_once('=')
            && is_log_filter_with_tail_search(flag)
            && !value.is_empty()
        {
            rewritten.push(arg.clone());
            let consumed = push_optional_tail_search(&args[index + 1..], &mut rewritten);
            index += consumed + 1;
            continue;
        }
        rewritten.push(arg.clone());
        index += 1;
    }
    rewritten
}

/// Returns the inline value for log severity/level filters.
fn inline_level_value(arg: &str) -> Option<&str> {
    arg.strip_prefix("--level=")
        .or_else(|| arg.strip_prefix("--severity="))
}

/// Pushes a `--search` filter value, joining adjacent non-flag search words.
fn push_explicit_search_filter_value(args: &[String], rewritten: &mut Vec<String>) -> usize {
    if let Some((query, rest)) = split_separator_log_search_tail(args) {
        push_search_filter(query, rewritten);
        return args.len() - rest.len();
    }
    rewritten.push(String::from("--search"));
    collect_explicit_search_words(args).map_or(0, |(query, consumed)| {
        rewritten.push(query);
        consumed
    })
}

/// Pushes an inline search value, joining adjacent non-flag search words.
fn push_inline_search_value(value: &str, tail: &[String], rewritten: &mut Vec<String>) -> usize {
    let consumed = tail.iter().take_while(|arg| !arg.starts_with('-')).count();
    let mut words = Vec::with_capacity(consumed + 1);
    words.push(value.to_owned());
    words.extend(tail[..consumed].iter().cloned());
    push_search_filter(words.join(" "), rewritten);
    consumed
}

/// Pushes a severity/level filter value and optional natural search text after it.
fn push_level_value_and_tail_search(args: &[String], rewritten: &mut Vec<String>) -> usize {
    let Some(level) = args.first().filter(|value| !value.starts_with('-')) else {
        return 0;
    };
    rewritten.push(level.clone());
    if !is_known_log_level(level) {
        return 1;
    }
    push_optional_tail_search(&args[1..], rewritten) + 1
}

/// Pushes optional natural search text after an explicit value-taking log filter.
fn push_optional_tail_search(args: &[String], rewritten: &mut Vec<String>) -> usize {
    if let Some((query, rest)) = split_separator_log_search_tail(args) {
        push_search_filter(query, rewritten);
        return args.len() - rest.len();
    }
    let (search, rest) = split_natural_log_search_tail(args, true);
    search.map_or(0, |query| {
        push_search_filter(query, rewritten);
        args.len() - rest.len()
    })
}

/// Pushes a value-taking log filter and optional natural search text after it.
fn push_filter_tail_search(args: &[String], rewritten: &mut Vec<String>) -> usize {
    let Some(value) = args.first().filter(|value| !value.starts_with('-')) else {
        return 0;
    };
    rewritten.push(value.clone());
    push_optional_tail_search(&args[1..], rewritten) + 1
}

/// Collects an explicit `--search` value plus adjacent non-flag words.
fn collect_explicit_search_words(args: &[String]) -> Option<(String, usize)> {
    if args.first().is_none_or(|value| value.starts_with('-')) {
        return None;
    }
    let word_count = args.iter().take_while(|arg| !arg.starts_with('-')).count();
    Some((args[..word_count].join(" "), word_count))
}

/// Rewrites `logs error checkout failed` to `logs --level error --search "checkout failed"`.
fn build_log_shortcut_args(
    level: Option<String>,
    search: Option<String>,
    rest: Vec<String>,
    source_len: usize,
) -> Vec<String> {
    let mut rewritten = Vec::with_capacity(source_len + 4);
    if let Some(level) = level {
        rewritten.push(String::from("--level"));
        rewritten.push(level);
    }
    if let Some(query) = search {
        push_search_filter(query, &mut rewritten);
    }
    rewritten.extend(rest);
    rewritten
}

/// Pushes search as an inline flag when the query itself starts with `-`.
fn push_search_filter(query: String, rewritten: &mut Vec<String>) {
    if query.is_empty() {
        rewritten.push(String::from("--search="));
    } else if query.starts_with('-') {
        rewritten.push(format!("--search={query}"));
    } else {
        rewritten.push(String::from("--search"));
        rewritten.push(query);
    }
}

/// Splits `--` literal search text, keeping one final `--json` as output mode.
fn split_separator_log_search_tail(args: &[String]) -> Option<(String, Vec<String>)> {
    if args.first().is_none_or(|arg| arg != "--") {
        return None;
    }
    let words = &args[1..];
    let has_trailing_json_mode = words.len() > 1 && words.last().is_some_and(|arg| arg == "--json");
    let query_end = if has_trailing_json_mode {
        words.len() - 1
    } else {
        words.len()
    };
    let query = words[..query_end].join(" ");
    let rest = if has_trailing_json_mode {
        vec![String::from("--json")]
    } else {
        Vec::new()
    };
    Some((query, rest))
}

/// Splits optional natural search text after a positional severity alias.
fn split_natural_log_search_tail(
    args: &[String],
    allow_single_word: bool,
) -> (Option<String>, Vec<String>) {
    if args
        .first()
        .is_some_and(|arg| is_ambiguous_log_search_word(arg))
    {
        return (None, args.to_vec());
    }
    let query_word_count = args.iter().take_while(|arg| !arg.starts_with('-')).count();
    if query_word_count == 0 {
        return (None, args.to_vec());
    }
    let query = args[..query_word_count].join(" ");
    if !allow_single_word && query_word_count == 1 && !query.contains(' ') {
        return (None, args.to_vec());
    }
    let rest = args[query_word_count..].to_vec();
    (Some(query), rest)
}

/// Returns whether a positional value is safe to treat as a log-level shortcut.
fn is_log_level_shortcut_candidate(level: &str, tail: &[String]) -> bool {
    if !is_known_log_level(level) {
        return false;
    }
    if level.eq_ignore_ascii_case("trace") && tail.first().is_some_and(|arg| !arg.starts_with('-'))
    {
        return false;
    }
    true
}

/// Returns whether a log read filter may be followed by natural search words.
fn is_log_filter_with_tail_search(flag: &str) -> bool {
    matches!(
        flag,
        "--since"
            | "--trace"
            | "--trace-id"
            | "--project"
            | "--project-id"
            | "--release"
            | "--environment"
            | "--env"
            | "--limit"
    )
}
